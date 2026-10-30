//! Windows MachineFix backend: a transparent ruleset of service-state rules.
//!
//! Every rule is declared in [`ruleset`] and visible through `fix scan`. The
//! probe is a read-only service-state query; the repair is an allowlisted
//! service start/stop with readback; the rollback is the inverse. The local
//! ledger is persisted as JSON under the user profile and read back by
//! `fix track`. No vendor issue database or opaque fixer is used.

use std::{
    env,
    fs,
    io::Write as _,
    path::PathBuf,
    sync::Mutex,
};

use lctrl_core::{
    ApplyMode, FixIssue, FixLedger, FixLedgerEntry, FixObservation, FixRepairKind, FixReport,
    FixRule, LctrlError, Result,
};
use lctrl_hal::MachineFixControl;

/// Service states (SERVICE_* constants).
const SERVICE_STOPPED: u32 = 1;
const SERVICE_RUNNING: u32 = 4;

/// A transparent MachineFix backend over the Windows service control APIs.
pub struct WindowsMachineFix {
    ledger: Mutex<FixLedger>,
}

impl Default for WindowsMachineFix {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsMachineFix {
    pub fn new() -> Self {
        Self {
            ledger: Mutex::new(load_ledger()),
        }
    }

    /// The complete declared ruleset. Each rule targets one service and repairs
    /// a stopped state by starting it (rolling back by stopping it).
    fn ruleset() -> Vec<FixRule> {
        service_state_rules(&[
            ("wuauserv", "Windows Update Agent"),
            ("SysMain", "Superfetch (SysMain)"),
        ])
    }

    fn observe(name: &str) -> Result<(bool, String)> {
        let state = service_status(name)?;
        let present = state == SERVICE_STOPPED;
        Ok((
            present,
            format!("service {name} state = {state} (4 = running, 1 = stopped)"),
        ))
    }
}

/// Build one transparent service-state rule per service.
fn service_state_rules(services: &[(&str, &str)]) -> Vec<FixRule> {
    services
        .iter()
        .filter_map(|(name, label)| {
            FixRule::new(
                FixIssue {
                    id: format!("service.stopped.{name}"),
                    title: format!("{label} is stopped"),
                    detail: format!(
                        "Detects service {name} in the stopped state; the repair starts it and reads back the running state. Rollback stops it again."
                    ),
                },
                FixRepairKind::StartService {
                    name: (*name).to_string(),
                },
                FixRepairKind::StopService {
                    name: (*name).to_string(),
                },
            )
            .ok()
        })
        .collect()
}

impl MachineFixControl for WindowsMachineFix {
    fn rules(&self) -> Vec<FixRule> {
        Self::ruleset()
    }

    fn scan(&self) -> Result<Vec<FixObservation>> {
        let mut observations = Vec::new();
        for rule in Self::ruleset() {
            let name = service_name(&rule.repair);
            match Self::observe(&name) {
                Ok((present, observed)) => observations.push(FixObservation {
                    issue: rule.issue,
                    present,
                    observed,
                }),
                Err(error) => observations.push(FixObservation {
                    issue: rule.issue,
                    present: false,
                    observed: format!("probe failed: {error}"),
                }),
            }
        }
        Ok(observations)
    }

