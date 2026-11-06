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

impl fmt::Display for BacklightState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "level={} effect={}", self.level, self.effect)
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

/// One independently observed input leaf in an explicit touchpad recovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadRecoveryLeaf {
    pub role: &'static str,
    pub previous: DeviceState,
    pub requested: DeviceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<DeviceState>,
}

/// Recovery acts on the admitted input leaves only; the ACPI controller, the
/// configuration collection and the vendor-defined collection are never
/// targeted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadRecoveryReport {
    pub mode: crate::ApplyMode,
    pub strategy: TouchpadRecoveryStrategy,
    pub leaves: Vec<TouchpadRecoveryLeaf>,
}

/// Which repair a recovery command performs, derived from the observed leaves
/// rather than from a caller flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchpadRecoveryStrategy {
    /// At least one input leaf was observed disabled (an interrupted
    /// transition): only those leaves are enabled.
    EnableDisabledLeaves,
    /// Every input leaf was already enabled while the pad does not serve
    /// input: each leaf is disabled and re-enabled so its driver
    /// re-initialises the collection.
    CycleInputLeaves,
}

/// Top-level HID collection a capability descriptor was read from.
///
/// A reported touchpad stays bound to the precision-touchpad collection
/// (`0x000D/0x0005`); the configuration and vendor-defined collections of the
/// same physical pad are never decoded as touchpad capabilities.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadUsage {
    pub page: u16,
    pub usage: u16,
}

/// Hardware identity the HID collection reports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
}

/// Report sizes and link-collection count from the parsed report descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadReports {
    pub input_bytes: u16,
    pub output_bytes: u16,
    pub feature_bytes: u16,
    pub link_collections: u16,
}

/// Contact reporting the collection declares.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadContacts {
    /// Finger link collections present in the input report. This is the number
    /// of contacts the descriptor can carry in one frame, not a measurement.
    pub declared_collections: u8,
    /// Declared logical maximum of Contact Identifier.
    pub identifier_max: i32,
    /// Declared logical maximum of the Contact Count field.
    pub count_field_max: i32,
    /// Contact Count Maximum returned by the device capabilities feature
    /// report. Absent when that report could not be read; the value is never
    /// defaulted from `declared_collections`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_maximum: Option<u16>,
}

/// One declared axis range. `unit` and `unit_exponent` are reported raw so a
/// caller can verify any derived conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadAxis {
    pub logical_min: i32,
    pub logical_max: i32,
    pub physical_min: i32,
    pub physical_max: i32,
    pub unit: u32,
    pub unit_exponent: i32,
}

/// Surface size derived from the declared physical range. This is arithmetic
/// over the firmware's declaration, not a measurement of the pad.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TouchpadDerivedSize {
    pub width_mm: f32,
    pub height_mm: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TouchpadSurface {
    pub x: TouchpadAxis,
    pub y: TouchpadAxis,
    /// Derived from the declared physical range; absent when the declared unit
    /// is not a length unit the backend converts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derived_size_mm: Option<TouchpadDerivedSize>,
}

/// Button implementation declared by the device capabilities feature report
/// (usage `0x000D/0x0059`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchpadButtonType {
    /// Button type 0: depressible click-pad.
    Depressible,
    /// Button type 1: non-depressible pressure-pad.
    NonDepressible,
    /// Button type 2: non-clickable discrete-pad.
    NonClickable,
    Other(u16),
}

impl TouchpadButtonType {
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Depressible,
            1 => Self::NonDepressible,
            2 => Self::NonClickable,
            other => Self::Other(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TouchpadButtons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_type: Option<TouchpadButtonType>,
    /// Button 1 (`0x0009/0x0001`) declared in the input report.
    pub integrated_button: bool,
}

/// Capability bits derived from the declared contacts and usages. Windows
/// synthesizes gestures from those contacts, so a set bit means "the
/// collection declares the input a gesture needs", not "the gesture is
/// enabled in Windows".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TouchpadSupportBits {
    pub two_finger: bool,
    pub three_finger: bool,
    pub four_finger: bool,
    pub five_finger: bool,
    pub pressure: bool,
    pub palm_confidence: bool,
    pub haptic_intensity: bool,
    pub button_press_threshold: bool,
}

/// Read-only capability descriptor of one precision touchpad, as declared by
/// its HID top-level collection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TouchpadCapabilities {
    pub instance_id: String,
    pub usage: TouchpadUsage,
    pub identity: TouchpadIdentity,
    pub reports: TouchpadReports,
    pub contacts: TouchpadContacts,
    pub surface: TouchpadSurface,
    pub buttons: TouchpadButtons,
    pub support: TouchpadSupportBits,
    /// Why the device capabilities feature report could not be read, when it
    /// could not. The descriptor facts above stay valid in that case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_report_error: Option<String>,
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
