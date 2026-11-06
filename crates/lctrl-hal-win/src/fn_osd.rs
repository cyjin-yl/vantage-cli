//! Lenovo Fn on-screen-display toggle channel.
//!
//! The persisted preference for the Fn OSD lives in two DWORDs under the
//! `LenovoFnAndFunctionKeys` service key, and the vendor UI changes them by
//! sending *user-defined service control codes* — never by writing the registry
//! itself. `IdeaKBDManagerAddin.dll` 1.0.3.84
//! (`ToastUtils.HotkeyServiceInvoker`, IL RVA 0x2400 / 0x25c4) maps
//!
//! | indicator | on | off | value written |
//! | --- | --- | --- | --- |
//! | CapsLk OSD | 146 (`0x92`) | 147 (`0x93`) | `VantageToast\ShowCapslkOSD` (+ `ControlShowOSD\ShowCapsNumlkOSD`) |
//! | NumLk OSD | 148 (`0x94`) | 149 (`0x95`) | `VantageToast\ShowNumlkOSD` |
//!
//! and verifies each write by polling the value 10 times with 200 ms sleeps. That
//! is the shape reproduced here. NumLk is additionally gated on
//! `Capbility\SupportNumlkKey == 1` inside the vendor code
//! (`DoesSupportNumLkOSDControl`, RVA 0x276c), so a machine without that flag
//! (21VG has `0`) reports `NotSupported` and is never sent 148/149.
//!
//! Full recon, IL offsets and the live before/after probe:
//! `docs/evidence/re-osd-20261105.md`.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use lctrl_core::{LctrlError, Result};
use lctrl_hal::{FnOsdChange, FnOsdIndicator, FnOsdStatus, OsdIndicatorState};
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RegGetValueW,
};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
    SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    SERVICE_USER_DEFINED_CONTROL,
};

use crate::map_win_error;

const SERVICE_NAME: &str = "LenovoFnAndFunctionKeys";
const VANTAGE_TOAST_KEY: &str =
    r"SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys\VantageToast";
/// The CapsLk control codes also rewrite the combined Caps+Num flag here; it is
/// a mirror of `ShowCapslkOSD`, reported for transparency, never written.
const CONTROL_SHOW_OSD_KEY: &str =
    r"SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys\ControlShowOSD";
const CAPABILITY_KEY: &str = r"SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys\Capbility";

const CAPSLOCK: Indicator = Indicator {
    name: "capslock",
    value_name: "ShowCapslkOSD",
    enable_control: 146,
    disable_control: 147,
};
const NUMLOCK: Indicator = Indicator {
    name: "numlock",
    value_name: "ShowNumlkOSD",
    enable_control: 148,
    disable_control: 149,
};

/// `Capbility\SupportNumlkKey` — the vendor's NumLk OSD capability flag.
const SUPPORT_NUMLK_VALUE: &str = "SupportNumlkKey";
/// `ControlShowOSD\ShowCapsNumlkOSD` — combined Caps+Num flag, a mirror.
const COMBINED_VALUE: &str = "ShowCapsNumlkOSD";
/// The raw DWORD the service writes: `1` = indicator shown.
const VALUE_SHOWN: u32 = 1;
/// Vendor verification budget: 10 samples, 200 ms apart.
const VERIFY_ATTEMPTS: usize = 10;
const VERIFY_INTERVAL: Duration = Duration::from_millis(200);
/// `ERROR_FILE_NOT_FOUND` — the preference value is absent, which the vendor
/// reports as `NotSupported` rather than as a channel failure.
const ERROR_FILE_NOT_FOUND: u32 = 2;

struct Indicator {
    name: &'static str,
    value_name: &'static str,
    enable_control: u32,
    disable_control: u32,
}

impl Indicator {
    fn control(&self, enabled: bool) -> u32 {
        if enabled {
            self.enable_control
        } else {
            self.disable_control
        }
    }
}

/// The Lenovo Fn OSD toggle channel.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeFnOsd;

