//! Native global hotkey control: RegisterHotKey backed by a message pump.
//!
//! Taking over a Lenovo hotkey means registering the same global hotkey
//! natively so the press reaches our handler instead of the official Vantage.
//! Registration requires an interactive desktop session; a daemon running
//! outside that session reports the gate honestly.
//!
//! The separate, read-only vendor-state probe behind `hotkey status` lives in
//! this module too: the `LenovoFnAndFunctionKeys` service and its two
//! session-1 helpers, the `Capbility` flags, the firmware FnLock and Fn/Ctrl
//! switches (`Lenovo_BiosSetting`) and the persisted per-keyboard tables.
//! Contract and live values: `docs/evidence/re-hotkeys-20261105.md`.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;

use crate::map_win_error;
use crate::wide::wide;
use lctrl_core::{BiosItem, Result};
use lctrl_hal::{
    HotkeyCapabilities, HotkeyControl, HotkeyOutcome, HotkeySetting, HotkeyStatus, HotkeySwitch,
    HotkeySwitchState, HotkeyTable,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, HANDLE, HWND,
        INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
    },
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
    System::LibraryLoader::GetModuleHandleW,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
        RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    },
    System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    },
    UI::Input::KeyboardAndMouse::{MOD_ALT, RegisterHotKey, UnregisterHotKey},
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        RegisterClassExW, TranslateMessage, WM_HOTKEY, WNDCLASSEXW, WS_EX_NOACTIVATE,
    },
};

/// The hotkey package service and the registry keys it owns.
const HOTKEY_SERVICE: &str = "LenovoFnAndFunctionKeys";
const SERVICE_KEY: &str = r"SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys";
const CAPABILITY_KEY: &str =
    r"SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys\Capbility";
/// Per-model hotkey tables: the user copy and the machine-wide mic-mute subset.
const USER_HOTKEY_KEY: &str = r"SOFTWARE\Lenovo\Hotkey";
const MACHINE_HOTKEY_KEY: &str = r"SOFTWARE\Lenovo\LenovoFnHotkey\HIDKBMicMuteLED";
/// The two session-1 renderers the service launches.
const RENDERER_EXE: &str = "FnHotkeyUtility.exe";
const CAPS_LED_EXE: &str = "FnHotkeyCapsLKNumLK.exe";
/// The firmware items behind the two switches.
const FN_LOCK_MODE: &str = "HotkeyMode";
const FN_LOCK_PRIMARY: &str = "F1-F12AsPrimaryFunction";
const FN_CTRL_SWAP: &str = "FoolProofFnCtrl";
const FN_CTRL_SWAP_SIBLING: &str = "FnAndCtrlKeySwap";

static LIVE: AtomicBool = AtomicBool::new(false);
/// The most recent hotkey id received by the pump (for the daemon to poll).
static LAST_HOTKEY: LazyLock<Mutex<Option<u32>>> = LazyLock::new(|| Mutex::new(None));
/// Registered hotkey ids.
static REGISTERED: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// The window class holds raw pointers valid for the process lifetime.
struct StaticClass(WNDCLASSEXW);
unsafe impl Send for StaticClass {}
unsafe impl Sync for StaticClass {}

const HOTKEY_CLASS_NAME: &[u16] = &wide!("vantage_hotkey_class");
const HOTKEY_WINDOW_TITLE: &[u16] = &wide!("vantage-hotkey");

static CLS: LazyLock<StaticClass> = LazyLock::new(|| {
    let cls = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(hotkey_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: HOTKEY_CLASS_NAME.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    unsafe {
        RegisterClassExW(&cls);
    }
    StaticClass(cls)
});

/// A set of globally registered hotkeys with a background message pump.
pub struct NativeHotkey {
    pump_running: AtomicBool,
}

impl Default for NativeHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeHotkey {
    pub fn new() -> Self {
        Self {
            pump_running: AtomicBool::new(false),
        }
    }

    fn hwnd(&self) -> HWND {
        unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                CLS.0.lpszClassName,
                HOTKEY_WINDOW_TITLE.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                CLS.0.hInstance,
                std::ptr::null_mut(),
            )
        }
    }

    fn ensure_pump(&self) {
        if self.pump_running.swap(true, Ordering::AcqRel) {
            return;
        }
        thread::spawn(move || {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            unsafe {
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    // Route the message to the WNDPROC, which records WM_HOTKEY.
                    DispatchMessageW(&msg);
                }
            }
        });
    }
}

