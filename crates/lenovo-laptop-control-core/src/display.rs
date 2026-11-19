//! Read-only display state: how a display is attached, how fast it scans out,
//! how much Windows scales it, and whether advanced colour (HDR or wide colour)
//! is declared, enabled, and active.
//!
//! # Two scopes, never merged
//!
//! Windows exposes display state at two different scopes, and they disagree on
//! a machine where the caller is not on the console desktop:
//!
//! - **Session scope** — `QueryDisplayConfig`, `EnumDisplaySettingsExW`,
//!   `GetDpiForMonitor` and the advanced-colour `DisplayConfigGetDeviceInfo`
//!   requests all describe the *calling session's* desktop. Called from a
//!   session 0 service or SSH logon they see that session's synthetic desktop
//!   (a `WinDisc` headless surface), and `QueryDisplayConfig` is refused with
//!   `ERROR_ACCESS_DENIED`.
//! - **Machine scope** — the display driver keeps per-monitor state machine
//!   wide (`GraphicsDrivers\MonitorDataStore`, keyed by monitor hardware id) and
//!   the monitor's own EDID is stored per PnP instance. Both are readable
//!   without a desktop, and they describe the physical panel.
//!
//! Every record therefore carries a [`DisplayScope`] so a consumer can never
//! mistake a machine-wide driver value for the current session's live state, or
//! a session's synthetic desktop for the panel.

use std::fmt;

use serde::Serialize;

use crate::PanelMode;

/// Where one observation came from. See the module documentation: the scopes
/// are not interchangeable and are reported per record.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayScope {
    /// A live display path of the calling session's desktop.
    Session,
    /// The calling session's own monitor list. Reported only when the live path
    /// enumeration is unavailable, to name what that session does have; a
    /// service/SSH logon sees a synthetic surface here, which is not a panel.
    SessionVirtual,
    /// Machine-wide state of a connected monitor (driver colour store, EDID).
    Machine,
}

impl fmt::Display for DisplayScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session => f.write_str("session"),
            Self::SessionVirtual => f.write_str("session-virtual"),
            Self::Machine => f.write_str("machine"),
        }
    }
}

/// The session whose desktop a live display read describes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DisplaySession {
    /// Session id of the calling process.
    pub id: u32,
    /// Session attached to the console, `None` when the OS reports none.
    pub console_session_id: Option<u32>,
    /// True when the caller runs in the console session, i.e. its display APIs
    /// describe the physical desktop rather than a synthetic one.
    pub interactive: bool,
}

/// How a display is attached, classified from
/// `DISPLAYCONFIG_PATH_TARGET_INFO::outputTechnology`.
///
/// `Virtual` is kept separate from `External` on purpose: a remote-desktop or
/// indirect display adapter publishes an output technology too, and such a
/// path must never be summarised as a monitor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayConnection {
    /// Embedded panel: `INTERNAL` (`0x8000_0000`), `LVDS`, embedded
    /// DisplayPort, or embedded UDI.
    Internal,
    /// A cable-attached sink (`HD15`, `DVI`, `HDMI`, external `DisplayPort`,
    /// `UDI_EXTERNAL`, or a TV/set-top dongle output).
    External,
    /// `MIRACAST`, `INDIRECT_WIRED`, `INDIRECT_VIRTUAL`, or
    /// `DISPLAYPORT_USB_TUNNEL`: a synthetic or tunnelled output.
    Virtual,
    /// A code this build does not classify.
    Unknown(u32),
}

impl DisplayConnection {
    const INTERNAL: u32 = 0x8000_0000;
    const LVDS: u32 = 6;
    const DISPLAYPORT_EMBEDDED: u32 = 11;
    const UDI_EMBEDDED: u32 = 13;
    const MIRACAST: u32 = 15;
    const INDIRECT_WIRED: u32 = 16;
    const INDIRECT_VIRTUAL: u32 = 17;
    const DISPLAYPORT_USB_TUNNEL: u32 = 18;

    #[must_use]
    pub const fn from_output_technology(raw: u32) -> Self {
        match raw {
            Self::INTERNAL | Self::LVDS | Self::DISPLAYPORT_EMBEDDED | Self::UDI_EMBEDDED => {
                Self::Internal
            }
            Self::MIRACAST | Self::INDIRECT_WIRED | Self::INDIRECT_VIRTUAL
            | Self::DISPLAYPORT_USB_TUNNEL => Self::Virtual,
            // 0..=5 (HD15, S-Video, composite, component, DVI, HDMI), 8/9
            // (D_JPN, SDI), 10 (external DisplayPort), 12 (UDI external) and 14
            // (SDTV dongle) are cable-attached sinks.
            0..=5 | 8..=10 | 12 | 14 => Self::External,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Internal => Self::INTERNAL,
            Self::External => 0,
            Self::Virtual => Self::INDIRECT_VIRTUAL,
            Self::Unknown(raw) => raw,
        }
    }

