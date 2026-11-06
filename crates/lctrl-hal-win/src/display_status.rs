//! Read-only display state for `vantage display status`.
//!
//! Two scopes are read and never merged (see [`lctrl_core::DisplayScope`]):
//!
//! - **session** — the calling session's display paths, their current mode, the
//!   per-monitor scale Windows applies, and the advanced-colour state of each
//!   target. All of it rides the public CCD/DPI APIs, which describe the calling
//!   session's desktop: a service or SSH logon gets `ERROR_ACCESS_DENIED` from
//!   `QueryDisplayConfig` and sees a synthetic `WinDisc` surface, so that refusal
//!   is reported instead of an empty list or a fake panel;
//! - **machine** — per-monitor state the display driver keeps for the physical
//!   panel (`GraphicsDrivers\MonitorDataStore`, keyed by monitor hardware id)
//!   plus the HDR capability the monitor declares in its own EDID. Both are
//!   readable without a desktop session, which is why they are the only HDR
//!   observation available over SSH.
//!
//! Nothing in this module writes: no `SET_ADVANCED_COLOR_STATE`, no
//! `SET_HDR_STATE`, no `ChangeDisplaySettingsEx`, no DPI write, no registry
//! write.

use lctrl_core::{
    DisplayColorState, DisplayConnection, DisplayPathState, DisplayScope, DisplayScalingObservation,
    DisplaySession, HdrCapabilityObservation, LctrlError, MonitorColorState, MonitorColorStore,
    PanelTarget, Result, SessionDesktop, decode_panel_hdr_capability,
};
use lctrl_hal::DisplayStatusControl;

use crate::display::DisplayApi;
use crate::wmi_contract::{ROOT_WMI, WmiObject, WmiTransport, WmiValue};
use crate::PanelRegistry;

/// Every connected PnP monitor with its OS output-technology classification.
/// This class is readable without an interactive desktop session, unlike the
/// monitor identity classes (`WmiMonitorID`, `WmiMonitorBasicDisplayParams`),
/// which return zero instances from a session-0 logon.
const MONITOR_CONNECTIONS_QUERY: &str =
    "SELECT InstanceName, VideoOutputTechnology, Active FROM WmiMonitorConnectionParams";

/// The installed display driver's per-monitor state store. Value names are
/// literals in `dxgkrnl.sys` (`MonitorGetIsAutoColorManagementSupported`,
/// `MonitorSetSDRWhiteLevel`, …), which is what makes this a driver contract
/// rather than a vendor convention.
const MONITOR_DATA_STORE: &str =
    r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers\MonitorDataStore";

/// `D3DKMDT_VIDEO_OUTPUT_TECHNOLOGY` for an embedded panel, as an unsigned word.
const VOT_INTERNAL_UNSIGNED: i64 = 0x8000_0000;
/// The same constant as a signed 32-bit WMI value.
const VOT_INTERNAL_SIGNED: i64 = -2_147_483_648;

/// Session-scoped display reads.
///
/// Implementations answer for the calling session only; they must not fall back
/// to another session's topology or to a different display.
pub trait DisplayStateApi: Send + Sync {
    /// Session scope of these reads.
    fn session(&self) -> DisplaySession;
    /// The calling session's monitor list. Used to attribute the fallback when
    /// the live path enumeration is unavailable, and to resolve a path's
    /// desktop position to its monitor's effective DPI.
    fn desktops(&self) -> Result<Vec<SessionDesktop>>;
    /// Advanced colour (HDR / wide colour) of one active target.
    fn advanced_color(&self, target: &PanelTarget) -> Result<DisplayColorState>;
    /// Effective DPI of the monitor covering a desktop position.
    fn effective_dpi_at(&self, x: i32, y: i32) -> Result<DisplayScalingObservation>;
}

