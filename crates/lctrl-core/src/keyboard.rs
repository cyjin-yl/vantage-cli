use std::fmt;

use serde::Serialize;

use crate::{LctrlError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LightingEffect {
    Static,
    Breathing,
    Wave,
    Reactive,
    Flashing,
    Unknown(u8),
}

impl LightingEffect {
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Static,
            1 => Self::Breathing,
            2 => Self::Wave,
            3 => Self::Reactive,
            4 => Self::Flashing,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Static => 0,
            Self::Breathing => 1,
            Self::Wave => 2,
            Self::Reactive => 3,
            Self::Flashing => 4,
            Self::Unknown(raw) => raw,
        }
    }
}

impl fmt::Display for LightingEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static => f.write_str("static"),
            Self::Breathing => f.write_str("breathing"),
            Self::Wave => f.write_str("wave"),
            Self::Reactive => f.write_str("reactive"),
            Self::Flashing => f.write_str("flashing"),
            Self::Unknown(raw) => write!(f, "unknown ({raw})"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BacklightState {
    pub level: u8,
    pub max_level: u8,
    pub effect: LightingEffect,
}

impl BacklightState {
    pub fn new(level: u8, max_level: u8, effect: LightingEffect) -> Result<Self> {
        if max_level == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: "backlight max_level must be nonzero".into(),
            });
        }
        if level > max_level {
            return Err(LctrlError::InvalidArgument {
                detail: format!("backlight level {level} exceeds max {max_level}"),
            });
        }
        Ok(Self {
            level,
            max_level,
            effect,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    Unlocked,
    Locked,
}

impl LockState {
    #[must_use]
    pub const fn from_raw(raw: u32, inverted: bool) -> Self {
        let active = raw != 0;
        let locked = if inverted { !active } else { active };
        if locked { Self::Locked } else { Self::Unlocked }
    }
}

impl fmt::Display for LockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unlocked => "unlocked",
            Self::Locked => "locked",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Enabled,
    Disabled,
}

impl DeviceState {
    #[must_use]
    pub const fn from_raw(raw: u32, inverted: bool) -> Self {
        let active = raw != 0;
        let enabled = if inverted { !active } else { active };
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    #[must_use]
    pub const fn raw(self, inverted: bool) -> u32 {
        let enabled = matches!(self, Self::Enabled);
        let active = if inverted { !enabled } else { enabled };
        if active { 1 } else { 0 }
    }
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        })
    }
}