impl HotkeyControl for NativeHotkey {
    fn register(&self, id: u32, vk: u32, dry_run: bool) -> Result<HotkeyOutcome> {
        if dry_run {
            return Ok(HotkeyOutcome {
                state: "dry_run".into(),
                id,
                detail: "RegisterHotKey not attempted during dry-run".into(),
            });
        }
        self.ensure_pump();
        let hwnd = self.hwnd();
        let ok = unsafe { RegisterHotKey(hwnd, id as i32, MOD_ALT, vk) };
        if ok != 0 {
            REGISTERED.lock().unwrap().insert(id);
            LIVE.store(true, Ordering::Release);
            Ok(HotkeyOutcome {
                state: "registered".into(),
                id,
                detail: "global Alt-hotkey live; press dispatches to the daemon".into(),
            })
        } else {
            Ok(HotkeyOutcome {
                state: "gated".into(),
                id,
                detail: "RegisterHotKey failed; no interactive desktop session / hotkey already \
                         owned"
                    .into(),
            })
        }
    }

    fn unregister(&self, id: u32) -> Result<bool> {
        let hwnd = self.hwnd();
        let ok = unsafe { UnregisterHotKey(hwnd, id as i32) };
        let mut reg = REGISTERED.lock().unwrap();
        let removed = reg.remove(&id);
        if removed && reg.is_empty() {
            LIVE.store(false, Ordering::Release);
        }
        Ok(ok != 0 || removed)
    }

    fn registered(&self) -> Vec<u32> {
        let mut v: Vec<u32> = REGISTERED.lock().unwrap().iter().copied().collect();
        v.sort();
        v
    }

    fn status(&self) -> Result<HotkeyStatus> {
        self.read_status(true)
    }
}

impl NativeHotkey {
    /// The package state behind `hotkey status`.
    ///
    /// `read_firmware` controls the single `Lenovo_BiosSetting` enumeration:
    /// the command reads it, while the capability probe skips it so a
    /// `vantage capability`/`info` run does not pay for a second BIOS query
    /// (the HAL probe already enumerates the settings once for `channel.bios`).
    fn read_status(&self, read_firmware: bool) -> Result<HotkeyStatus> {
        let mut notes = Vec::new();
        match key_exists(HKEY_LOCAL_MACHINE, SERVICE_KEY) {
            Ok(true) => {}
            Ok(false) => notes.push(format!(
                "HKLM\\{SERVICE_KEY} does not exist; the Lenovo Fn package is not installed"
            )),
            Err(error) => notes.push(format!("HKLM\\{SERVICE_KEY}: {error}")),
        }
        let service_running = service_running(HOTKEY_SERVICE);
        let utility_version = match read_string(SERVICE_KEY, "UtilityVersion") {
            Ok(version) => version,
            Err(error) => {
                notes.push(format!("HKLM\\{SERVICE_KEY}\\UtilityVersion: {error}"));
                None
            }
        };
        let capabilities = read_capabilities(&mut notes);
        let (utility_running, caps_led_running) = renderer_state(&mut notes);
        let (fn_lock, fn_ctrl_swap) = if read_firmware {
            read_switches(&mut notes)
        } else {
            let detail = "firmware switches are not read by the capability probe; \
                          run `vantage hotkey status`";
            (unknown_switch(detail), unknown_switch(detail))
        };
        let tables = read_tables(&mut notes);
        Ok(HotkeyStatus {
            service: HOTKEY_SERVICE,
            service_running,
            utility_version,
            utility_running,
            caps_led_running,
            capabilities,
            fn_lock,
            fn_ctrl_swap,
            tables,
            notes,
        })
    }

    /// Package state without the firmware query, for the capability probe.
    pub(crate) fn capability_status(&self) -> Result<HotkeyStatus> {
        self.read_status(false)
    }
}

/// Poll and clear the most recent hotkey press seen by the pump.
pub fn take_last_hotkey() -> Option<u32> {
    LAST_HOTKEY.lock().unwrap().take()
}

