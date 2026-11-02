//! SmartSense human-presence settings control.
//!
//! Contract recovered from ThinkSmartSenseAddin.dll 1.0.0.77 (IL) and
//! LnvSST.dll/SmartSense.exe 1.0.8.5 on TB14P-EZRA; see
//! `docs/interfaces.md` §13. HPD reads and writes use the owned system-RPC
//! client in `sense_rpc`; privacy alert and attention tracking retain their
//! separate legacy settings pipe. Independent write readback comes from
//! `HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`, which
//! is also the readback channel the addin itself uses for reads.

use std::ffi::c_void;
use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, Result, SenseDistance, SenseState, SenseWait,
};
use lctrl_hal::{SenseControl, SenseFeature, encode_request, poll_readback_until};

use crate::sense_rpc::HpdRpc;

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

pub struct WindowsSenseControl<R: HpdRpc, P: SensePipe, S: HpdSettingsStore> {
    rpc: R,
    pipe: P,
    store: S,
}

impl<R: HpdRpc, P: SensePipe, S: HpdSettingsStore> WindowsSenseControl<R, P, S> {
    #[must_use]
    pub const fn new(rpc: R, pipe: P, store: S) -> Self {
        Self { rpc, pipe, store }
    }

    fn write_feature(&self, feature: SenseFeature) -> Result<()> {
        if feature.rpc_command().is_some() {
            return self.rpc.set_feature(feature);
        }
        let (code, arg) = feature
            .pipe_command()
            .ok_or_else(|| LctrlError::Unsupported {
                feature: format!("no recovered SmartSense setter for {feature:?}"),
            })?;
        match self.pipe.call(code, arg)? {
            0 => Err(LctrlError::FirmwareRejected {
                detail: format!("SmartSense pipe rejected command 0x{code:08x} arg {arg}"),
            }),
            -1 => Err(LctrlError::ChannelUnavailable {
                channel: format!("SmartSense pipe raised an exception for command 0x{code:08x}"),
            }),
            // The vendor client treats every nonzero i32 as success; the
            // service uses other positive values for queued/accepted.
            _ => Ok(()),
        }
    }
}

impl<R: HpdRpc, P: SensePipe, S: HpdSettingsStore> SenseControl for WindowsSenseControl<R, P, S> {
    fn sense_state(&self) -> Result<SenseState> {
        // The registry projection supplements the live RPC read; a missing
        // key must not abort the read before the RPC channel is reached.
        let legacy = self.store.read_state().ok();
        match self.rpc.read_state() {
            Ok(mut state) => {
                if let Some(legacy) = &legacy {
                    state.privacy_alert = legacy.privacy_alert;
                    state.attention_tracking = legacy.attention_tracking;
                }
                Ok(state)
            }
            // The live HPD channel is unavailable. The persisted registry
            // projection still describes the configured state, and it is
            // labelled as such so a degraded read never looks like a live
            // one.
            Err(error) => match legacy {
                Some(legacy) => Ok(legacy),
                // Neither the live channel nor the persisted projection is
                // available: report the channel failure instead of an empty
                // state that would read as "nothing is configured".
                None => Err(error),
            },
        }
    }