impl NativeFnOsd {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Read both indicators. Read-only and total on Windows: a missing hotkey
    /// package yields `NotSupported` indicators, never an error.
    pub fn status(&self) -> Result<FnOsdStatus> {
        let service_running = service_running();
        let capslock_value = read_dword(VANTAGE_TOAST_KEY, CAPSLOCK.value_name)?;
        let numlock_value = read_dword(VANTAGE_TOAST_KEY, NUMLOCK.value_name)?;
        let combined_value = read_dword(CONTROL_SHOW_OSD_KEY, COMBINED_VALUE)?;
        let numlock_supported = read_dword(CAPABILITY_KEY, SUPPORT_NUMLK_VALUE)?.unwrap_or(0) == 1;

        let mut capslock = indicator_status(CAPSLOCK, capslock_value, service_running, None);
        if let Some(combined) = combined_value {
            capslock.detail.push_str(&format!(
                "; ControlShowOSD\\{COMBINED_VALUE}={combined} (rewritten by the same codes)"
            ));
        }

        Ok(FnOsdStatus {
            service: SERVICE_NAME,
            service_running,
            capslock,
            numlock: indicator_status(
                NUMLOCK,
                numlock_value,
                service_running,
                Some(numlock_supported),
            ),
        })
    }

    /// Drive both indicators, verifying each written value.
    pub fn set(&self, enabled: bool, dry_run: bool) -> Result<FnOsdChange> {
        let before = self.status()?;
        let requested = if enabled {
            OsdIndicatorState::On
        } else {
            OsdIndicatorState::Off
        };
        if dry_run {
            return Ok(FnOsdChange {
                requested,
                mode: "dry_run",
                after: before.clone(),
                before,
            });
        }

        // Each target carries the state observed before any code was sent, so a
        // mismatch can restore exactly the indicators this call already drove.
        let targets: Vec<(Indicator, OsdIndicatorState)> = [
            (CAPSLOCK, before.capslock.state),
            (NUMLOCK, before.numlock.state),
        ]
        .into_iter()
        .filter(|(_, state)| matches!(state, OsdIndicatorState::On | OsdIndicatorState::Off))
        .collect();

        if targets.is_empty() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{} exposes no {SERVICE_NAME} OSD indicator on this machine \
                     ({} and {} are not supported)",
                    SERVICE_NAME, CAPSLOCK.value_name, NUMLOCK.value_name
                ),
            });
        }

        for (position, (indicator, _)) in targets.iter().enumerate() {
            let code = indicator.control(enabled);
            control_service(code)?;
            let observed = await_value(indicator, enabled)?;
            if observed.map(|value| value == VALUE_SHOWN) != Some(enabled) {
                // Readback disagreed: restore the captured previous state of the
                // indicators already driven, then report the mismatch.
                let restored = restore(&targets[..=position]);
                let now = self.status().map_or_else(
                    |error| format!("unreadable: {error}"),
                    |status| {
                        format!(
                            "capslock={} numlock={}",
                            status.capslock.state, status.numlock.state
                        )
                    },
                );
                return Err(LctrlError::VerifyMismatch {
                    requested: format!("{} {} via control {code}", indicator.name, requested),
                    actual: format!(
                        "{}={} (wanted {}); rollback {restored}; now {now}",
                        indicator.value_name,
                        observed.map_or_else(|| "absent".to_string(), |value| value.to_string()),
                        if enabled { 1 } else { 0 },
                    ),
                });
            }
        }

        Ok(FnOsdChange {
            requested,
            mode: "committed",
            after: self.status()?,
            before,
        })
    }
}

/// Best-effort rollback of the indicators a failed commit already drove. Returns
/// a human-readable outcome instead of an error so the original
/// `VerifyMismatch` survives.
fn restore(attempted: &[(Indicator, OsdIndicatorState)]) -> String {
    let mut failures = Vec::new();
    for (indicator, wanted) in attempted {
        let enabled = match *wanted {
            OsdIndicatorState::On => true,
            OsdIndicatorState::Off => false,
            OsdIndicatorState::NotSupported => continue,
        };
        if let Err(error) = control_service(indicator.control(enabled)) {
            failures.push(format!("{}: {error}", indicator.name));
        }
    }
    if failures.is_empty() {
        "control codes re-sent".into()
    } else {
        format!("control codes failed for {}", failures.join(", "))
    }
}