/// Machine-wide per-monitor colour state, keyed by monitor hardware id.
pub trait MonitorColorStoreReader: Send + Sync {
    /// `Ok(None)` when the driver store holds no entry for this monitor; an
    /// ambiguous join (two entries sharing the hardware-id prefix) is an error,
    /// never a guess.
    fn color_store(&self, hardware_id: &str) -> Result<Option<MonitorColorStore>>;
}

/// Read-only display state over the native display APIs, the WMI monitor
/// classes, and the driver's colour store.
pub struct WindowsDisplayStatus<D, A, W, R> {
    display: D,
    state: A,
    wmi: W,
    registry: R,
}

impl<D, A, W, R> WindowsDisplayStatus<D, A, W, R> {
    #[must_use]
    pub const fn new(display: D, state: A, wmi: W, registry: R) -> Self {
        Self {
            display,
            state,
            wmi,
            registry,
        }
    }

    #[must_use]
    pub const fn display(&self) -> &D {
        &self.display
    }

    #[must_use]
    pub const fn state(&self) -> &A {
        &self.state
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn registry(&self) -> &R {
        &self.registry
    }
}

impl<D, A, W, R> DisplayStatusControl for WindowsDisplayStatus<D, A, W, R>
where
    D: DisplayApi,
    A: DisplayStateApi,
    W: WmiTransport,
    R: PanelRegistry + MonitorColorStoreReader,
{
    fn session(&self) -> DisplaySession {
        self.state.session()
    }

    fn displays(&self) -> Result<Vec<DisplayPathState>> {
        let targets = self.display.connected_targets()?;
        let mut displays = Vec::with_capacity(targets.len());
        for target in targets {
            // The current desktop mode is the only authority for the refresh
            // rate the user is running; a firmware range or an adapter-mode
            // summary is not a substitute.
            let mode = self.display.current_mode(&target)?;
            let connection = match self.display.target_connection(&target)? {
                Some(connection) => connection,
                // A transport that cannot report the raw output technology
                // leaves only the internal flag; that is not enough to call a
                // virtual output external, so the fallback is documented and
                // only reachable for such transports.
                None if target.internal => DisplayConnection::Internal,
                None => DisplayConnection::External,
            };
            let scaling = self.state.effective_dpi_at(mode.position_x, mode.position_y)?;
            displays.push(DisplayPathState {
                scope: DisplayScope::Session,
                device_name: target.device_name.clone(),
                connection,
                mode,
                scaling,
                color: self.state.advanced_color(&target).ok(),
            });
        }
        Ok(displays)
    }

    fn desktops(&self) -> Result<Vec<SessionDesktop>> {
        self.state.desktops()
    }

    fn monitors(&self) -> Result<Vec<MonitorColorState>> {
        let objects = self.wmi.query(ROOT_WMI, MONITOR_CONNECTIONS_QUERY)?;
        let mut monitors: Vec<MonitorColorState> = Vec::new();
        for object in &objects {
            let Some(WmiValue::String(instance)) = object.get("InstanceName") else {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WmiMonitorConnectionParams returned no InstanceName".into(),
                });
            };
            if !is_active(object) {
                continue;
            }
            let device = device_instance(instance)?;
            let hardware_id = hardware_id(&device)?;
            if monitors
                .iter()
                .any(|monitor| monitor.hardware_id == hardware_id)
            {
                // One row per class instance; a duplicate row would otherwise be
                // reported twice for the same hardware id.
                continue;
            }
            let connection = output_technology(object)?;
            let store = self
                .registry
                .color_store(&hardware_id)?
                .unwrap_or_else(MonitorColorStore::empty);
            let hdr_capability = match self.registry.edid(&device) {
                Ok(bytes) => match decode_panel_hdr_capability(&bytes) {
                    Ok(Some(capability)) => HdrCapabilityObservation::Declared(capability),
                    Ok(None) => HdrCapabilityObservation::NotDeclared,
                    Err(error) => HdrCapabilityObservation::Unavailable(error.to_string()),
                },
                Err(error) => HdrCapabilityObservation::Unavailable(error.to_string()),
            };
            monitors.push(MonitorColorState {
                scope: DisplayScope::Machine,
                hardware_id,
                connection,
                hdr_enabled: store.hdr_enabled,
                sdr_white_level_raw: store.sdr_white_level_raw,
                sdr_white_millinits: store.sdr_white_millinits(),
                auto_color_management_supported: store.auto_color_management_supported,
                auto_color_management_enabled: store.auto_color_management_enabled,
                display_hdr_level_notified: store.display_hdr_level_notified,
                hdr_capability,
            });
        }
        if monitors.is_empty() {
            return Err(LctrlError::Unsupported {
                feature: "display.monitors".into(),
            });
        }
        monitors.sort_by(|left, right| left.hardware_id.cmp(&right.hardware_id));
        Ok(monitors)
    }
}