unsafe extern "system" fn hotkey_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_HOTKEY {
            let id = wparam as u32;
            *LAST_HOTKEY.lock().unwrap() = Some(id);
            0
        } else {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

/// Read the `Capbility` flags the Fn package keys its hotkey behaviour on.
///
/// A missing value is `false` (the flag is absent, not zero); a registry error
/// is reported as a note and also reads as `false`, so a failed read can never
/// claim support.
fn read_capabilities(notes: &mut Vec<String>) -> HotkeyCapabilities {
    HotkeyCapabilities {
        support_fn_lock: capability_flag("SupportFnLockVer", notes),
        support_keyboard_layout: capability_flag("SupportKeyboardLocationLayout", notes),
        support_numlock_key: capability_flag("SupportNumlkKey", notes),
        user_changed_caps_numlock: capability_flag("IsUserChangedCapsNumLKStatus", notes),
    }
}

fn capability_flag(name: &str, notes: &mut Vec<String>) -> bool {
    match read_dword(CAPABILITY_KEY, name) {
        Ok(Some(value)) => value != 0,
        Ok(None) => false,
        Err(error) => {
            notes.push(format!("HKLM\\{CAPABILITY_KEY}\\{name}: {error}"));
            false
        }
    }
}

/// Whether the two session-1 renderers named by the service are running.
///
/// The check is machine-wide, so a session-0 daemon still sees the interactive
/// session's renderer. A snapshot failure is a note, never a fabricated state.
fn renderer_state(notes: &mut Vec<String>) -> (bool, bool) {
    match running_images() {
        Ok(images) => (
            images.iter().any(|image| image.eq_ignore_ascii_case(RENDERER_EXE)),
            images.iter().any(|image| image.eq_ignore_ascii_case(CAPS_LED_EXE)),
        ),
        Err(error) => {
            notes.push(format!("process snapshot: {error}"));
            (false, false)
        }
    }
}

/// Every running image name, from a Toolhelp process snapshot.
fn running_images() -> Result<Vec<String>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(map_win_error(
            last_error(),
            "CreateToolhelp32Snapshot(SNAPPROCESS)",
        ));
    }
    let snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(map_win_error(last_error(), "Process32FirstW"));
    }
    let mut images = Vec::new();
    loop {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        images.push(String::from_utf16_lossy(&entry.szExeFile[..length]));
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            return Ok(images);
        }
    }
}

/// Read both firmware switches from one `Lenovo_BiosSetting` enumeration.
///
/// One WMI query serves all four items; a failed query leaves both switches
/// `unknown` with the firmware's own error in the notes.
fn read_switches(notes: &mut Vec<String>) -> (HotkeySwitch, HotkeySwitch) {
    let controller = crate::WindowsBiosController::new(crate::NativeWmi);
    let settings = match controller.settings() {
        Ok(settings) => settings,
        Err(error) => {
            notes.push(format!("Lenovo_BiosSetting read failed: {error}"));
            let detail = "firmware settings could not be read";
            return (unknown_switch(detail), unknown_switch(detail));
        }
    };
    (fn_lock_switch(&settings), fn_ctrl_swap_switch(&settings))
}

fn unknown_switch(detail: &str) -> HotkeySwitch {
    HotkeySwitch {
        state: HotkeySwitchState::Unknown,
        settings: Vec::new(),
        detail: detail.into(),
    }
}

