//! Native global hotkey control: RegisterHotKey backed by a message pump.
//!
//! Taking over a Lenovo hotkey means registering the same global hotkey
//! natively so the press reaches our handler instead of the official Vantage.
//! Registration requires an interactive desktop session; a daemon running
//! outside that session reports the gate honestly.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;

use crate::wide::wide;
use lctrl_core::Result;
use lctrl_hal::{HotkeyControl, HotkeyOutcome};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::Input::KeyboardAndMouse::{MOD_ALT, RegisterHotKey, UnregisterHotKey},
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        RegisterClassExW, TranslateMessage, WM_HOTKEY, WNDCLASSEXW, WS_EX_NOACTIVATE,
    },
};

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