/// `Active` is a boolean property; an absent value is not "inactive".
fn is_active(object: &WmiObject) -> bool {
    match object.get("Active") {
        Some(WmiValue::Bool(active)) => *active,
        Some(WmiValue::U8(value)) => *value != 0,
        Some(WmiValue::U16(value)) => *value != 0,
        Some(WmiValue::U32(value)) => *value != 0,
        _ => true,
    }
}

fn output_technology(object: &WmiObject) -> Result<DisplayConnection> {
    let raw = match object.get("VideoOutputTechnology") {
        Some(value) => numeric(value).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("VideoOutputTechnology is not numeric: {value:?}"),
        })?,
        None => {
            return Err(LctrlError::ChannelUnavailable {
                channel: "WmiMonitorConnectionParams omitted VideoOutputTechnology".into(),
            })
        }
    };
    if raw == VOT_INTERNAL_UNSIGNED || raw == VOT_INTERNAL_SIGNED {
        return Ok(DisplayConnection::Internal);
    }
    let raw = u32::try_from(raw).map_err(|_| LctrlError::ChannelUnavailable {
        channel: format!(
            "VideoOutputTechnology {raw} is outside the 32-bit output-technology range"
        ),
    })?;
    Ok(DisplayConnection::from_output_technology(raw))
}

/// Strip the WMI display index (`..._0`) from a monitor instance name so the
/// remainder is the PnP device instance path used by the EDID registry key.
fn device_instance(instance: &str) -> Result<String> {
    let device = match instance.rsplit_once('_') {
        Some((device, index))
            if !device.is_empty() && !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()) =>
        {
            device
        }
        _ => instance,
    };
    let mut parts = device.split('\\');
    let class = parts.next().unwrap_or_default();
    let hardware = parts.next().unwrap_or_default();
    let instance_id = parts.next().unwrap_or_default();
    if class.eq_ignore_ascii_case("DISPLAY") && !hardware.is_empty() && !instance_id.is_empty() {
        Ok(device.to_owned())
    } else {
        Err(LctrlError::ChannelUnavailable {
            channel: format!("monitor instance {instance:?} is not a PnP display path"),
        })
    }
}

/// The hardware id (`<manufacturer><product code>`, e.g. `LEN8AD7`) of a PnP
/// display instance. It is the join key between the monitor, its EDID registry
/// value, and the driver's colour store; the instance path stays private.
fn hardware_id(device_instance: &str) -> Result<String> {
    device_instance
        .split('\\')
        .nth(1)
        .filter(|hardware| !hardware.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("monitor instance {device_instance:?} carries no hardware id"),
        })
}