    #[must_use]
    pub const fn is_internal(self) -> bool {
        matches!(self, Self::Internal)
    }
}

impl fmt::Display for DisplayConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal => f.write_str("internal"),
            Self::External => f.write_str("external"),
            Self::Virtual => f.write_str("virtual"),
            Self::Unknown(raw) => write!(f, "unknown ({raw:#010x})"),
        }
    }
}

/// The scaling Windows actually applies to one monitor.
///
/// `percent` comes from the OS effective DPI of that monitor
/// (`GetDpiForMonitor(MDT_EFFECTIVE_DPI)`), which is what the Settings "Scale"
/// control shows. It is never re-derived from resolution: a resolution ratio
/// cannot tell a 200 % scale on a 3072-pixel panel from a 1536-pixel mode.
///
/// The persisted `GraphicsDrivers\ScaleFactors` `DpiValue` is deliberately not
/// used: the OS stores it *relative to the recommended scale step*, so a bare
/// `DpiValue` cannot be converted to a percentage without the recommended
/// value, and guessing that value would invent a number.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DisplayScaling {
    /// Which channel produced [`Self::percent`].
    #[serde(rename = "source")]
    pub source: ScalingSource,
    pub dpi_x: u32,
    pub dpi_y: u32,
    /// `dpi_x / 96 * 100`, rounded up at the half point (96 dpi is 100 %).
    pub percent: u32,
}

/// Channel behind a [`DisplayScaling`] value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalingSource {
    /// Live `GetDpiForMonitor(MDT_EFFECTIVE_DPI)` for the monitor that covers
    /// this display path (requires a per-monitor DPI aware caller).
    EffectiveDpi,
}

impl DisplayScaling {
    /// `None` when the OS reports a zero DPI, which is not a scale factor.
    #[must_use]
    pub fn from_effective_dpi(dpi_x: u32, dpi_y: u32) -> Option<Self> {
        if dpi_x == 0 || dpi_y == 0 {
            return None;
        }
        Some(Self {
            source: ScalingSource::EffectiveDpi,
            dpi_x,
            dpi_y,
            percent: scale_percent(dpi_x),
        })
    }
}

/// What a per-monitor scaling read produced.
///
/// The three cases are kept apart because "no monitor covers this path" and
/// "this process cannot see per-monitor DPI" are different facts, and neither is
/// a scale of 100 %.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayScalingObservation {
    /// The OS reported the effective DPI of the monitor covering this path.
    Effective(DisplayScaling),
    /// No monitor in the session covers this path's desktop position.
    NoMonitor,
    /// Windows would virtualise the answer: the process is not per-monitor DPI
    /// aware, so `GetDpiForMonitor` reports the unaware/system value instead of
    /// the scale the user selected. Reporting that number would be wrong.
    ProcessNotPerMonitorAware,
}

impl DisplayScalingObservation {
    #[must_use]
    pub const fn effective(self) -> Option<DisplayScaling> {
        match self {
            Self::Effective(scaling) => Some(scaling),
            Self::NoMonitor | Self::ProcessNotPerMonitorAware => None,
        }
    }
}

/// Windows treats 96 dpi as 100 % and the Settings steps are 25 %; the OS
/// rounds the ratio to the nearest whole percent, so a 144 dpi monitor is
/// reported as 150 % and a 192 dpi one as 200 %.
fn scale_percent(dpi: u32) -> u32 {
    let scaled = u64::from(dpi) * 100 + 48;
    u32::try_from(scaled / 96).unwrap_or(u32::MAX)
}

/// `DISPLAYCONFIG_COLOR_ENCODING` of the active target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayColorEncoding {
    Rgb,
    YCbCr444,
    YCbCr422,
    YCbCr420,
    Intensity,
    Unknown(u32),
}

impl DisplayColorEncoding {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Rgb,
            1 => Self::YCbCr444,
            2 => Self::YCbCr422,
            3 => Self::YCbCr420,
            4 => Self::Intensity,
            other => Self::Unknown(other),
        }
    }
}

/// `DISPLAYCONFIG_ADVANCED_COLOR_MODE` from the 24H2 request. The three states
/// are kept apart because "wide colour gamut is on" is not "HDR is on".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveColorMode {
    Sdr,
    WideColorGamut,
    Hdr,
    Unknown(u32),
}

impl ActiveColorMode {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Sdr,
            1 => Self::WideColorGamut,
            2 => Self::Hdr,
            other => Self::Unknown(other),
        }
    }
}

