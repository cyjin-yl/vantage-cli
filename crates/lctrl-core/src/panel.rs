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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Manual,
    Adaptive,
    Performance,
    Unknown(u16),
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