fn numeric(value: &WmiValue) -> Option<i64> {
    match value {
        WmiValue::I8(value) => Some(i64::from(*value)),
        WmiValue::I16(value) => Some(i64::from(*value)),
        WmiValue::I32(value) => Some(i64::from(*value)),
        WmiValue::I64(value) => Some(*value),
        WmiValue::U8(value) => Some(i64::from(*value)),
        WmiValue::U16(value) => Some(i64::from(*value)),
        WmiValue::U32(value) => Some(i64::from(*value)),
        WmiValue::U64(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

#[cfg(windows)]
mod native {
    use std::ptr;
    use std::sync::LazyLock;

    use lctrl_core::{
        ActiveColorMode, ColorStateSource, DisplayColorEncoding, DisplayColorState,
        DisplayScaling, DisplayScalingObservation, DisplayScope, DisplaySession, PanelTarget,
        Result, SessionDesktop,
    };
    use windows_sys::Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_HEADER,
            DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
            DisplayConfigGetDeviceInfo,
        },
        Foundation::{
            ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, LUID, LPARAM, RECT,
        },
        Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW},
        System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId},
        System::Threading::GetCurrentProcessId,
        UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_PER_MONITOR_AWARE,
            GetAwarenessFromDpiAwarenessContext, GetDpiForMonitor, GetThreadDpiAwarenessContext,
            MDT_EFFECTIVE_DPI, PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness,
            SetProcessDpiAwarenessContext,
        },
    };

    use super::DisplayStateApi;
    use crate::map_win_error;

    /// `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2`. The public headers
    /// this crate binds (`windows-sys` 0.61) stop at type 11; the numeric value
    /// and the 36-byte layout come from mingw-w64 `wingdi.h` (September 2024,
    /// commit `e33940a2`) and the `SDK_26100.h` shim added by `xbmc/xbmc#26096`,
    /// and are documented in `docs/evidence/re-display-state-20261105.md` §3.1.
    const GET_ADVANCED_COLOR_INFO_2: i32 = 15;
    /// `WTSGetActiveConsoleSessionId` sentinel for "no console session".
    const NO_CONSOLE_SESSION: u32 = u32::MAX;

    /// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2`: the pre-24H2 request plus
    /// `activeColorMode`, with HDR and wide colour split into separate bits.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AdvancedColorInfo2 {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
        /// bit 0 advancedColorSupported, bit 1 advancedColorActive,
        /// bit 3 advancedColorLimitedByPolicy, bit 4 highDynamicRangeSupported,
        /// bit 5 highDynamicRangeUserEnabled, bit 6 wideColorSupported,
        /// bit 7 wideColorUserEnabled
        value: u32,
        color_encoding: i32,
        bits_per_color: u32,
        active_color_mode: u32,
    }

    /// Process DPI awareness, established once. `GetDpiForMonitor` answers per
    /// the *process* awareness (an unaware process always gets 96 dpi), and the
    /// process context can only be set before any DPI-dependent call, so the
    /// result is cached and callers refuse a DPI they cannot trust.
    static PER_MONITOR_AWARE: LazyLock<bool> = LazyLock::new(|| {
        let context =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if context == 0 {
            // Already claimed by a manifest or an earlier call; the older
            // process API is the only remaining chance to raise awareness.
            unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE) };
        }
        let awareness =
            unsafe { GetAwarenessFromDpiAwarenessContext(GetThreadDpiAwarenessContext()) };
        awareness == DPI_AWARENESS_PER_MONITOR_AWARE
    });

    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeDisplayState;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct MonitorSnapshot {
        device_name: [u16; 32],
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        dpi_x: u32,
        dpi_y: u32,
    }

    impl MonitorSnapshot {
        fn contains(&self, x: i32, y: i32) -> bool {
            (self.left..self.right).contains(&x) && (self.top..self.bottom).contains(&y)
        }
    }

    impl DisplayStateApi for NativeDisplayState {
        fn session(&self) -> DisplaySession {
            let id = current_session_id();
            let console = active_console_session_id();
            DisplaySession {
                id,
                console_session_id: console,
                // A missing console session means the caller is not on the
                // physical desktop.
                interactive: console == Some(id),
            }
        }

        fn desktops(&self) -> Result<Vec<SessionDesktop>> {
            let aware = *PER_MONITOR_AWARE;
            let snapshots = self.session_monitors()?;
            Ok(snapshots
                .iter()
                .map(|snapshot| SessionDesktop {
                    // The calling session owns this surface. For a service or SSH
                    // logon it is a synthetic desktop (`WinDisc`), which must not
                    // be read as a panel.
                    scope: DisplayScope::SessionVirtual,
                    device_name: utf16_string(&snapshot.device_name),
                    width: u32::try_from(snapshot.right - snapshot.left).unwrap_or(0),
                    height: u32::try_from(snapshot.bottom - snapshot.top).unwrap_or(0),
                    scaling: scaling_of(snapshot, aware),
                })
                .collect())
        }

        fn advanced_color(&self, target: &PanelTarget) -> Result<DisplayColorState> {
            match self.advanced_color_info_2(target)? {
                Some(state) => Ok(state),
                // The OS does not know the 24H2 request: fall back to the legacy
                // one, whose HDR fields stay unknown.
                None => self.advanced_color_info_1(target),
            }
        }

        fn effective_dpi_at(&self, x: i32, y: i32) -> Result<DisplayScalingObservation> {
            let aware = *PER_MONITOR_AWARE;
            let snapshots = self.session_monitors()?;
            Ok(match snapshots.iter().find(|snapshot| snapshot.contains(x, y)) {
                Some(snapshot) => scaling_of(snapshot, aware),
                None => DisplayScalingObservation::NoMonitor,
            })
        }
    }

    impl NativeDisplayState {
        /// 24H2 request: HDR and wide colour are separate bits, and the active
        /// colour mode is reported directly. `Ok(None)` when the OS refuses the
        /// request type itself (pre-24H2).
        fn advanced_color_info_2(&self, target: &PanelTarget) -> Result<Option<DisplayColorState>> {
            let mut info = AdvancedColorInfo2::default();
            info.header.r#type = GET_ADVANCED_COLOR_INFO_2;
            info.header.size = u32::try_from(std::mem::size_of::<AdvancedColorInfo2>())
                .expect("AdvancedColorInfo2 size fits u32");
            info.header.adapterId = luid(target);
            info.header.id = target.target_id;
            let status = unsafe { DisplayConfigGetDeviceInfo(&mut info.header) };
            if u32::try_from(status).is_ok_and(|code| {
                code == ERROR_NOT_SUPPORTED || code == ERROR_INVALID_PARAMETER
            }) {
                return Ok(None);
            }
            if status != 0 {
                return Err(display_config_error(
                    status,
                    "DisplayConfigGetDeviceInfo(GET_ADVANCED_COLOR_INFO_2)",
                ));
            }
            let bits = info.value;
            let sdr_white = self.sdr_white_level(target);
            Ok(Some(DisplayColorState {
                source: ColorStateSource::AdvancedColorInfo2,
                advanced_color_supported: bits & 0x01 != 0,
                advanced_color_active: bits & 0x02 != 0,
                advanced_color_limited_by_policy: Some(bits & 0x08 != 0),
                hdr_supported: Some(bits & 0x10 != 0),
                hdr_user_enabled: Some(bits & 0x20 != 0),
                wide_color_supported: Some(bits & 0x40 != 0),
                wide_color_user_enabled: Some(bits & 0x80 != 0),
                active_mode: Some(ActiveColorMode::from_raw(info.active_color_mode)),
                color_encoding: DisplayColorEncoding::from_raw(info.color_encoding as u32),
                bits_per_color: info.bits_per_color,
                sdr_white_level_raw: sdr_white,
                sdr_white_millinits: sdr_white.map(DisplayColorState::sdr_white_millinits_of),
            }))
        }

        /// Pre-24H2 request. Bit 0 means "HDR **or** wide colour", so the HDR and
        /// wide-colour fields stay `None` instead of being inferred from it.
        fn advanced_color_info_1(&self, target: &PanelTarget) -> Result<DisplayColorState> {
            let mut info = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            info.header.size =
                u32::try_from(std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>())
                    .expect("advanced colour size fits u32");
            info.header.adapterId = luid(target);
            info.header.id = target.target_id;
            let status = unsafe { DisplayConfigGetDeviceInfo(&mut info.header) };
            if status != 0 {
                return Err(display_config_error(
                    status,
                    "DisplayConfigGetDeviceInfo(GET_ADVANCED_COLOR_INFO)",
                ));
            }
            let bits = unsafe { info.Anonymous.value };
            let sdr_white = self.sdr_white_level(target);
            Ok(DisplayColorState {
                source: ColorStateSource::AdvancedColorInfo,
                advanced_color_supported: bits & 0x01 != 0,
                // `advancedColorEnabled` (bit 1) is set for wide colour too, which
                // is why it is not reported as "HDR active".
                advanced_color_active: bits & 0x02 != 0,
                advanced_color_limited_by_policy: None,
                hdr_supported: None,
                hdr_user_enabled: None,
                wide_color_supported: None,
                wide_color_user_enabled: None,
                active_mode: None,
                color_encoding: DisplayColorEncoding::from_raw(info.colorEncoding as u32),
                bits_per_color: info.bitsPerColorChannel,
                sdr_white_level_raw: sdr_white,
                sdr_white_millinits: sdr_white.map(DisplayColorState::sdr_white_millinits_of),
            })
        }

        /// `GET_SDR_WHITE_LEVEL`: a multiplier of 80 nits, multiplied by 1000.
        /// A refusal is reported as "unknown", not as 0 nits.
        fn sdr_white_level(&self, target: &PanelTarget) -> Option<u32> {
            let mut info = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            info.header.size = u32::try_from(std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>())
                .expect("SDR white level size fits u32");
            info.header.adapterId = luid(target);
            info.header.id = target.target_id;
            let status = unsafe { DisplayConfigGetDeviceInfo(&mut info.header) };
            (status == 0).then_some(info.SDRWhiteLevel)
        }

        fn session_monitors(&self) -> Result<Vec<MonitorSnapshot>> {
            let mut snapshots: Vec<MonitorSnapshot> = Vec::new();
            let ok = unsafe {
                EnumDisplayMonitors(
                    ptr::null_mut(),
                    ptr::null(),
                    Some(collect_monitor),
                    &mut snapshots as *mut Vec<MonitorSnapshot> as LPARAM,
                )
            };
            if ok == 0 {
                return Err(display_access_error(
                    std::io::Error::last_os_error().raw_os_error().map_or(0, |code| {
                        u32::try_from(code).unwrap_or(0)
                    }),
                    "EnumDisplayMonitors",
                ));
            }
            Ok(snapshots)
        }
    }

    fn scaling_of(snapshot: &MonitorSnapshot, aware: bool) -> DisplayScalingObservation {
        if !aware {
            return DisplayScalingObservation::ProcessNotPerMonitorAware;
        }
        match DisplayScaling::from_effective_dpi(snapshot.dpi_x, snapshot.dpi_y) {
            Some(scaling) => DisplayScalingObservation::Effective(scaling),
            None => DisplayScalingObservation::NoMonitor,
        }
    }

    unsafe extern "system" fn collect_monitor(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        let snapshots = unsafe { &mut *(data as *mut Vec<MonitorSnapshot>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = u32::try_from(std::mem::size_of::<MONITORINFOEXW>())
            .expect("MONITORINFOEXW size fits u32");
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
            return 1;
        }
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        let status = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let (dpi_x, dpi_y) = if status == 0 { (dpi_x, dpi_y) } else { (0, 0) };
        snapshots.push(MonitorSnapshot {
            device_name: info.szDevice,
            left: info.monitorInfo.rcMonitor.left,
            top: info.monitorInfo.rcMonitor.top,
            right: info.monitorInfo.rcMonitor.right,
            bottom: info.monitorInfo.rcMonitor.bottom,
            dpi_x,
            dpi_y,
        });
        1
    }

    fn current_session_id() -> u32 {
        let mut session = 0u32;
        let ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
        if ok == 0 {
            return 0;
        }
        session
    }

    fn active_console_session_id() -> Option<u32> {
        let console = unsafe { WTSGetActiveConsoleSessionId() };
        (console != NO_CONSOLE_SESSION).then_some(console)
    }

    fn luid(target: &PanelTarget) -> LUID {
        LUID {
            LowPart: target.adapter.low,
            HighPart: target.adapter.high,
        }
    }

    fn display_config_error(status: i32, context: &str) -> lctrl_core::LctrlError {
        // `DisplayConfigGetDeviceInfo` returns a Win32 error code.
        display_access_error(u32::try_from(status).unwrap_or(0), context)
    }

    /// A display API refusal is about the calling session, not about
    /// administrator rights: a service or SSH logon cannot read the console
    /// session's topology, and saying "administrator" would send the operator to
    /// the wrong fix.
    fn display_access_error(code: u32, context: &str) -> lctrl_core::LctrlError {
        if code == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED {
            return lctrl_core::LctrlError::PermissionDenied {
                need: "access to the active Windows desktop session".into(),
            };
        }
        map_win_error(code, context)
    }

    fn utf16_string(raw: &[u16]) -> String {
        let length = raw.iter().position(|value| *value == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..length])
    }

    #[cfg(test)]
    mod tests {
        use super::{AdvancedColorInfo2, GET_ADVANCED_COLOR_INFO_2};
        use std::mem::size_of;

        #[test]
        fn the_24h2_colour_request_has_the_documented_layout() {
            // 20-byte header + value + colorEncoding + bitsPerColorChannel +
            // activeColorMode; a short structure would let the API write past it.
            assert_eq!(size_of::<AdvancedColorInfo2>(), 36);
            assert_eq!(GET_ADVANCED_COLOR_INFO_2, 15);
        }
    }
}

