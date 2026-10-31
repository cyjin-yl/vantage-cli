//! Noise-cancellation control through the vendor Elevoc control SDK.
//!
//! Contract recovered from ElevocControlSDK_x64.dll 1.0.1.1108.8f92a86 and
//! ElevocControlService.exe 1.0.1.1142 on TB14P-EZRA; see
//! `docs/RE-interfaces.md` §14. The SDK forwards to the service over ALPC
//! (`ncalrpc:[ElevocRpcEndpoint]`, interface
//! `{6F761170-1054-4339-A573-063DD08C7E16}` v4.0) using a JSON request
//! envelope on a single NDR opnum; reimplementing that stack is
//! deliberately avoided in favour of the vendor exports. The DSP pipeline
//! stays inside ElevocControlService — this surface only selects modes.
//!
//! Mode domain (`SetEffectMode`, proven from SDK string slots
//! 765):
//!
//! | mode | name          | notes                                        |
//! |------|---------------|----------------------------------------------|
//! | 0    | environmental | service default; closest to "off"            |
//! | 1    | shared        |                                              |
//! | 2    | private       |                                              |
//! | 3    | special       |                                              |
//! | 4    | voiceid       | rejected unless VoiceID is enrolled          |
//! | 5    | HQRecord      |                                              |
//! | 6/7  | shared_td / private_td | legacy aliases remapped to 1/2 by the SDK |
//!
//! `SetEffectMode` results (proven 545):
//! 1 = success, -1 = RPC/default-endpoint failure, 2 = mode not supported
//! by the endpoint ability bitmask, 4 = VoiceID requested but not enrolled.

use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, NoiseCancelState, NoiseCancellationMode, Result,
};
use lctrl_hal::NoiseCancelControl;
use lctrl_hal::poll_readback;

/// Sealed seam over the SDK exports so [`WindowsNoiseCancel`] stays testable
/// without the vendor DLL or a running ElevocControlService.
pub trait ElevocRpc: Send + Sync {
    /// `GetCurrentMode(NULL)` — SDK domain 0–7, negative on failure.
    fn current_mode(&self) -> Result<i32>;
    /// `SetEffectMode(mode, NULL)`.
    fn set_effect_mode(&self, mode: u32) -> Result<()>;
    /// `QueryVoiceIDStatus()` — 0 = unset, 2 = enrolled.
    fn voice_id_status(&self) -> Result<i32>;
}

/// `AudioControl`-adjacent noise-cancellation control backed by an
/// [`ElevocRpc`] transport plus the F4006 registry mirror for the
/// speaker-side observation.
pub struct WindowsNoiseCancel<R: ElevocRpc> {
    rpc: R,
}

impl<R: ElevocRpc> WindowsNoiseCancel<R> {
    #[must_use]
    pub const fn new(rpc: R) -> Self {
        Self { rpc }
    }

    fn read_state(&self) -> Result<NoiseCancelState> {
        let mic_mode = match self.rpc.current_mode()? {
            raw @ 0..=7 => NoiseCancellationMode::from_raw(raw as u8),
            raw => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("Elevoc GetCurrentMode returned unexpected value {raw}"),
                });
            }
        };
        let voice_id_enrolled = match self.rpc.voice_id_status() {
            Ok(0) => Some(false),
            Ok(2) => Some(true),
            _ => None,
        };
        Ok(NoiseCancelState {
            mic_mode,
            speaker_enabled: read_f4006_speaker_mirror(),
            voice_id_enrolled,
        })
    }
}

impl<R: ElevocRpc> NoiseCancelControl for WindowsNoiseCancel<R> {
    fn noise_cancel_state(&self) -> Result<NoiseCancelState> {
        self.read_state()
    }

    fn set_noise_cancel_mode(
        &self,
        mode: NoiseCancellationMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<NoiseCancelState>> {
        let previous = self.read_state()?;
        let mut requested = previous;
        requested.mic_mode = mode;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.rpc.set_effect_mode(u32::from(mode.raw()))?;
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

/// Native transport binding the vendor `ElevocControlSDK_x64.dll` exports.
pub struct NativeElevocRpc {
    _module: DllModule,
    get_current_mode: unsafe extern "C" fn(*const c_char) -> i32,
    set_effect_mode: unsafe extern "C" fn(i32, *const c_char) -> i32,
    query_voice_id_status: unsafe extern "C" fn() -> i32,
}

struct DllModule {
    handle: *mut c_void,
}

// The module handle is process-local and never mutated.
unsafe impl Send for DllModule {}
unsafe impl Sync for DllModule {}

impl Drop for DllModule {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::FreeLibrary(self.handle);
        }
    }
}

impl NativeElevocRpc {
    /// Discover, cache, and bind the vendor SDK DLL. Returns
    /// `ChannelUnavailable` when the SmartNoiseCancelled addin (and thus
    /// the SDK) is not installed.
    pub fn load() -> Result<Self> {
        let cached = cached_sdk_path()?;
        let handle = load_library(&cached)?;
        unsafe {
            let get_current_mode: unsafe extern "C" fn(*const c_char) -> i32 =
                export(handle, b"GetCurrentMode\0")?;
            let set_effect_mode: unsafe extern "C" fn(i32, *const c_char) -> i32 =
                export(handle, b"SetEffectMode\0")?;
            let query_voice_id_status: unsafe extern "C" fn() -> i32 =
                export(handle, b"QueryVoiceIDStatus\0")?;
            Ok(Self {
                _module: DllModule { handle },
                get_current_mode,
                set_effect_mode,
                query_voice_id_status,
            })
        }
    }
}

impl ElevocRpc for NativeElevocRpc {
    fn current_mode(&self) -> Result<i32> {
        Ok(unsafe { (self.get_current_mode)(std::ptr::null()) })
    }