    fn set_sense_feature(
        &self,
        feature: SenseFeature,
        apply: ApplyMode,
    ) -> Result<ChangeReport<SenseState>> {
        let previous = self.sense_state()?;
        if feature.rpc_command().is_some() && !feature_available(feature, &previous) {
            return Err(LctrlError::Unsupported {
                feature: format!("HPD RPC does not advertise {feature:?}"),
            });
        }
        let requested = apply_feature_to_state(feature, previous);
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.write_feature(feature)?;
        let actual = if feature_readback_verifiable(feature) {
            // RPC capabilities and registry support metadata are distinct.
            // Verify only the requested feature against the independent
            // persistence channel, not unrelated fields from two snapshots.
            poll_readback_until(
                &requested,
                10,
                Duration::from_millis(50),
                || self.store.read_state(),
                |actual| feature_matches(feature, actual),
            )?
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

const fn feature_available(feature: SenseFeature, state: &SenseState) -> bool {
    // On the RPC channel, advertisement is the op-5 capability bitset. A
    // feature whose bit is set stays available even when its current raw
    // value cannot be modelled; refusing it as "not advertised" would
    // contradict the bitset the transport just read.
    if let Some(bits) = state.advertised {
        if let Some(bit) = feature_capability_bit(feature) {
            return bits & bit != 0;
        }
    }
    match feature {
        SenseFeature::LockOnLeave(_) => state.lock_on_leave.is_some(),
        SenseFeature::LeaveDistance(_) => state.leave_distance.is_some(),
        SenseFeature::LeaveWait(_) => state.leave_wait.is_some(),
        SenseFeature::WakeOnApproach(_) => state.wake_on_approach.is_some(),
        SenseFeature::ApproachDistance(_) => state.approach_distance.is_some(),
        SenseFeature::PauseVideo(_) => state.pause_video.is_some(),
        SenseFeature::PrivacyAlert(_) | SenseFeature::AttentionTracking(_) => true,
    }
}

/// The op-5 capability bit that advertises each RPC-backed setting.
const fn feature_capability_bit(feature: SenseFeature) -> Option<u64> {
    match feature {
        SenseFeature::LockOnLeave(_) => Some(0x0010),
        SenseFeature::LeaveDistance(_) => Some(0x0080),
        SenseFeature::LeaveWait(_) => Some(0x0020),
        SenseFeature::WakeOnApproach(_) => Some(0x0002),
        SenseFeature::ApproachDistance(_) => Some(0x0004),
        SenseFeature::PauseVideo(_) => Some(0x0200),
        SenseFeature::PrivacyAlert(_) | SenseFeature::AttentionTracking(_) => None,
    }
}

fn feature_matches(feature: SenseFeature, state: &SenseState) -> bool {
    match feature {
        SenseFeature::LockOnLeave(value) => state.lock_on_leave == Some(value),
        SenseFeature::LeaveDistance(value) => state.leave_distance == Some(value),
        SenseFeature::LeaveWait(value) => state.leave_wait == Some(value),
        SenseFeature::WakeOnApproach(value) => state.wake_on_approach == Some(value),
        SenseFeature::ApproachDistance(value) => state.approach_distance == Some(value),
        SenseFeature::PauseVideo(value) => state.pause_video == Some(value),
        SenseFeature::PrivacyAlert(value) => state.privacy_alert == Some(value),
        SenseFeature::AttentionTracking(_) => true,
    }
}

/// Native named-pipe transport. One connection per call, matching the
/// addin's `NamedPipeClientStream` usage.
pub struct NativeSensePipe;

impl SensePipe for NativeSensePipe {
    fn call(&self, code: u32, arg: u32) -> Result<i32> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{
                CreateFileW, OPEN_EXISTING, SECURITY_ANONYMOUS, SECURITY_SQOS_PRESENT,
            },
            System::Pipes::WaitNamedPipeW,
        };
        let wide: Vec<u16> = std::ffi::OsStr::new(PIPE_PATH)
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Vendor parity: the addin's NamedPipeClientStream calls
        // Connect(3000) before writing, which is WaitNamedPipeW + open.
        let waited = unsafe {
            WaitNamedPipeW(
                wide.as_ptr(),
                u32::try_from(CONNECT_TIMEOUT.as_millis()).unwrap_or(0),
            )
        };
        if waited == 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "SmartSense settings pipe did not become available within {}ms (is the SmartSense service running?): {}",
                    u32::try_from(CONNECT_TIMEOUT.as_millis()).unwrap_or(0),
                    std::io::Error::last_os_error()
                ),
            });
        }
        // Vendor parity: TokenImpersonationLevel.Anonymous maps to
        // SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS (0x0010_0000) in
        // dwFlagsAndAttributes; the service expects an explicit SQOS level.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_ANONYMOUS,
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
    let request = encode_request(code, arg);
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
        Ok(SenseState {
            source: Some("hpd-registry-store"),
            advertised: None,
            lock_on_leave: read_bool(HPD_KEY, "PresentLeave", BoolEncoding::Legacy01)?,
            leave_distance: read_u32(HPD_KEY, "LeaveDistance")?.and_then(SenseDistance::from_raw),
            leave_wait: read_u32(HPD_KEY, "LeaveWait")?.and_then(SenseWait::from_raw),
            wake_on_approach: read_bool(HPD_KEY, "Approach", BoolEncoding::Legacy01)?,
            approach_distance: read_u32(HPD_KEY, "ApproachDistance")?
                .and_then(SenseDistance::from_raw),
            pause_video: read_bool(HPD_KEY, "Video", BoolEncoding::OneTwo)?,
            privacy_alert: read_bool(HPD_KEY, "CameraDetect", BoolEncoding::OneTwo)?,
            // No registry value backs this feature on this model.
            attention_tracking: None,
            capability: read_u32(HPD_KEY, "Capability")?,
            sensor_type: read_u32(HPD_KEY, "SensorType")?,
            facial_feature_registered: read_bool(
                HPD_KEY,
                "FacialFeatureRegistered",
                BoolEncoding::Legacy01,
            )?,
        })
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
