//! SmartSense human-presence settings control.
//!
//! Contract recovered from ThinkSmartSenseAddin.dll 1.0.0.77 (IL) and
//! LnvSST.dll/SmartSense.exe 1.0.8.5 on TB14P-EZRA; see
//! `docs/RE-interfaces.md` §13. Writes go to the
//! `\\.\pipe\com.lenovo.smartsense.settings` named pipe (raw little-endian
//! `u32 code + u32 arg`, response `i32`: 0 = rejected, nonzero = queued,
//! -1 = server exception); the service persists to
//! `HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`, which
//! is also the readback channel the addin itself uses for reads.

use std::ffi::c_void;
use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, Result, SenseDistance, SenseState, SenseWait,
};
use lctrl_hal::{SenseControl, SenseFeature, poll_readback};

const PIPE_PATH: &str = r"\\.\pipe\com.lenovo.smartsense.settings";
const HPD_KEY: &str = r"SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(3000);

/// Sealed seam over the settings pipe so [`WindowsSenseControl`] stays
/// testable without the SmartSense service.
pub trait SensePipe: Send + Sync {
    /// Send one `u32 code + u32 arg` frame and return the server `i32`
    /// result (0 = rejected, nonzero = accepted, -1 = server exception).
    fn call(&self, code: u32, arg: u32) -> Result<i32>;
}

/// Sealed seam over the HPD registry store used for reads and readback.
pub trait HpdSettingsStore: Send + Sync {
    fn read_state(&self) -> Result<SenseState>;
}

pub struct WindowsSenseControl<P: SensePipe, S: HpdSettingsStore> {
    pipe: P,
    store: S,
}

impl<P: SensePipe, S: HpdSettingsStore> WindowsSenseControl<P, S> {
    #[must_use]
    pub const fn new(pipe: P, store: S) -> Self {
        Self { pipe, store }
    }

    fn write_feature(&self, feature: SenseFeature) -> Result<()> {
        let (code, arg) = feature.command();
        match self.pipe.call(code, arg)? {
            1 => Ok(()),
            0 => Err(LctrlError::FirmwareRejected {
                detail: format!("SmartSense pipe rejected command 0x{code:08x} arg {arg}"),
            }),
            -1 => Err(LctrlError::ChannelUnavailable {
                channel: format!("SmartSense pipe raised an exception for command 0x{code:08x}"),
            }),
            other => Err(LctrlError::ChannelUnavailable {
                channel: format!("SmartSense pipe returned unexpected result {other}"),
            }),
        }
    }
}

impl<P: SensePipe, S: HpdSettingsStore> SenseControl for WindowsSenseControl<P, S> {
    fn sense_state(&self) -> Result<SenseState> {
        self.store.read_state()
    }

    fn set_sense_feature(
        &self,
        feature: SenseFeature,
        apply: ApplyMode,
    ) -> Result<ChangeReport<SenseState>> {
        let previous = self.store.read_state()?;
        let mut requested = previous;
        requested = apply_feature_to_state(feature, requested);
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.write_feature(feature)?;
        let actual = if feature_readback_verifiable(feature) {
            poll_readback(&requested, 10, Duration::from_millis(50), || {
                self.store.read_state()
            })?
        } else {
            // Attention tracking has no observed persistence key on this
            // model; the write is fire-and-verify-later, so report the
            // registry state as-is without polling for a match.
            self.store.read_state()?
        };
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

/// Update the modeled state as the registry should reflect after `feature`.
/// Attention tracking has no known persistence key, so it is left unchanged
/// here; the caller still reports the intent via the pipe write.
fn apply_feature_to_state(feature: SenseFeature, mut state: SenseState) -> SenseState {
    match feature {
        SenseFeature::LockOnLeave(enabled) => state.lock_on_leave = Some(enabled),
        SenseFeature::LeaveDistance(distance) => state.leave_distance = Some(distance),
        SenseFeature::LeaveWait(wait) => state.leave_wait = Some(wait),
        SenseFeature::WakeOnApproach(enabled) => state.wake_on_approach = Some(enabled),
        SenseFeature::ApproachDistance(distance) => state.approach_distance = Some(distance),
        SenseFeature::PauseVideo(enabled) => state.pause_video = Some(enabled),
        SenseFeature::PrivacyAlert(enabled) => state.privacy_alert = Some(enabled),
        SenseFeature::AttentionTracking(_) => {}
    }
    state
}

const fn feature_readback_verifiable(feature: SenseFeature) -> bool {
    !matches!(feature, SenseFeature::AttentionTracking(_))
}

/// Native named-pipe transport. One connection per call, matching the
/// addin's `NamedPipeClientStream` usage.
pub struct NativeSensePipe;

impl SensePipe for NativeSensePipe {
    fn call(&self, code: u32, arg: u32) -> Result<i32> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{CreateFileW, OPEN_EXISTING},
        };
        let wide: Vec<u16> = std::ffi::OsStr::new(PIPE_PATH)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "SmartSense settings pipe is not reachable (is the SmartSense service running?): {}",
                    std::io::Error::last_os_error()
                ),
            });
        }
        let result = pipe_call(handle, code, arg);
        unsafe {
            CloseHandle(handle);
        }
        result
    }
}

