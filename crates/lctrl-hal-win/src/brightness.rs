//! Public WMI control of a uniquely associated internal panel.
//!
//! WMI calls can block independently; polling does not impose a deadline on a
//! call. Selection, invocation, verification and recovery are not atomic with
//! respect to other display or brightness controllers.

use std::{collections::HashSet, thread, time::Duration};

use crate::{WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};
use lctrl_core::{ApplyMode, ChangeReport, LctrlError, PanelBrightness, Result};
use lctrl_hal::PanelBrightnessControl;

const BRIGHTNESS: &str = "WmiMonitorBrightness";
const CONNECTION: &str = "WmiMonitorConnectionParams";
const METHODS: &str = "WmiMonitorBrightnessMethods";
const POLLS: usize = 3;

#[cfg(windows)]
mod lease {
    use std::{io, ptr};

    use lctrl_core::{LctrlError, Result};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED_0, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
    };

    const NAME_BYTES: &[u8] = b"Global\\VantagePanelBrightnessV1\0";
    const fn mutex_name() -> [u16; NAME_BYTES.len()] {
        let mut output = [0; NAME_BYTES.len()];
        let mut index = 0;
        while index < NAME_BYTES.len() {
            output[index] = NAME_BYTES[index] as u16;
            index += 1;
        }
        output
    }
    const NAME: [u16; NAME_BYTES.len()] = mutex_name();

    pub(super) struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                ReleaseMutex(self.0);
                CloseHandle(self.0);
            }
        }
    }

    pub(super) fn acquire() -> Result<Guard> {
        let handle = unsafe { CreateMutexW(ptr::null(), 0, NAME.as_ptr()) };
        if handle.is_null() {
            return Err(LctrlError::Io(io::Error::last_os_error()));
        }
        match unsafe { WaitForSingleObject(handle, 2_000) } {
            WAIT_OBJECT_0 => Ok(Guard(handle)),
            WAIT_ABANDONED_0 => {
                drop(Guard(handle));
                Err(LctrlError::ChannelUnavailable {
                    channel: "a previous brightness transition was interrupted; inspect the panel before retrying".into(),
                })
            }
            WAIT_TIMEOUT => {
                unsafe { CloseHandle(handle) };
                Err(LctrlError::ChannelUnavailable {
                    channel: "another brightness transition is active".into(),
                })
            }
            _ => {
                let error = io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                Err(LctrlError::Io(error))
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowsPanelBrightness<W> {
    wmi: W,
}

impl<W> WindowsPanelBrightness<W> {
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

struct Snapshot {
    identity: String,
    panel: PanelBrightness,
}

fn unavailable(detail: &str) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("panel brightness: {detail}"),
    }
}

// Transport diagnostics can contain instance paths. Keep their error category
// without exposing a raw monitor identity to callers.
fn transport_error(operation: &str, error: LctrlError) -> LctrlError {
    let context = format!("panel brightness {operation} failed");
    match error {
        LctrlError::Unsupported { .. } => LctrlError::Unsupported { feature: context },
        LctrlError::ChannelUnavailable { .. } => {
            LctrlError::ChannelUnavailable { channel: context }
        }
        LctrlError::PermissionDenied { .. } => LctrlError::PermissionDenied { need: context },
        LctrlError::FirmwareRejected { .. } => LctrlError::FirmwareRejected { detail: context },
        LctrlError::InvalidArgument { .. } => LctrlError::InvalidArgument { detail: context },
        LctrlError::VerifyMismatch { .. } => LctrlError::VerifyMismatch {
            requested: context,
            actual: "redacted transport mismatch".into(),
        },
        LctrlError::Io(error) => {
            let detail = match error.raw_os_error() {
                Some(code) => format!("{context} (OS error {code})"),
                None => format!("{context} ({:?})", error.kind()),
            };
            LctrlError::Io(std::io::Error::new(error.kind(), detail))
        }
    }
}

fn active_identity(row: &WmiObject) -> Result<Option<&str>> {
    match row.get("Active") {
        Some(WmiValue::Bool(false)) => return Ok(None),
        Some(WmiValue::Bool(true)) => {}
        _ => return Err(unavailable("missing or malformed Active")),
    }
    match row.get("InstanceName") {
        Some(WmiValue::String(name))
            if !name.trim().is_empty() && !name.chars().any(char::is_control) =>
        {
            Ok(Some(name))
        }
        _ => Err(unavailable("missing or malformed instance identity")),
    }
}

fn active_rows(rows: &[WmiObject]) -> Result<Vec<(&str, &WmiObject)>> {
    let mut found = Vec::with_capacity(rows.len());
    let mut names = HashSet::with_capacity(rows.len());
    for row in rows {
        if let Some(name) = active_identity(row)? {
            if !names.insert(name) {
                return Err(unavailable("duplicate active instance identity"));
            }
            found.push((name, row));
        }
    }
    Ok(found)
}

fn parse_panel(row: &WmiObject) -> Result<PanelBrightness> {
    let current = match row.get("CurrentBrightness") {
        Some(WmiValue::U8(value)) if *value <= 100 => *value,
        _ => return Err(unavailable("malformed current brightness")),
    };
    let count = match row.get("Levels") {
        Some(WmiValue::U32(value)) if *value <= 101 => *value as usize,
        _ => return Err(unavailable("malformed brightness level count")),
    };
    let values = match row.get("Level") {
        Some(WmiValue::Array(values)) if !values.is_empty() && values.len() == count => values,
        _ => return Err(unavailable("malformed brightness levels")),
    };
    let mut levels = Vec::with_capacity(values.len());
    for value in values {
        match value {
            WmiValue::U8(value) if *value <= 100 && !levels.contains(value) => levels.push(*value),
            _ => return Err(unavailable("malformed brightness levels")),
        }
    }
    if !levels.contains(&current) {
        return Err(unavailable("current brightness is not a supported level"));
    }
    Ok(PanelBrightness {
        current_percent: current,
        supported_levels: levels,
    })
}

impl<W: WmiTransport> WindowsPanelBrightness<W> {
    fn rows(&self, class: &str, properties: &str) -> Result<Vec<WmiObject>> {
        self.wmi
            .query(ROOT_WMI, &format!("SELECT {properties} FROM {class}"))
            .map_err(|e| transport_error("WMI query", e))
    }

    fn snapshot(&self, require_setter: bool) -> Result<Snapshot> {
        let brightness = self.rows(
            BRIGHTNESS,
            "Active, InstanceName, CurrentBrightness, Levels, Level",
        )?;
        let connections = self.rows(CONNECTION, "Active, InstanceName, VideoOutputTechnology")?;
        let methods = if require_setter {
            Some(self.rows(METHODS, "Active, InstanceName")?)
        } else {
            None
        };
        let brightness = active_rows(&brightness)?;
        let connections = active_rows(&connections)?;
        let methods = methods.as_deref().map(active_rows).transpose()?;
        let mut selected = None;
        for (name, row) in brightness {
            let connection = connections
                .iter()
                .find(|(id, _)| *id == name)
                .ok_or_else(|| {
                    unavailable("active brightness instance lacks a unique active connection")
                })?;
            let technology = match connection.1.get("VideoOutputTechnology") {
                Some(WmiValue::U32(value)) => *value,
                _ => return Err(unavailable("malformed connection technology")),
            };
            // D3DKMDT_VOT_INTERNAL, embedded DisplayPort and embedded UDI.
            if !matches!(technology, 0x8000_0000 | 11 | 13) {
                continue;
            }
            if methods
                .as_ref()
                .is_some_and(|rows| !rows.iter().any(|(id, _)| *id == name))
            {
                return Err(unavailable(
                    "internal panel lacks an active matching methods instance",
                ));
            }
            let panel = parse_panel(row)?;
            if selected.is_some() {
                return Err(unavailable("multiple active internal panels"));
            }
            selected = Some(Snapshot {
                identity: name.to_owned(),
                panel,
            });
        }
        selected.ok_or_else(|| unavailable("no active internal panel"))
    }

    fn same_target(&self, identity: &str) -> Result<PanelBrightness> {
        let snapshot = self.snapshot(true)?;
        if snapshot.identity != identity {
            return Err(unavailable("internal panel changed"));
        }
        Ok(snapshot.panel)
    }

    fn invoke(&self, identity: &str, value: u8) -> Result<()> {
        let quoted = identity.replace('\\', "\\\\").replace('"', "\\\"");
        let path = format!("{METHODS}.InstanceName=\"{quoted}\"");
        let input = WmiObject::from([
            ("Brightness".into(), WmiValue::U8(value)),
            ("Timeout".into(), WmiValue::U32(0)),
        ]);
        let result = self
            .wmi
            .invoke_instance(ROOT_WMI, METHODS, &path, "WmiSetBrightness", &input)
            .map_err(|e| transport_error("WMI brightness setter", e))?;
        match result.get("ReturnValue") {
            Some(WmiValue::U32(0)) => Ok(()),
            Some(WmiValue::U32(code)) => Err(LctrlError::FirmwareRejected {
                detail: format!("WMI brightness setter returned {code}"),
            }),
            _ => Err(unavailable(
                "WMI brightness setter returned malformed ReturnValue",
            )),
        }
    }

    fn poll(&self, identity: &str, expected: u8) -> Result<()> {
        let mut actual = 0;
        for attempt in 0..POLLS {
            actual = self.same_target(identity)?.current_percent;
            if actual == expected {
                return Ok(());
            }
            if attempt + 1 < POLLS {
                thread::sleep(Duration::from_millis(50));
            }
        }
        Err(LctrlError::VerifyMismatch {
            requested: expected.to_string(),
            actual: actual.to_string(),
        })
    }

    fn recover(&self, identity: &str, previous: u8, requested: u8) -> Result<()> {
        let current = self.same_target(identity)?.current_percent;
        if current == previous {
            return Ok(());
        }
        if current != requested {
            return Err(unavailable(
                "recovery refused: brightness changed by another actor",
            ));
        }
        self.invoke(identity, previous)?;
        self.poll(identity, previous)
    }
}

impl<W: WmiTransport> PanelBrightnessControl for WindowsPanelBrightness<W> {
    fn brightness(&self) -> Result<PanelBrightness> {
        Ok(self.snapshot(false)?.panel)
    }

    fn set_brightness(&self, percent: u8, apply: ApplyMode) -> Result<ChangeReport<u8>> {
        // Serialize cooperating Windows CLI processes; foreign brightness
        // controllers do not honor this lease. A vanished mutex does not
        // constitute a crash-recovery journal.
        #[cfg(windows)]
        let _lease = if apply == ApplyMode::Commit {
            Some(lease::acquire()?)
        } else {
            None
        };
        let initial = self.snapshot(true)?;
        let previous = initial.panel.current_percent;
        if !initial.panel.supported_levels.contains(&percent) {
            return Err(LctrlError::InvalidArgument {
                detail: "requested brightness is not a supported panel level".into(),
            });
        }
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, percent));
        }
        let fresh = self.same_target(&initial.identity)?;
        if fresh.current_percent != previous
            || !fresh.supported_levels.contains(&percent)
            || !fresh.supported_levels.contains(&previous)
        {
            return Err(unavailable(
                "panel brightness or supported levels changed before commit",
            ));
        }
        if previous == percent {
            return Ok(ChangeReport::committed(previous, percent, previous));
        }
        let result = self
            .invoke(&initial.identity, percent)
            .and_then(|()| self.poll(&initial.identity, percent));
        match result {
            Ok(()) => Ok(ChangeReport::committed(previous, percent, percent)),
            Err(primary) => match self.recover(&initial.identity, previous, percent) {
                Ok(()) => Err(primary),
                Err(recovery) => Err(LctrlError::FirmwareRejected {
                    detail: format!(
                        "brightness change failed ({primary}); recovery failed ({recovery}); state is unverified"
                    ),
                }),
            },
        }
    }
}