/// `HotkeyMode` + `F1-F12AsPrimaryFunction` define the F1-F12 row mode; any
/// other pair is reported `unknown` with both raw values attached.
fn fn_lock_switch(settings: &[BiosItem]) -> HotkeySwitch {
    let mode = find_setting(settings, FN_LOCK_MODE);
    let primary = find_setting(settings, FN_LOCK_PRIMARY);
    let items = switch_items(&[mode, primary]);
    let raw = |item: Option<&BiosItem>| {
        item.map(|item| item.value.clone())
            .unwrap_or_else(|| "<absent>".into())
    };
    let raw_mode = raw(mode);
    let raw_primary = raw(primary);
    let values = (
        mode.map(|item| item.value.to_ascii_lowercase()),
        primary.map(|item| item.value.to_ascii_lowercase()),
    );
    let (state, detail) = match values {
        (Some(mode), Some(primary)) if mode == "disable" && primary == "enable" => (
            HotkeySwitchState::On,
            format!(
                "{FN_LOCK_MODE}=Disable with {FN_LOCK_PRIMARY}=Enable: \
                 F1-F12 act as standard function keys (Fn-lock on)"
            ),
        ),
        (Some(mode), Some(primary)) if mode == "enable" && primary == "disable" => (
            HotkeySwitchState::Off,
            format!(
                "{FN_LOCK_MODE}=Enable with {FN_LOCK_PRIMARY}=Disable: \
                 F1-F12 need Fn (Fn-lock off)"
            ),
        ),
        (Some(mode), Some(primary)) => (
            HotkeySwitchState::Unknown,
            format!(
                "{FN_LOCK_MODE}={raw_mode} with {FN_LOCK_PRIMARY}={raw_primary} is not a \
                 documented pair (lowercased {mode} / {primary}); reporting the raw values"
            ),
        ),
        _ if items.is_empty() => (
            HotkeySwitchState::Unknown,
            format!("neither {FN_LOCK_MODE} nor {FN_LOCK_PRIMARY} is exposed by this firmware"),
        ),
        _ => (
            HotkeySwitchState::Unknown,
            format!("one of {FN_LOCK_MODE} / {FN_LOCK_PRIMARY} is absent; the pair is incomplete"),
        ),
    };
    HotkeySwitch {
        state,
        settings: items,
        detail,
    }
}

/// `FoolProofFnCtrl` is the item `kbd fn-ctrl-swap` writes; `FnAndCtrlKeySwap`
/// is its sibling on this SKU and is reported, never treated as the state
/// unless the primary item is absent.
fn fn_ctrl_swap_switch(settings: &[BiosItem]) -> HotkeySwitch {
    let primary = find_setting(settings, FN_CTRL_SWAP);
    let sibling = find_setting(settings, FN_CTRL_SWAP_SIBLING);
    let items = switch_items(&[primary, sibling]);
    let (source, item) = match (primary, sibling) {
        (Some(item), _) => (FN_CTRL_SWAP, Some(item)),
        (None, Some(item)) => (FN_CTRL_SWAP_SIBLING, Some(item)),
        (None, None) => ("", None),
    };
    let raw = item
        .map(|item| item.value.clone())
        .unwrap_or_else(|| "<absent>".into());
    let (state, detail) = match item.map(|item| item.value.to_ascii_lowercase()) {
        Some(value) if value == "enable" => (
            HotkeySwitchState::On,
            format!("{source}=Enable: Fn and Ctrl are swapped"),
        ),
        Some(value) if value == "disable" => (
            HotkeySwitchState::Off,
            format!("{source}=Disable: Fn and Ctrl are not swapped"),
        ),
        Some(value) => (
            HotkeySwitchState::Unknown,
            format!("{source}={raw} is not a documented value (lowercased {value})"),
        ),
        None => (
            HotkeySwitchState::Unknown,
            format!("neither {FN_CTRL_SWAP} nor {FN_CTRL_SWAP_SIBLING} is exposed by this firmware"),
        ),
    };
    HotkeySwitch {
        state,
        settings: items,
        detail,
    }
}

fn find_setting<'a>(settings: &'a [BiosItem], name: &str) -> Option<&'a BiosItem> {
    settings
        .iter()
        .find(|item| item.name.eq_ignore_ascii_case(name))
}

fn switch_items(items: &[Option<&BiosItem>]) -> Vec<HotkeySetting> {
    items
        .iter()
        .flatten()
        .map(|item| HotkeySetting {
            name: item.name.clone(),
            value: item.value.clone(),
        })
        .collect()
}

/// Enumerate the persisted per-model tables from both stores.
///
/// `HKCU` is the invoking user's copy (the renderer runs in that session);
/// `HKLM` is the machine-wide mic-mute subset. A missing key is an empty list,
/// not an error, so a machine without the package still answers.
fn read_tables(notes: &mut Vec<String>) -> Vec<HotkeyTable> {
    let mut tables = Vec::new();
    for (scope, root, path) in [
        ("current_user", HKEY_CURRENT_USER, USER_HOTKEY_KEY),
        ("machine", HKEY_LOCAL_MACHINE, MACHINE_HOTKEY_KEY),
    ] {
        match enumerate_tables(root, path) {
            Ok(found) => tables.extend(found.into_iter().map(|(device, keys)| HotkeyTable {
                scope,
                device,
                mappings: keys.len(),
                keys,
            })),
            Err(error) => notes.push(format!("{path}: {error}")),
        }
    }
    tables
}