    fn set_effect_mode(&self, mode: u32) -> Result<()> {
        let result = unsafe { (self.set_effect_mode)(mode as i32, std::ptr::null()) };
        match result {
            1 => Ok(()),
            -1 => Err(LctrlError::ChannelUnavailable {
                channel: "Elevoc SetEffectMode RPC or default-endpoint failure".to_string(),
            }),
            2 => Err(LctrlError::Unsupported {
                feature: format!("noise-cancel mode {mode} is not supported by this endpoint"),
            }),
            4 => Err(LctrlError::InvalidArgument {
                detail: "voiceid mode requires an enrolled VoiceID profile".to_string(),
            }),
            other => Err(LctrlError::ChannelUnavailable {
                channel: format!("Elevoc SetEffectMode returned unexpected result {other}"),
            }),
        }
    }

    fn voice_id_status(&self) -> Result<i32> {
        Ok(unsafe { (self.query_voice_id_status)() })
    }
}

unsafe fn export<T>(module: *mut c_void, name: &[u8]) -> Result<T> {
    let address = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.as_ptr().cast())
    };
    if address.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "ElevocControlSDK_x64.dll is missing export {}",
                String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name))
            ),
        });
    }
    Ok(unsafe { std::mem::transmute_copy(&address) })
}

fn load_library(path: &std::path::Path) -> Result<*mut c_void> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr()) };
    if handle.is_null() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "ElevocControlSDK_x64.dll failed to load from {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(handle)
}

fn cached_sdk_path() -> Result<PathBuf> {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = CACHE.get() {
        return Ok(path.clone());
    }
    let packaged = discover_packaged_sdk().map_err(|_| LctrlError::ChannelUnavailable {
        channel: "ElevocControlSDK_x64.dll (SmartNoiseCancelled addin RPC client)".to_string(),
    })?;
    let cache_dir = cache_dir()?;
    std::fs::create_dir_all(&cache_dir).map_err(LctrlError::Io)?;
    let cached = cache_dir.join("ElevocControlSDK_x64.dll");
    std::fs::copy(&packaged, &cached).map_err(LctrlError::Io)?;
    let _ = CACHE.set(cached.clone());
    Ok(cached)
}

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "%LOCALAPPDATA% is required to stage ElevocControlSDK_x64.dll".to_string(),
        })?;
    Ok(base.join("vantage").join("elevoc-rpc-client"))
}

/// Locate `ElevocControlSDK_x64.dll` inside any installed
/// `SmartNoiseCancelledAddin` version directory.
fn discover_packaged_sdk() -> Result<PathBuf> {
    let addins = PathBuf::from(r"C:\ProgramData\Lenovo\Vantage\Addins");
    let entries = std::fs::read_dir(&addins).map_err(|error| LctrlError::ChannelUnavailable {
        channel: format!(
            "cannot enumerate {} (SmartNoiseCancelled addin required): {error}",
            addins.display()
        ),
    })?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("SmartNoiseCancelledAddin"))
        })
        .map(|path| path.join("ElevocControlSDK_x64.dll"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "ElevocControlSDK_x64.dll not found in any SmartNoiseCancelledAddin version"
                .to_string(),
        })
}

/// Speaker-side (downlink) observation from the SmartEngine F4006 cache.
/// The registry is a write-only mirror maintained by the Vantage plugin;
/// no service consumes it, so this is display state, not control state.
fn read_f4006_speaker_mirror() -> Option<bool> {
    use std::os::windows::ffi::OsStrExt;
    const KEY: &str = r"SOFTWARE\Lenovo\SmartEngine\modules\F4006";
    const VALUE: &str = "noise_speaker_mode";
    let key_wide: Vec<u16> = std::ffi::OsStr::new(KEY)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value_wide: Vec<u16> = std::ffi::OsStr::new(VALUE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut buffer = [0u16; 8];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        windows_sys::Win32::System::Registry::RegGetValueW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            value_wide.as_ptr(),
            windows_sys::Win32::System::Registry::RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..len]);
    match text.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}
