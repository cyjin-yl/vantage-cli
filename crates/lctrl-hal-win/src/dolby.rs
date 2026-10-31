//! Dolby DAX RPC control through the vendored `DolbyRpcClient.dll` C exports.
//!
//! Contract recovered by static analysis of TB14P-EZRA vendor binaries; see
//! `docs/RE-interfaces.md` §12. The client DLL forwards to DAX3API.exe over
//! ALPC (`ncalrpc:[DaxRpcEndpoint]`, interface `11c6212d-d5f3-4500-ab16-
//! 634dc63037be` v1.0). Only the toggle/profile exports are used; the
//! licensed APO DSP inside DAX3API is never touched.
//!
//! Export signatures were verified byte-by-byte against the thunk bodies
//! (all exports take no caller-visible parameters; out-parameters are stack
//! locals inside the wrappers):
//!
//! | export                    | signature                | RPC opnum |
//! |---------------------------|--------------------------|-----------|
//! | DolbyClient_GetDolbyEnabled    | `fn() -> bool`      | 5         |
//! | DolbyClient_SetDolbyEnabled    | `fn(u8) -> bool`    | 6         |
//! | DolbyClient_GetActiveProfile   | `fn() -> u32`       | 7         |
//! | DolbyClient_SetActiveProfile   | `fn(u32) -> bool`   | 8         |
//! | DolbyClient_GetSKU             | `fn() -> u32`       | —         |
//! | DolbyClient_IsSpatialAudioSupported | `fn() -> bool` | 2         |

use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use lctrl_core::{ApplyMode, ChangeReport, DolbyProfile, DolbyState, LctrlError, Result};
use lctrl_hal::AudioControl;
use lctrl_hal::poll_readback;

/// Sealed seam over the vendor RPC exports so [`WindowsDolbyAudio`] stays
/// testable without the vendor DLL or a running DAX3API.
pub trait DolbyRpc: Send + Sync {
    fn dolby_enabled(&self) -> Result<bool>;
    fn set_dolby_enabled(&self, enabled: bool) -> Result<()>;
    fn active_profile(&self) -> Result<u32>;
    fn set_active_profile(&self, index: u32) -> Result<()>;
    fn sku(&self) -> Result<u32>;
    fn spatial_supported(&self) -> Result<bool>;
}

/// `AudioControl` backed by a [`DolbyRpc`] transport.
pub struct WindowsDolbyAudio<R: DolbyRpc> {
    rpc: R,
}

impl<R: DolbyRpc> WindowsDolbyAudio<R> {
    #[must_use]
    pub const fn new(rpc: R) -> Self {
        Self { rpc }
    }

    #[must_use]
    pub const fn rpc(&self) -> &R {
        &self.rpc
    }

    fn read_state(&self) -> Result<DolbyState> {
        let enabled = self.rpc.dolby_enabled()?;
        let raw_profile = self.rpc.active_profile()?;
        let active_profile = DolbyProfile::from_raw(raw_profile as u8).ok_or_else(|| {
            LctrlError::ChannelUnavailable {
                channel: format!(
                    "DAX RPC returned profile index {raw_profile}; valid range is [0,5]"
                ),
            }
        })?;
        let sku = self.rpc.sku().ok();
        let spatial_supported = self.rpc.spatial_supported().ok();
        Ok(DolbyState {
            enabled,
            active_profile,
            sku,
            spatial_supported,
        })
    }
}

impl<R: DolbyRpc> AudioControl for WindowsDolbyAudio<R> {
    fn dolby_state(&self) -> Result<DolbyState> {
        self.read_state()
    }

    fn set_dolby_profile(
        &self,
        profile: DolbyProfile,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DolbyState>> {
        let previous = self.read_state()?;
        let mut requested = previous;
        requested.enabled = profile != DolbyProfile::Off;
        requested.active_profile = profile;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        if profile == DolbyProfile::Off {
            self.rpc.set_dolby_enabled(false)?;
        } else {
            self.rpc.set_dolby_enabled(true)?;
            self.rpc.set_active_profile(u32::from(profile.raw()))?;
        }
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

/// Native transport loading the vendored `DolbyRpcClient.dll`.
///
/// The packaged DLL lives under `C:\Program Files\WindowsApps\...`; its DACL
/// blocks `LoadLibrary` in place, so the bytes are copied to a writable
/// per-user cache directory first (vendor-probe verified requirement).
pub struct NativeDolbyRpc {
    // Keep the module handle owned so FreeLibrary runs after the last call.
    _module: DllModule,
    exports: Exports,
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

#[allow(non_snake_case)]
struct Exports {
    DolbyClient_GetDolbyEnabled: unsafe extern "C" fn() -> bool,
    DolbyClient_SetDolbyEnabled: unsafe extern "C" fn(u8) -> bool,
    DolbyClient_GetActiveProfile: unsafe extern "C" fn() -> u32,
    DolbyClient_SetActiveProfile: unsafe extern "C" fn(u32) -> bool,
    DolbyClient_GetSKU: unsafe extern "C" fn() -> u32,
    DolbyClient_IsSpatialAudioSupported: unsafe extern "C" fn() -> bool,
    DolbyClient_Initialize: unsafe extern "C" fn() -> bool,
}

impl Exports {
    unsafe fn load(module: *mut c_void) -> Result<Self> {
        unsafe {
            Ok(Self {
                DolbyClient_GetDolbyEnabled: export(module, b"DolbyClient_GetDolbyEnabled\0")?,
                DolbyClient_SetDolbyEnabled: export(module, b"DolbyClient_SetDolbyEnabled\0")?,
                DolbyClient_GetActiveProfile: export(module, b"DolbyClient_GetActiveProfile\0")?,
                DolbyClient_SetActiveProfile: export(module, b"DolbyClient_SetActiveProfile\0")?,
                DolbyClient_GetSKU: export(module, b"DolbyClient_GetSKU\0")?,
                DolbyClient_IsSpatialAudioSupported: export(
                    module,
                    b"DolbyClient_IsSpatialAudioSupported\0",
                )?,
                DolbyClient_Initialize: export(module, b"DolbyClient_Initialize\0")?,
            })
        }
    }
}

unsafe fn export<T>(module: *mut c_void, name: &[u8]) -> Result<T> {
    let address = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.as_ptr().cast())
    };
    if address.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "DolbyRpcClient.dll is missing export {}",
                String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name))
            ),
        });
    }
    Ok(unsafe { std::mem::transmute_copy(&address) })
}