fn enumerate_tables(root: HKEY, path: &str) -> Result<Vec<(String, Vec<String>)>> {
    let path_wide = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(root, path_wide.as_ptr(), 0, KEY_READ, &mut key) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(Vec::new());
    }
    if status != 0 {
        return Err(map_win_error(status, &format!("open {path}")));
    }
    let key = RegKey(key);
    let mut tables = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(tables);
        }
        if status != 0 {
            return Err(map_win_error(status, &format!("enumerate {path}")));
        }
        index += 1;
        let device = String::from_utf16_lossy(&name[..length as usize]);
        let keys = enumerate_values(key.0, &device)?;
        tables.push((device, keys));
    }
}

fn enumerate_values(parent: HKEY, device: &str) -> Result<Vec<String>> {
    let device_wide = wide(device);
    let mut handle: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(parent, device_wide.as_ptr(), 0, KEY_READ, &mut handle) };
    if status != 0 {
        return Err(map_win_error(status, &format!("open {device}")));
    }
    let key = RegKey(handle);
    let mut values = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(values);
        }
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("enumerate values of {device}"),
            ));
        }
        index += 1;
        values.push(String::from_utf16_lossy(&name[..length as usize]));
    }
}

/// Owned `HKEY` that closes on drop.
struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

/// Owned handle that closes on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn key_exists(root: HKEY, path: &str) -> Result<bool> {
    let path_wide = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(root, path_wide.as_ptr(), 0, KEY_READ, &mut key) };
    match status {
        0 => {
            unsafe { RegCloseKey(key) };
            Ok(true)
        }
        ERROR_FILE_NOT_FOUND => Ok(false),
        status => Err(map_win_error(status, &format!("open HKLM\\{path}"))),
    }
}

fn read_dword(key: &str, value: &str) -> Result<Option<u32>> {
    let key_wide = wide(key);
    let name_wide = wide(value);
    let mut data = 0u32;
    let mut bytes = u32::try_from(std::mem::size_of_val(&data)).expect("u32 size fits");
    let mut kind: REG_VALUE_TYPE = 0;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut kind,
            (&mut data as *mut u32).cast::<c_void>(),
            &mut bytes,
        )
    };
    match status {
        0 if bytes == 4 => Ok(Some(data)),
        0 => Err(lctrl_core::LctrlError::ChannelUnavailable {
            channel: format!("HKLM\\{key}\\{value} is {bytes} bytes; expected 4"),
        }),
        ERROR_FILE_NOT_FOUND => Ok(None),
        status => Err(map_win_error(status, &format!("HKLM\\{key}\\{value}"))),
    }
}

fn read_string(key: &str, value: &str) -> Result<Option<String>> {
    let key_wide = wide(key);
    let name_wide = wide(value);
    let mut buffer = [0u16; 512];
    let mut bytes = u32::try_from(std::mem::size_of_val(&buffer)).expect("buffer size fits");
    let mut kind: REG_VALUE_TYPE = 0;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            RRF_RT_REG_SZ,
            &mut kind,
            buffer.as_mut_ptr().cast::<c_void>(),
            &mut bytes,
        )
    };
    match status {
        0 => {
            let length = buffer
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(buffer.len());
            Ok(Some(String::from_utf16_lossy(&buffer[..length])))
        }
        ERROR_FILE_NOT_FOUND => Ok(None),
        status => Err(map_win_error(status, &format!("HKLM\\{key}\\{value}"))),
    }
}

/// Whether a named service exists and reports `SERVICE_RUNNING`.
fn service_running(name: &str) -> bool {
    let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return false;
    }
    let name = wide(name);
    let service = unsafe { OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if service.is_null() {
        unsafe { CloseServiceHandle(manager) };
        return false;
    }
    let mut status = SERVICE_STATUS::default();
    let queried = unsafe { QueryServiceStatus(service, &mut status) } != 0;
    unsafe { CloseServiceHandle(service) };
    unsafe { CloseServiceHandle(manager) };
    queried && status.dwCurrentState == SERVICE_RUNNING
}

fn last_error() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
}

/// Runtime NUL-terminated UTF-16 for the registry and service names.
///
/// The `wide!` macro imported from `crate::wide` only takes literals; these
/// paths are formatted from the constants above.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