    fn fix(&self, issue_id: &str, mode: ApplyMode) -> Result<FixReport> {
        let rule = Self::ruleset()
            .into_iter()
            .find(|rule| rule.issue.id == issue_id)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("unknown fix issue {issue_id}; run `fix scan` for the declared ruleset"),
            })?;
        let name = service_name(&rule.repair);
        let previous_state = service_status(&name)?;
        let previous = format!("service {name} state = {previous_state}");
        if mode == ApplyMode::DryRun {
            return Ok(FixReport {
                issue_id: rule.issue.id,
                mode,
                previous,
                requested: rule.repair,
                actual: None,
                rollback: rule.rollback,
            });
        }
        apply_repair(&rule.repair)?;
        let actual_state = service_status(&name)?;
        let actual = format!("service {name} state = {actual_state}");
        require_readback(&rule.repair, actual_state)?;
        let entry = FixLedgerEntry {
            issue_id: rule.issue.id.clone(),
            applied_at_unix_ms: now_unix_ms(),
            repair: rule.repair.clone(),
            rollback: rule.rollback.clone(),
            status: "applied".into(),
        };
        persist_ledger_entry(&self.ledger, entry)?;
        Ok(FixReport {
            issue_id: rule.issue.id,
            mode,
            previous,
            requested: rule.repair,
            actual: Some(actual),
            rollback: rule.rollback,
        })
    }

    fn track(&self) -> FixLedger {
        self.ledger
            .lock()
            .map(|ledger| ledger.clone())
            .unwrap_or_default()
    }
}

fn service_name(repair: &FixRepairKind) -> String {
    match repair {
        FixRepairKind::StartService { name } | FixRepairKind::StopService { name } => name.clone(),
    }
}

fn apply_repair(repair: &FixRepairKind) -> Result<()> {
    match repair {
        FixRepairKind::StartService { name } => start_service(name),
        FixRepairKind::StopService { name } => stop_service(name),
    }
}

fn require_readback(repair: &FixRepairKind, state: u32) -> Result<()> {
    let expected = match repair {
        FixRepairKind::StartService { .. } => SERVICE_RUNNING,
        FixRepairKind::StopService { .. } => SERVICE_STOPPED,
    };
    if state != expected {
        return Err(LctrlError::VerifyMismatch {
            requested: format!("service state {expected}"),
            actual: format!("service state {state}"),
        });
    }
    Ok(())
}

fn load_ledger() -> FixLedger {
    read_ledger_file().unwrap_or_default()
}

fn ledger_path() -> Option<PathBuf> {
    let base = env::var("USERPROFILE").ok()?;
    Some(PathBuf::from(base).join(".vantage-fix-ledger.json"))
}

fn read_ledger_file() -> Option<FixLedger> {
    let path = ledger_path()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn persist_ledger_entry(ledger: &Mutex<FixLedger>, entry: FixLedgerEntry) -> Result<()> {
    let mut guard = ledger
        .lock()
        .map_err(|_| LctrlError::InvalidArgument {
            detail: "fix ledger lock was poisoned".into(),
        })?;
    guard.entries.push(entry);
    let path = ledger_path().ok_or_else(|| LctrlError::InvalidArgument {
        detail: "USERPROFILE is unset; cannot persist the fix ledger".into(),
    })?;
    let text = serde_json::to_string_pretty(&*guard)
        .map_err(|error| LctrlError::InvalidArgument {
            detail: format!("serializing fix ledger failed: {error}"),
        })?;
    write_all(&path, text.as_bytes())
}

fn write_all(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| LctrlError::InvalidArgument {
            detail: format!("opening fix ledger failed: {error}"),
        })?;
    file.write_all(bytes).and_then(|_| file.flush()).map_err(|error| {
        LctrlError::InvalidArgument {
            detail: format!("writing fix ledger failed: {error}"),
        }
    })
}