impl NativeDolbyRpc {
    /// Discover, cache, and bind the vendor Dolby RPC client DLL.
    ///
    /// Returns `ChannelUnavailable` when the Lenovo Vantage package (and thus
    /// `DolbyRpcClient.dll`) is not installed or the DAX endpoint exports
    /// cannot be resolved.
    pub fn load() -> Result<Self> {
        let cached = cached_client_path()?;
        let module = load_library(&cached)?;
        let exports = unsafe { Exports::load(module) }?;
        // Initialize the client context. The vendor export's boolean result
        // is not a readiness signal (live probe returned false while every
        // subsequent RPC succeeded), so it is intentionally not checked.
        unsafe {
            (exports.DolbyClient_Initialize)();
        }
        Ok(Self {
            _module: DllModule { handle: module },
            exports,
        })
    }
}

fn load_library(path: &Path) -> Result<*mut c_void> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr()) };
    if handle.is_null() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "DolbyRpcClient.dll failed to load from {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(handle)
}

fn cached_client_path() -> Result<PathBuf> {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = CACHE.get() {
        return Ok(path.clone());
    }
    let packaged = discover_packaged_client().map_err(|_| LctrlError::ChannelUnavailable {
        channel: "DolbyRpcClient.dll (Lenovo Vantage package DolbyDAXAPI RPC client)".to_string(),
    })?;
    let cache_dir = cache_dir()?;
    fs::create_dir_all(&cache_dir).map_err(LctrlError::Io)?;
    let cached = cache_dir.join("DolbyRpcClient.dll");
    fs::copy(&packaged, &cached).map_err(LctrlError::Io)?;
    let _ = CACHE.set(cached.clone());
    Ok(cached)
}

fn cache_dir() -> Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "%LOCALAPPDATA% is required to stage DolbyRpcClient.dll".to_string(),
        })?;
    Ok(base.join("vantage").join("dolby-rpc-client"))
}

/// Locate `DolbyRpcClient.dll` inside any installed
/// `E046963F.LenovoCompanion_*` WindowsApps package.
fn discover_packaged_client() -> Result<PathBuf> {
    let windows_apps = PathBuf::from(r"C:\Program Files\WindowsApps");
    let entries = fs::read_dir(&windows_apps).map_err(|error| LctrlError::ChannelUnavailable {
        channel: format!(
            "cannot enumerate {} (administrator read of WindowsApps required): {error}",
            windows_apps.display()
        ),
    })?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("E046963F.LenovoCompanion_"))
        })
        .map(|path| path.join("LenovoVantage").join("DolbyRpcClient.dll"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "DolbyRpcClient.dll not found in any E046963F.LenovoCompanion_* package"
                .to_string(),
        })
}

impl DolbyRpc for NativeDolbyRpc {
    fn dolby_enabled(&self) -> Result<bool> {
        Ok(unsafe { (self.exports.DolbyClient_GetDolbyEnabled)() })
    }

    fn set_dolby_enabled(&self, enabled: bool) -> Result<()> {
        let ok = unsafe { (self.exports.DolbyClient_SetDolbyEnabled)(u8::from(enabled)) };
        if !ok {
            return Err(LctrlError::ChannelUnavailable {
                channel: "DAX RPC SetDolbyEnabled (opnum 6) was rejected by DAX3API".to_string(),
            });
        }
        Ok(())
    }

    fn active_profile(&self) -> Result<u32> {
        Ok(unsafe { (self.exports.DolbyClient_GetActiveProfile)() })
    }

    fn set_active_profile(&self, index: u32) -> Result<()> {
        let ok = unsafe { (self.exports.DolbyClient_SetActiveProfile)(index) };
        if !ok {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "DAX RPC SetActiveProfile (opnum 8) rejected index {index}; valid range is [0,5]"
                ),
            });
        }
        Ok(())
    }

    fn sku(&self) -> Result<u32> {
        Ok(unsafe { (self.exports.DolbyClient_GetSKU)() })
    }

    fn spatial_supported(&self) -> Result<bool> {
        Ok(unsafe { (self.exports.DolbyClient_IsSpatialAudioSupported)() })
    }
}