fn pipe_call(handle: *mut c_void, code: u32, arg: u32) -> Result<i32> {
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    let request = [code.to_le_bytes(), arg.to_le_bytes()].concat();
    let mut written = 0;
    let write_ok = unsafe {
        WriteFile(
            handle,
            request.as_ptr(),
            request.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if write_ok == 0 || written as usize != request.len() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "SmartSense pipe write failed: {}",
                std::io::Error::last_os_error()
            ),
        });
    }
    let mut response = [0u8; 4];
    let mut read = 0;
    let read_ok = unsafe {
        ReadFile(
            handle,
            response.as_mut_ptr(),
            response.len() as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if read_ok == 0 || read != 4 {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "SmartSense pipe response read failed: {}",
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(i32::from_le_bytes(response))
}

/// Native reader for the `Parameters\HPD` DWORD store.
pub struct NativeHpdStore;

impl HpdSettingsStore for NativeHpdStore {
    fn read_state(&self) -> Result<SenseState> {
        let mut state = SenseState::default();
        state.lock_on_leave = read_bool(HPD_KEY, "PresentLeave", BoolEncoding::Legacy01)?;
        state.leave_distance =
            read_u32(HPD_KEY, "LeaveDistance")?.and_then(SenseDistance::from_raw);
        state.leave_wait = read_u32(HPD_KEY, "LeaveWait")?.and_then(SenseWait::from_raw);
        state.wake_on_approach = read_bool(HPD_KEY, "Approach", BoolEncoding::Legacy01)?;
        state.approach_distance =
            read_u32(HPD_KEY, "ApproachDistance")?.and_then(SenseDistance::from_raw);
        state.pause_video = read_bool(HPD_KEY, "Video", BoolEncoding::OneTwo)?;
        state.privacy_alert = read_bool(HPD_KEY, "CameraDetect", BoolEncoding::OneTwo)?;
        state.capability = read_u32(HPD_KEY, "Capability")?;
        state.sensor_type = read_u32(HPD_KEY, "SensorType")?;
        state.facial_feature_registered =
            read_bool(HPD_KEY, "FacialFeatureRegistered", BoolEncoding::Legacy01)?;
        Ok(state)
    }
}

#[derive(Clone, Copy)]
enum BoolEncoding {
    /// Legacy commands persist 0/1.
    Legacy01,
    /// Camera/video commands persist 1=enable/2=disable.
    OneTwo,
}

fn read_u32(key: &str, value: &str) -> Result<Option<u32>> {
    use std::os::windows::ffi::OsStrExt;
    let key_wide: Vec<u16> = std::ffi::OsStr::new(key)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value_wide: Vec<u16> = std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut handle = std::ptr::null_mut();
    let open = unsafe {
        windows_sys::Win32::System::Registry::RegOpenKeyExW(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            key_wide.as_ptr(),
            0,
            windows_sys::Win32::System::Registry::KEY_QUERY_VALUE,
            &mut handle,
        )
    };
    if open != 0 {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("SmartSense HPD store {key} is absent (error {open})"),
        });
    }
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let mut kind = 0u32;
    let status = unsafe {
        windows_sys::Win32::System::Registry::RegGetValueW(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            key_wide.as_ptr(),
            value_wide.as_ptr(),
            windows_sys::Win32::System::Registry::RRF_RT_REG_DWORD,
            &mut kind,
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    unsafe {
        windows_sys::Win32::System::Registry::RegCloseKey(handle);
    }
    match status {
        0 => Ok(Some(data)),
        // ERROR_FILE_NOT_FOUND: the value simply has never been persisted.
        2 => Ok(None),
        other => Err(LctrlError::ChannelUnavailable {
            channel: format!("SmartSense HPD value {value} read failed (error {other})"),
        }),
    }
}

fn read_bool(key: &str, value: &str, encoding: BoolEncoding) -> Result<Option<bool>> {
    Ok(read_u32(key, value)?.map(|raw| match encoding {
        BoolEncoding::Legacy01 => raw != 0,
        BoolEncoding::OneTwo => raw == 1,
    }))
}

// Keep the connect timeout visible next to the pipe path for reviewers
// checking the framing contract against docs/RE-interfaces.md §13.
#[allow(dead_code)]
const fn connect_timeout() -> Duration {
    CONNECT_TIMEOUT
}