fn now_unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(windows)]
mod services {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Foundation::{GetLastError, INVALID_HANDLE_VALUE},
        System::Services::{
            CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
            StartServiceW, SERVICE_ALL_ACCESS, SC_MANAGER_CONNECT,
            SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_STOP, SERVICE_STATUS,
        },
    };

    use lctrl_core::LctrlError;

    type ScHandle = *mut std::ffi::c_void;

    pub fn status(name: &str) -> Result<u32, LctrlError> {
        let (manager, handle) = open(name, SERVICE_QUERY_STATUS)?;
        let mut status = SERVICE_STATUS::default();
        let ok = unsafe { QueryServiceStatus(handle, &mut status) } != 0;
        close(manager, handle);
        if ok {
            Ok(status.dwCurrentState)
        } else {
            Err(map_error("QueryServiceStatus"))
        }
    }

    pub fn start(name: &str) -> Result<(), LctrlError> {
        let (manager, handle) = open(name, SERVICE_START)?;
        let ok = unsafe { StartServiceW(handle, 0, std::ptr::null()) } != 0;
        close(manager, handle);
        if ok {
            Ok(())
        } else {
            // ERROR_SERVICE_ALREADY_RUNNING (1056) is a benign no-op for a start.
            if unsafe { GetLastError() } == 1056 {
                Ok(())
            } else {
                Err(map_error("StartServiceW"))
            }
        }
    }

    pub fn stop(name: &str) -> Result<(), LctrlError> {
        let (manager, handle) = open(name, SERVICE_ALL_ACCESS)?;
        let mut service_status = SERVICE_STATUS::default();
        let controlled = unsafe { ControlService(handle, SERVICE_STOP, &mut service_status) } != 0;
        let queried = unsafe { QueryServiceStatus(handle, &mut service_status) } != 0;
        close(manager, handle);
        if controlled && queried && service_status.dwCurrentState == 1 {
            Ok(())
        } else if !controlled && unsafe { GetLastError() } == 1062 {
            // ERROR_SERVICE_NOT_ACTIVE (1062) is a benign no-op for a stop.
            Ok(())
        } else {
            Err(map_error("ControlService(stop)"))
        }
    }

    fn open(name: &str, access: u32) -> Result<(ScHandle, ScHandle), LctrlError> {
        let wide = wide(name);
        let manager = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
        if manager == INVALID_HANDLE_VALUE {
            return Err(map_error("OpenSCManagerW"));
        }
        let handle = unsafe { OpenServiceW(manager, wide.as_ptr(), access) };
        if handle == INVALID_HANDLE_VALUE {
            unsafe { CloseServiceHandle(manager) };
            return Err(map_error("OpenServiceW"));
        }
        Ok((manager, handle))
    }

    fn close(manager: ScHandle, handle: ScHandle) {
        unsafe { CloseServiceHandle(handle) };
        unsafe { CloseServiceHandle(manager) };
    }

    fn wide(name: &str) -> Vec<u16> {
        OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn map_error(operation: &str) -> LctrlError {
        LctrlError::ChannelUnavailable {
            channel: format!("{operation}: win32 error 0x{:08x}", unsafe { GetLastError() }),
        }
    }
}

#[cfg(windows)]
fn service_status(name: &str) -> Result<u32> {
    services::status(name)
}

#[cfg(windows)]
fn start_service(name: &str) -> Result<()> {
    services::start(name)
}

#[cfg(windows)]
fn stop_service(name: &str) -> Result<()> {
    services::stop(name)
}

#[cfg(not(windows))]
fn service_status(_name: &str) -> Result<u32> {
    Err(LctrlError::ChannelUnavailable {
        channel: "service control is unavailable on this platform".into(),
    })
}

#[cfg(not(windows))]
fn start_service(_name: &str) -> Result<()> {
    Err(LctrlError::ChannelUnavailable {
        channel: "service control is unavailable on this platform".into(),
    })
}

#[cfg(not(windows))]
fn stop_service(_name: &str) -> Result<()> {
    Err(LctrlError::ChannelUnavailable {
        channel: "service control is unavailable on this platform".into(),
    })
}

#[cfg(test)]
mod ruleset_tests {
    use super::*;

    #[test]
    fn ruleset_is_transparent_and_well_formed() {
        let rules = WindowsMachineFix::ruleset();
        assert_eq!(rules.len(), 2);
        let wua = &rules[0];
        assert_eq!(wua.issue.id, "service.stopped.wuauserv");
        assert!(matches!(wua.repair, FixRepairKind::StartService { ref name } if name == "wuauserv"));
        assert!(matches!(wua.rollback, FixRepairKind::StopService { ref name } if name == "wuauserv"));
        for rule in &rules {
            rule.issue.validate().unwrap();
            rule.repair.validate().unwrap();
            rule.rollback.validate().unwrap();
        }
    }
}