/// Advanced-colour state of one live display target.
///
/// The fields are reported verbatim from the display-config request that
/// produced them. `hdr_*`/`wide_color_*`/`active_mode` exist only on the 24H2
/// request (`GET_ADVANCED_COLOR_INFO_2`); a pre-24H2 target answers the legacy
/// request, whose `advanced_color_supported` means "HDR **or** wide colour", so
/// the HDR fields stay `None` instead of being inferred from it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DisplayColorState {
    /// Which display-config request produced this record.
    pub source: ColorStateSource,
    /// Bit 0: the target supports advanced colour (HDR or wide colour).
    pub advanced_color_supported: bool,
    /// Bit 1: advanced colour is active (legacy `advancedColorEnabled`).
    pub advanced_color_active: bool,
    /// Bit 3 on the 24H2 request: advanced colour is limited by policy.
    pub advanced_color_limited_by_policy: Option<bool>,
    /// Bit 4 (24H2 only): HDR is supported. Distinct from wide colour.
    pub hdr_supported: Option<bool>,
    /// Bit 5 (24H2 only): the user has HDR switched on.
    pub hdr_user_enabled: Option<bool>,
    /// Bit 6 (24H2 only): wide colour is supported.
    pub wide_color_supported: Option<bool>,
    /// Bit 7 (24H2 only): the user has wide colour switched on.
    pub wide_color_user_enabled: Option<bool>,
    /// `activeColorMode` (24H2 only): what is actually being output.
    pub active_mode: Option<ActiveColorMode>,
    pub color_encoding: DisplayColorEncoding,
    pub bits_per_color: u32,
    /// Raw `SDRWhiteLevel`: a multiplier of 80 nits, multiplied by 1000.
    pub sdr_white_level_raw: Option<u32>,
    /// [`Self::sdr_white_level_raw`] converted with the documented formula
    /// (`raw / 1000 * 80` nits), in millinits so the value stays exact.
    pub sdr_white_millinits: Option<u32>,
}

/// Display-config request behind a [`DisplayColorState`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorStateSource {
    /// `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO_2` (24H2+, type 15).
    AdvancedColorInfo2,
    /// `DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO` (type 9). Cannot
    /// distinguish HDR from wide colour.
    AdvancedColorInfo,
}

impl DisplayColorState {
    /// Convert a raw `SDRWhiteLevel` with the documented unit.
    #[must_use]
    pub fn sdr_white_millinits_of(raw: u32) -> u32 {
        // value = (nits / 80) * 1000  =>  millinits = value * 80
        raw.saturating_mul(80)
    }
}

/// One live display path of the calling session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DisplayPathState {
    pub scope: DisplayScope,
    pub device_name: String,
    pub connection: DisplayConnection,
    /// The current desktop mode of this path, as the OS reports it (pixels,
    /// bits per pixel, refresh rate, position, rotation).
    pub mode: PanelMode,
    pub scaling: DisplayScalingObservation,
    /// `None` when the target rejected the colour query.
    pub color: Option<DisplayColorState>,
}

/// One monitor in the calling session's own monitor list.
///
/// Only reported when the live path enumeration is unavailable; see
/// [`DisplayScope::SessionVirtual`]. `device_name`, width and height describe
/// the session's desktop surface, not necessarily a physical panel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionDesktop {
    pub scope: DisplayScope,
    pub device_name: String,
    pub width: u32,
    pub height: u32,
    pub scaling: DisplayScalingObservation,
}

/// What a monitor's own EDID says about HDR.
///
/// "Declares no HDR block" and "the EDID could not be read" are different
/// observations: the first is a property of an SDR-only sink, the second is a
/// missing capability, and a record whose EDID failed must not be summarised as
/// an SDR sink.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrCapabilityObservation {
    /// The EDID carries a CTA-861 HDR Static Metadata block.
    Declared(crate::PanelHdrCapability),
    /// The EDID was read and declares no HDR Static Metadata block.
    NotDeclared,
    /// The monitor's EDID could not be read or decoded, and why.
    Unavailable(String),
}

impl HdrCapabilityObservation {
    #[must_use]
    pub const fn declared(&self) -> Option<&crate::PanelHdrCapability> {
        match self {
            Self::Declared(capability) => Some(capability),
            Self::NotDeclared | Self::Unavailable(_) => None,
        }
    }
}