#[cfg(windows)]
mod registry {
    use std::ptr;

    use lctrl_core::{LctrlError, MonitorColorStore, Result};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
        RegQueryInfoKeyW, RegQueryValueExW,
    };

    use super::{MONITOR_DATA_STORE, MonitorColorStoreReader};
    use crate::map_win_error;
    use crate::panel_identity::NativePanelRegistry;
    use crate::PanelRegistry;

    /// `ERROR_FILE_NOT_FOUND` (2): the value does not exist.
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    /// A bounded subkey scan: the store holds one entry per monitor the driver
    /// has seen, so a runaway count means the key is not what we expect.
    const MAX_STORE_ENTRIES: u32 = 512;
    /// Longest subkey name accepted; the store name is
    /// `<mfg><product><serial>_<week>_<year>_<byte>`.
    const MAX_STORE_NAME: usize = 128;

    /// The display driver's per-monitor colour state.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeMonitorColorStore;

    impl MonitorColorStoreReader for NativeMonitorColorStore {
        fn color_store(&self, hardware_id: &str) -> Result<Option<MonitorColorStore>> {
            if hardware_id.is_empty() || !hardware_id.is_ascii() {
                return Err(LctrlError::InvalidArgument {
                    detail: format!("monitor hardware id {hardware_id:?} is not ASCII"),
                });
            }
            let root = open_store()?;
            let mut matches = Vec::new();
            for name in root.subkeys()? {
                if name.starts_with(hardware_id) {
                    matches.push(name);
                }
            }
            match matches.len() {
                0 => Ok(None),
                1 => {
                    let key = root.open(&matches[0])?;
                    Ok(Some(MonitorColorStore {
                        hdr_enabled: key.dword("HDREnabled")?.map(|value| value != 0),
                        sdr_white_level_raw: key.dword("SDRWhiteLevel")?,
                        auto_color_management_supported: key
                            .dword("AutoColorManagementSupported")?
                            .map(|value| value != 0),
                        auto_color_management_enabled: key
                            .dword("AutoColorManagementEnabled")?
                            .map(|value| value != 0),
                        display_hdr_level_notified: key
                            .dword("DisplayHdrLevelNotified")?
                            .map(|value| value != 0),
                    }))
                }
                count => Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "the driver colour store has {count} entries for monitor {hardware_id}; the join is ambiguous"
                    ),
                }),
            }
        }
    }

    /// Delegate to [`NativePanelRegistry`] so `WindowsDisplayStatus` can use a
    /// single type parameter that satisfies both `PanelRegistry` and
    /// `MonitorColorStoreReader`.
    impl PanelRegistry for NativeMonitorColorStore {
        fn edid(&self, device_instance: &str) -> Result<Vec<u8>> {
            NativePanelRegistry.edid(device_instance)
        }
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl Key {
        fn open(&self, name: &str) -> Result<Self> {
            let path = wide(name);
            let mut handle: HKEY = ptr::null_mut();
            let status = unsafe { RegOpenKeyExW(self.0, path.as_ptr(), 0, KEY_READ, &mut handle) };
            if status != 0 {
                return Err(map_win_error(
                    status,
                    &format!("open monitor colour store subkey {name:?}"),
                ));
            }
            Ok(Self(handle))
        }

        fn subkeys(&self) -> Result<Vec<String>> {
            let mut count = 0u32;
            let mut max_name = 0u32;
            let status = unsafe {
                RegQueryInfoKeyW(
                    self.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    &mut count,
                    &mut max_name,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != 0 {
                return Err(map_win_error(status, "RegQueryInfoKeyW monitor colour store"));
            }
            if count > MAX_STORE_ENTRIES || max_name > MAX_STORE_NAME as u32 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "driver colour store reported {count} subkeys with names up to {max_name} characters"
                    ),
                });
            }
            let mut names = Vec::with_capacity(count as usize);
            let mut index = 0u32;
            while index < count {
                let mut buffer = [0u16; MAX_STORE_NAME + 1];
                let mut length = MAX_STORE_NAME as u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buffer.as_mut_ptr(),
                        &mut length,
                        ptr::null(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    )
                };
                if status != 0 {
                    return Err(map_win_error(status, "RegEnumKeyExW monitor colour store"));
                }
                names.push(String::from_utf16_lossy(
                    &buffer[..usize::try_from(length).unwrap_or(0)],
                ));
                index = index
                    .checked_add(1)
                    .ok_or_else(|| LctrlError::ChannelUnavailable {
                        channel: "driver colour store subkey index overflow".into(),
                    })?;
            }
            Ok(names)
        }

        /// `Ok(None)` when the value is absent; a value of the wrong type is an
        /// error, not "absent".
        fn dword(&self, name: &str) -> Result<Option<u32>> {
            let value = wide(name);
            let mut data = 0u32;
            let mut bytes = u32::try_from(std::mem::size_of::<u32>()).expect("u32 size fits");
            let mut kind = 0u32;
            let status = unsafe {
                RegQueryValueExW(
                    self.0,
                    value.as_ptr(),
                    ptr::null(),
                    &mut kind,
                    (&mut data as *mut u32).cast::<u8>(),
                    &mut bytes,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            if status != 0 {
                return Err(map_win_error(status, &format!("read {name}")));
            }
            if kind != REG_DWORD || bytes != 4 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "{name} is registry type {kind} with {bytes} bytes; expected REG_DWORD"
                    ),
                });
            }
            Ok(Some(data))
        }
    }

    fn open_store() -> Result<Key> {
        let path = wide(MONITOR_DATA_STORE);
        let mut handle: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut handle) };
        if status != 0 {
            return Err(map_win_error(status, "open the monitor colour store"));
        }
        Ok(Key(handle))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use native::NativeDisplayState;
#[cfg(windows)]
pub use registry::NativeMonitorColorStore;
