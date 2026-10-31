//! Windows power overlay scheme: the OS-native performance mode.
//!
//! `PowerSetActiveOverlayScheme`, `PowerGetActualOverlayScheme` and
//! `PowerGetEffectiveOverlayScheme` are documented Windows 10 1809+ exports of
//! `powrprof.dll` (see the Windows SDK `powersetting.h` overlay GUIDs). They are
//! the direct OS equivalent of the Linux `platform_profile` /
//! `ideapad/thermal_mode` control: no vendor service, driver, DLL, or registry
//! surface is involved, and nothing here calls into Lenovo software.

use lctrl_core::{LctrlError, PerformanceMode, Result};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};

/// Layout-compatible with the SDK `GUID` used by the overlay APIs.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayGuid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl OverlayGuid {
    /// The zero GUID, which selects the balanced (no-overlay) scheme.
    pub const BALANCED: Self = Self {
        data1: 0,
        data2: 0,
        data3: 0,
        data4: [0; 8],
    };

    /// `961cc777-2547-4f9d-8174-7d86181b8a7a` (best power efficiency).
    pub const POWER_EFFICIENCY: Self = Self {
        data1: 0x961c_c777,
        data2: 0x2547,
        data3: 0x4f9d,
        data4: [0x81, 0x74, 0x7d, 0x86, 0x18, 0x1b, 0x8a, 0x7a],
    };

    /// `ded574b5-45a0-4f42-8737-46345c09c238` (best performance).
    pub const PERFORMANCE: Self = Self {
        data1: 0xded5_74b5,
        data2: 0x45a0,
        data3: 0x4f42,
        data4: [0x87, 0x37, 0x46, 0x34, 0x5c, 0x09, 0xc2, 0x38],
    };
}

impl Default for OverlayGuid {
    fn default() -> Self {
        Self::BALANCED
    }
}

impl core::fmt::Display for OverlayGuid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.data1,
            self.data2,
            self.data3,
            self.data4[0],
            self.data4[1],
            self.data4[2],
            self.data4[3],
            self.data4[4],
            self.data4[5],
            self.data4[6],
            self.data4[7],
        )
    }
}

/// Maps a portable performance mode onto an OS overlay scheme.
///
/// Only the three modes that the OS actually defines are accepted. `Geek`,
/// `SilentHighPerformance` and `Custom` are vendor-specific modes with no
/// documented Windows equivalent, so they are refused rather than silently
/// mapped onto a different behaviour.
pub fn overlay_for_mode(mode: PerformanceMode) -> Result<OverlayGuid> {
    match mode {
        PerformanceMode::Balanced => Ok(OverlayGuid::BALANCED),
        PerformanceMode::Quiet => Ok(OverlayGuid::POWER_EFFICIENCY),
        PerformanceMode::Performance => Ok(OverlayGuid::PERFORMANCE),
        PerformanceMode::Geek
        | PerformanceMode::SilentHighPerformance
        | PerformanceMode::Custom => Err(LctrlError::Unsupported {
            feature: format!("perf.mode.{mode}.overlay"),
        }),
    }
}

/// Inverse of [`overlay_for_mode`]; `None` for a scheme this build does not own.
#[must_use]
pub fn mode_for_overlay(guid: &OverlayGuid) -> Option<PerformanceMode> {
    if *guid == OverlayGuid::BALANCED {
        Some(PerformanceMode::Balanced)
    } else if *guid == OverlayGuid::POWER_EFFICIENCY {
        Some(PerformanceMode::Quiet)
    } else if *guid == OverlayGuid::PERFORMANCE {
        Some(PerformanceMode::Performance)
    } else {
        None
    }
}

/// Overlay access, split out so tests can drive it without `powrprof.dll`.
pub trait PowerOverlayControl: Send + Sync {
    fn actual(&self) -> Result<OverlayGuid>;
    fn effective(&self) -> Result<OverlayGuid>;
    fn set_active(&self, scheme: &OverlayGuid) -> Result<()>;
}

impl<T> PowerOverlayControl for &T
where
    T: PowerOverlayControl + ?Sized,
{
    fn actual(&self) -> Result<OverlayGuid> {
        (**self).actual()
    }

    fn effective(&self) -> Result<OverlayGuid> {
        (**self).effective()
    }

    fn set_active(&self, scheme: &OverlayGuid) -> Result<()> {
        (**self).set_active(scheme)
    }
}

/// The real `powrprof.dll` overlay surface.
///
/// The three exports are resolved at run time through `GetProcAddress` instead
/// of an import library: `powrprof.lib` shipped with some SDKs does not carry
/// them, and a missing export must surface as an honest `unavailable` rather
/// than a link failure.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePowerOverlay;

type GetSchemeFn = unsafe extern "system" fn(*mut OverlayGuid) -> u32;
type SetSchemeFn = unsafe extern "system" fn(*const OverlayGuid) -> u32;

struct OverlayApi {
    actual: GetSchemeFn,
    effective: GetSchemeFn,
    set_active: SetSchemeFn,
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn resolve(module: HMODULE, name: &[u8]) -> Option<*const ()> {
    let address = unsafe { GetProcAddress(module, name.as_ptr()) }?;
    Some(address as *const ())
}

fn load_api() -> Option<OverlayApi> {
    let mut module = unsafe { GetModuleHandleW(wide("powrprof.dll").as_ptr()) };
    if module.is_null() {
        module = unsafe { LoadLibraryW(wide("powrprof.dll").as_ptr()) };
    }
    if module.is_null() {
        return None;
    }
    let actual = resolve(module, b"PowerGetActualOverlayScheme\0")?;
    let effective = resolve(module, b"PowerGetEffectiveOverlayScheme\0")?;
    let set_active = resolve(module, b"PowerSetActiveOverlayScheme\0")?;
    Some(OverlayApi {
        actual: unsafe { core::mem::transmute::<*const (), GetSchemeFn>(actual) },
        effective: unsafe { core::mem::transmute::<*const (), GetSchemeFn>(effective) },
        set_active: unsafe { core::mem::transmute::<*const (), SetSchemeFn>(set_active) },
    })
}

fn api() -> Result<&'static OverlayApi> {
    static API: std::sync::OnceLock<Option<OverlayApi>> = std::sync::OnceLock::new();
    API.get_or_init(load_api)
        .as_ref()
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "powrprof.dll does not export the power overlay API (Windows 10 1809+ required)"
                .into(),
        })
}

fn check_status(status: u32, operation: &str) -> Result<()> {
    if status == 0 {
        return Ok(());
    }
    Err(LctrlError::ChannelUnavailable {
        channel: format!("power overlay {operation} failed with Win32 status {status}"),
    })
}

impl PowerOverlayControl for NativePowerOverlay {
    fn actual(&self) -> Result<OverlayGuid> {
        let api = api()?;
        let mut scheme = OverlayGuid::BALANCED;
        let status = unsafe { (api.actual)(&raw mut scheme) };
        check_status(status, "PowerGetActualOverlayScheme")?;
        Ok(scheme)
    }

    fn effective(&self) -> Result<OverlayGuid> {
        let api = api()?;
        let mut scheme = OverlayGuid::BALANCED;
        let status = unsafe { (api.effective)(&raw mut scheme) };
        check_status(status, "PowerGetEffectiveOverlayScheme")?;
        Ok(scheme)
    }

    fn set_active(&self, scheme: &OverlayGuid) -> Result<()> {
        let api = api()?;
        let status = unsafe { (api.set_active)(scheme as *const OverlayGuid) };
        check_status(status, "PowerSetActiveOverlayScheme")
    }
}