/// Machine-wide colour state of one connected monitor.
///
/// Every field is state the display driver keeps for the monitor itself, keyed
/// by its hardware id (`LEN8AD7`), and readable without a desktop session. The
/// values are the driver's persisted state, not a live read of the current
/// signal, so they are reported under [`DisplayScope::Machine`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MonitorColorState {
    pub scope: DisplayScope,
    /// PnP monitor hardware id (`<manufacturer><product code>`), the join key
    /// between the monitor, its EDID, and the driver's colour store. The PnP
    /// *instance* path stays private.
    pub hardware_id: String,
    pub connection: DisplayConnection,
    /// `MonitorDataStore\HDREnabled`.
    pub hdr_enabled: Option<bool>,
    /// `MonitorDataStore\SDRWhiteLevel`, raw (multiplier of 80 nits × 1000).
    pub sdr_white_level_raw: Option<u32>,
    /// The same value in millinits (`raw * 80`).
    pub sdr_white_millinits: Option<u32>,
    /// `MonitorDataStore\AutoColorManagementSupported`.
    pub auto_color_management_supported: Option<bool>,
    /// `MonitorDataStore\AutoColorManagementEnabled`.
    pub auto_color_management_enabled: Option<bool>,
    /// `MonitorDataStore\DisplayHdrLevelNotified`: the OS has latched that it
    /// told the user about this monitor's HDR level. The level itself is not
    /// stored here.
    pub display_hdr_level_notified: Option<bool>,
    /// HDR capability the monitor declares in its own EDID.
    pub hdr_capability: HdrCapabilityObservation,
}

/// Machine-wide auto colour management state of one monitor, without the HDR
/// block. Kept as a separate accessor shape so a caller that cannot read the
/// EDID still gets the driver state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct MonitorColorStore {
    pub hdr_enabled: Option<bool>,
    pub sdr_white_level_raw: Option<u32>,
    pub auto_color_management_supported: Option<bool>,
    pub auto_color_management_enabled: Option<bool>,
    pub display_hdr_level_notified: Option<bool>,
}

impl MonitorColorStore {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            hdr_enabled: None,
            sdr_white_level_raw: None,
            auto_color_management_supported: None,
            auto_color_management_enabled: None,
            display_hdr_level_notified: None,
        }
    }

    #[must_use]
    pub fn sdr_white_millinits(&self) -> Option<u32> {
        self.sdr_white_level_raw
            .map(DisplayColorState::sdr_white_millinits_of)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveColorMode, DisplayColorEncoding, DisplayConnection, DisplayScaling, ScalingSource,
    };

    #[test]
    fn output_technology_classification_keeps_virtual_apart() {
        assert_eq!(
            DisplayConnection::from_output_technology(0x8000_0000),
            DisplayConnection::Internal
        );
        for embedded in [6, 11, 13] {
            assert_eq!(
                DisplayConnection::from_output_technology(embedded),
                DisplayConnection::Internal
            );
        }
        for cable in [0, 5, 10, 12, 14] {
            assert_eq!(
                DisplayConnection::from_output_technology(cable),
                DisplayConnection::External
            );
        }
        for synthetic in [15, 16, 17, 18] {
            assert_eq!(
                DisplayConnection::from_output_technology(synthetic),
                DisplayConnection::Virtual
            );
        }
        assert_eq!(
            DisplayConnection::from_output_technology(99),
            DisplayConnection::Unknown(99)
        );
    }

    #[test]
    fn scaling_is_the_os_ratio_not_a_resolution_ratio() {
        let hundred = DisplayScaling::from_effective_dpi(96, 96).expect("96 dpi is a scale");
        assert_eq!(hundred.percent, 100);
        assert_eq!(hundred.source, ScalingSource::EffectiveDpi);

        assert_eq!(
            DisplayScaling::from_effective_dpi(120, 120)
                .expect("120 dpi is a scale")
                .percent,
            125
        );
        assert_eq!(
            DisplayScaling::from_effective_dpi(144, 144)
                .expect("144 dpi is a scale")
                .percent,
            150
        );
        assert_eq!(
            DisplayScaling::from_effective_dpi(192, 192)
                .expect("192 dpi is a scale")
                .percent,
            200
        );
        assert_eq!(
            DisplayScaling::from_effective_dpi(240, 240)
                .expect("240 dpi is a scale")
                .percent,
            250
        );
        assert!(DisplayScaling::from_effective_dpi(0, 96).is_none());
    }

    #[test]
    fn hdr_and_wide_colour_modes_stay_distinct() {
        assert_eq!(ActiveColorMode::from_raw(0), ActiveColorMode::Sdr);
        assert_eq!(
            ActiveColorMode::from_raw(1),
            ActiveColorMode::WideColorGamut
        );
        assert_eq!(ActiveColorMode::from_raw(2), ActiveColorMode::Hdr);
        assert_eq!(ActiveColorMode::from_raw(7), ActiveColorMode::Unknown(7));
        assert_eq!(
            DisplayColorEncoding::from_raw(3),
            DisplayColorEncoding::YCbCr420
        );
    }

    #[test]
    fn sdr_white_level_uses_the_documented_unit() {
        // 6000 -> (6000 / 1000) * 80 = 480 nits
        assert_eq!(
            super::DisplayColorState::sdr_white_millinits_of(6000),
            480_000
        );
    }
}