/// Poll the indicator's value until it matches `enabled` or the vendor's budget
/// is spent. Returns the last raw value observed (`None` = the value vanished).
fn await_value(indicator: &Indicator, enabled: bool) -> Result<Option<u32>> {
    let deadline = Instant::now() + VERIFY_INTERVAL * VERIFY_ATTEMPTS as u32;
    loop {
        let observed = read_dword(VANTAGE_TOAST_KEY, indicator.value_name)?;
        if observed.map(|value| value == VALUE_SHOWN) == Some(enabled) {
            return Ok(observed);
        }
        if Instant::now() >= deadline {
            return Ok(observed);
        }
        std::thread::sleep(VERIFY_INTERVAL);
    }
}

fn indicator_status(
    indicator: Indicator,
    value: Option<u32>,
    service_running: bool,
    capability: Option<bool>,
) -> FnOsdIndicator {
    let state = match (capability, service_running, value) {
        (Some(false), _, _) => OsdIndicatorState::NotSupported,
        (_, false, _) => OsdIndicatorState::NotSupported,
        (_, _, None) => OsdIndicatorState::NotSupported,
        (_, _, Some(raw)) if raw == VALUE_SHOWN => OsdIndicatorState::On,
        (_, _, Some(_)) => OsdIndicatorState::Off,
    };
    let detail = match state {
        OsdIndicatorState::NotSupported => match (capability, service_running, value) {
            (Some(false), _, _) => {
                format!("{CAPABILITY_KEY}\\{SUPPORT_NUMLK_VALUE}=0 on this machine")
            }
            (_, false, _) => format!("{SERVICE_NAME} is not running"),
            _ => format!("{VANTAGE_TOAST_KEY}\\{} does not exist", indicator.value_name),
        },
        _ => format!(
            "{}={}; control codes {} (on) / {} (off)",
            indicator.value_name,
            value.unwrap_or_default(),
            indicator.enable_control,
            indicator.disable_control
        ),
    };
    FnOsdIndicator {
        indicator: indicator.name,
        state,
        value_name: indicator.value_name,
        registry_value: value,
        enable_control: indicator.enable_control,
        disable_control: indicator.disable_control,
        detail,
    }
}

fn read_dword(key: &str, value: &str) -> Result<Option<u32>> {
    // Keep the UTF-16 buffers separate from the names: shadowing `key`/`name`
    // with `Vec<u16>` made every error message print raw code units instead of
    // the path the caller asked for.
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
        0 => Err(LctrlError::ChannelUnavailable {
            channel: format!("HKLM\\{key}\\{value} is {bytes} bytes; expected 4"),
        }),
        ERROR_FILE_NOT_FOUND => Ok(None),
        status => Err(map_win_error(
            status,
            &format!("HKLM\\{key}\\{value}"),
        )),
    }
}

/// Whether the owning service exists and reports `SERVICE_RUNNING`.
fn service_running() -> bool {
    let Ok((manager, handle)) = open_service(SERVICE_QUERY_STATUS) else {
        return false;
    };
    let mut status = SERVICE_STATUS::default();
    let queried = unsafe { QueryServiceStatus(handle, &mut status) } != 0;
    unsafe { CloseServiceHandle(handle) };
    unsafe { CloseServiceHandle(manager) };
    queried && status.dwCurrentState == SERVICE_RUNNING
}

/// Send one user-defined control code to the Fn service.
fn control_service(code: u32) -> Result<()> {
    let (manager, handle) = open_service(SERVICE_USER_DEFINED_CONTROL | SERVICE_QUERY_STATUS)
        .map_err(|error| {
            map_win_error(
                error,
                &format!("open {SERVICE_NAME} for a user-defined control"),
            )
        })?;
    let mut status = SERVICE_STATUS::default();
    let controlled = unsafe { ControlService(handle, code, &mut status) } != 0;
    let error = last_error();
    unsafe { CloseServiceHandle(handle) };
    unsafe { CloseServiceHandle(manager) };
    if controlled {
        Ok(())
    } else {
        Err(map_win_error(
            error,
            &format!("ControlService({SERVICE_NAME}, {code})"),
        ))
    }
}

type ScHandle = *mut c_void;

fn open_service(access: u32) -> std::result::Result<(ScHandle, ScHandle), u32> {
    let manager =
        unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(last_error());
    }
    let name = wide(SERVICE_NAME);
    let handle = unsafe { OpenServiceW(manager, name.as_ptr(), access) };
    if handle.is_null() {
        let error = last_error();
        unsafe { CloseServiceHandle(manager) };
        return Err(error);
    }
    Ok((manager, handle))
}

fn last_error() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

