use std::fmt;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelDisplayMode {
    Srgb,
    DciP3,
    AdobeRgb,
    Custom,
    Cinema,
    Unknown(u32),
}

impl PanelDisplayMode {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Srgb,
            1 => Self::DciP3,
            2 => Self::AdobeRgb,
            3 => Self::Custom,
            4 => Self::Cinema,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Srgb => 0,
            Self::DciP3 => 1,
            Self::AdobeRgb => 2,
            Self::Custom => 3,
            Self::Cinema => 4,
            Self::Unknown(raw) => raw,
        }
    }
}

impl fmt::Display for PanelDisplayMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Srgb => f.write_str("srgb"),
            Self::DciP3 => f.write_str("dci-p3"),
            Self::AdobeRgb => f.write_str("adobe-rgb"),
            Self::Custom => f.write_str("custom"),
            Self::Cinema => f.write_str("cinema"),
            Self::Unknown(raw) => write!(f, "unknown ({raw})"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GamutMode {
    Srgb65,
    DciP3Full,
    Unknown(u32),
}

impl GamutMode {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Srgb65,
            1 => Self::DciP3Full,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LowLatencyMode {
    Off,
    Medium,
    High,
    Unknown(u32),
}

impl LowLatencyMode {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Off,
            1 => Self::Medium,
            2 => Self::High,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelSupportBits(u32);

impl PanelSupportBits {
    pub const PIP: u32 = 1 << 0;
    pub const LOW_LATENCY: u32 = 1 << 1;
    pub const GAME_AID: u32 = 1 << 2;
    pub const MPRT: u32 = 1 << 3;
    pub const GAMUT: u32 = 1 << 4;
    pub const GAME_AID_FPS: u32 = 1 << 5;

    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn supports(self, bit: u32) -> bool {
        self.0 & bit != 0
    }
}

/// Current brightness and selectable percentage levels for one uniquely
/// identified active internal panel. Monitor instance identities stay private.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PanelBrightness {
    pub current_percent: u8,
    pub supported_levels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PanelRefreshCapability {
    pub min_hz: u16,
    pub max_hz: u16,
    pub default_hz: u16,
}

impl PanelRefreshCapability {
    pub fn new(min_hz: u16, max_hz: u16, default_hz: u16) -> Self {
        Self {
            min_hz,
            max_hz,
            default_hz,
        }
    }

    pub fn supports_hz(&self, hz: u16) -> bool {
        (self.min_hz..=self.max_hz).contains(&hz)
    }
}

/// A requested panel refresh transition. `Auto` deliberately resolves to the
/// firmware-advertised default and still requires an exact native mode; it is
/// not a synthetic success or an unverified adaptive-VRR write.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelRefreshRequest {
    Fixed(u16),
    Auto,
}

impl fmt::Display for PanelRefreshRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(hz) => write!(f, "{hz} Hz"),
            Self::Auto => f.write_str("auto (firmware default)"),
        }
    }
}

/// The LUID of the adapter owning a display path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct PanelAdapterId {
    pub high: i32,
    pub low: u32,
}

impl PanelAdapterId {
    #[must_use]
    pub const fn new(high: i32, low: u32) -> Self {
        Self { high, low }
    }
}

/// A connected display path. Only a single `internal` target is admitted by
/// the panel controller; external and ambiguous topologies are not writable.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct PanelTarget {
    pub device_name: String,
    pub adapter: PanelAdapterId,
    pub source_id: u32,
    pub target_id: u32,
    pub internal: bool,
}

impl PanelTarget {
    #[must_use]
    pub fn new(
        device_name: impl Into<String>,
        adapter: PanelAdapterId,
        source_id: u32,
        target_id: u32,
        internal: bool,
    ) -> Self {
        Self {
            device_name: device_name.into(),
            adapter,
            source_id,
            target_id,
            internal,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelRotation {
    Default,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl PanelRotation {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Default),
            1 => Some(Self::Rotate90),
            2 => Some(Self::Rotate180),
            3 => Some(Self::Rotate270),
            _ => None,
        }
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Default => 0,
            Self::Rotate90 => 1,
            Self::Rotate180 => 2,
            Self::Rotate270 => 3,
        }
    }
}

/// A complete native mode for refresh-only transitions. Equality includes
/// path identity and every layout property that must survive a rate change.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PanelMode {
    pub target: PanelTarget,
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
    pub refresh_hz: u32,
    pub position_x: i32,
    pub position_y: i32,
    pub rotation: PanelRotation,
}

impl PanelMode {
    #[must_use]
    pub fn new(
        target: PanelTarget,
        width: u32,
        height: u32,
        bits_per_pixel: u32,
        refresh_hz: u32,
    ) -> Self {
        Self {
            target,
            width,
            height,
            bits_per_pixel,
            refresh_hz,
            position_x: 0,
            position_y: 0,
            rotation: PanelRotation::Default,
        }
    }

    #[must_use]
    pub fn with_refresh_hz(&self, refresh_hz: u32) -> Self {
        let mut mode = self.clone();
        mode.refresh_hz = refresh_hz;
        mode
    }

    #[must_use]
    pub fn same_mode_except_refresh(&self, other: &Self) -> bool {
        self.target == other.target
            && self.width == other.width
            && self.height == other.height
            && self.bits_per_pixel == other.bits_per_pixel
            && self.position_x == other.position_x
            && self.position_y == other.position_y
            && self.rotation == other.rotation
    }
}

impl fmt::Display for PanelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}x{}@{}Hz {}bpp rotation={} position=({}, {}) adapter={:08x}:{:08x} source={} target={}",
            self.target.device_name,
            self.width,
            self.height,
            self.refresh_hz,
            self.bits_per_pixel,
            self.rotation.raw(),
            self.position_x,
            self.position_y,
            self.target.adapter.high,
            self.target.adapter.low,
            self.target.source_id,
            self.target.target_id,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Manual,
    Adaptive,
    Performance,
    Unknown(u16),
}

/// Eye-care (low blue light) level applied through a 1D pixel-transformation
/// LUT. `Custom` reports a LUT that matches no known preset.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EyeCareLevel {
    Off,
    Mid,
    High,
    Custom,
}

impl fmt::Display for EyeCareLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("off"),
            Self::Mid => f.write_str("mid"),
            Self::High => f.write_str("high"),
            Self::Custom => f.write_str("custom"),
        }
    }
}

impl RefreshMode {
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Manual,
            1 => Self::Adaptive,
            2 => Self::Performance,
            other => Self::Unknown(other),
        }
    }
}
