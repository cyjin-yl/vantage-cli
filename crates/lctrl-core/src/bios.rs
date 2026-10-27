use serde::Serialize;

use crate::{LctrlError, Result};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosName(String);

impl BiosName {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "BIOS setting name must not be empty".into(),
            });
        }
        if name.contains('\0') || name.contains(',') || name.contains(';') {
            return Err(LctrlError::InvalidArgument {
                detail: "BIOS setting name must not contain NUL, comma, or semicolon".into(),
            });
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BiosName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosValue(String);

impl BiosValue {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.contains('\0') || value.contains(',') || value.contains(';') {
            return Err(LctrlError::InvalidArgument {
                detail: "BIOS setting value must not contain NUL, comma, or semicolon".into(),
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BiosValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosChange {
    pub name: BiosName,
    pub value: BiosValue,
}

impl BiosChange {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        Ok(Self {
            name: BiosName::new(name)?,
            value: BiosValue::new(value)?,
        })
    }

    pub fn serialized(&self) -> String {
        format!("{},{};", self.name.as_str(), self.value.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosItem {
    pub name: String,
    pub value: String,
    pub selections: Vec<String>,
}

/// One validated BIOS mutation and the state captured before any staging.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosPlannedChange {
    pub previous: BiosChange,
    pub requested: BiosChange,
}

/// A side-effect-free preview of an atomic BIOS apply operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosApplyPlan {
    pub changes: Vec<BiosPlannedChange>,
}

/// A saved setting and its post-save `CurrentSetting` mirror readback.
///
/// The mirror is not evidence that the running hardware state has changed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosAppliedChange {
    pub previous: BiosChange,
    pub requested: BiosChange,
    pub readback: BiosItem,
}

/// What is known about the effect of a successful BIOS NVRAM save.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BiosApplyEffect {
    /// The setting was saved and its mirror was verified, but firmware does
    /// not say whether it is effective now or only after a reboot.
    PendingRebootOrUnknown,
}

/// Receipt for an all-or-nothing BIOS apply operation.
///
/// This deliberately has no `actual` field: `readback` is the firmware's
/// saved-settings mirror, not a claim about the running hardware state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BiosApplyResult {
    pub accepted: bool,
    pub saved: bool,
    pub effect: BiosApplyEffect,
    pub changes: Vec<BiosAppliedChange>,
}

pub fn parse_current_setting(raw: &str) -> Option<BiosItem> {
    let (name, value) = raw.split_once(',')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(BiosItem {
        name: name.to_string(),
        value: value.trim().to_string(),
        selections: Vec::new(),
    })
}

pub fn parse_selections(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn save_parameter() -> &'static str {
    ";"
}

pub fn is_success(return_value: &str) -> bool {
    return_value == "Success"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BiosRisk {
    Normal,
    Disruptive,
    Destructive,
    Experimental,
    Unknown,
}

pub fn classify_risk(name: &str) -> BiosRisk {
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "securerollbackprevention" | "efi-bootorder") {
        BiosRisk::Destructive
    } else if matches!(
        lower.as_str(),
        "usbboot" | "pxeboottolan" | "ipv4pxefirst" | "secureboot" | "graphicsdevice"
    ) {
        BiosRisk::Disruptive
    } else {
        BiosRisk::Normal
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BiosPasswordStatus {
    pub min_length: u32,
    pub max_length: u32,
    pub password_state: u32,
    pub supervisor_set: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_mode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_encodings: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_keyboard: Option<u32>,
}

impl BiosPasswordStatus {
    /// Construct the legacy status subset without inventing metadata that was
    /// not queried. New backends should use [`Self::from_raw_metadata`].
    #[must_use]
    pub const fn from_raw(min_length: u32, max_length: u32, password_state: u32) -> Self {
        Self {
            min_length,
            max_length,
            password_state,
            supervisor_set: password_state & 0x2 != 0,
            password_mode: None,
            supported_encodings: None,
            supported_keyboard: None,
        }
    }

    #[must_use]
    pub const fn from_raw_metadata(
        min_length: u32,
        max_length: u32,
        password_mode: u32,
        password_state: u32,
        supported_encodings: u32,
        supported_keyboard: u32,
    ) -> Self {
        Self {
            min_length,
            max_length,
            password_state,
            supervisor_set: password_state & 0x2 != 0,
            password_mode: Some(password_mode),
            supported_encodings: Some(supported_encodings),
            supported_keyboard: Some(supported_keyboard),
        }
    }
}

/// Decoded `LENOVO_BIOS_ASSISTANT.GetCapabilityValue` data.
///
/// Capability bits are meaningful only when `command_accepted` is true. The
/// GPU field preserves the documented two-bit field without inferring a list
/// of supported GPU modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BiosAssistantCapabilities {
    pub raw: u32,
    pub command_accepted: bool,
    pub super_boost_supported: bool,
    pub gpu_mode_support: u8,
    pub fn_ctrl_swap_supported: bool,
    pub major_version: u8,
    pub minor_version: u8,
}

impl BiosAssistantCapabilities {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            command_accepted: raw & 0x8000_0000 != 0,
            super_boost_supported: raw & 0x0000_0001 != 0,
            gpu_mode_support: ((raw & 0x0000_0006) >> 1) as u8,
            fn_ctrl_swap_supported: raw & 0x0000_0008 != 0,
            major_version: ((raw & 0x00f0_0000) >> 20) as u8,
            minor_version: ((raw & 0x000f_0000) >> 16) as u8,
        }
    }
}

/// Decoded `LENOVO_BIOS_ASSISTANT.GetValue` data.
///
/// `payload` removes only bit31. No function-specific meaning is assigned to
/// the remaining bits by this generic read API.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BiosAssistantValue {
    pub index_data: u32,
    pub raw: u32,
    pub command_accepted: bool,
    pub payload: u32,
}

impl BiosAssistantValue {
    #[must_use]
    pub const fn from_raw(index_data: u32, raw: u32) -> Self {
        Self {
            index_data,
            raw,
            command_accepted: raw & 0x8000_0000 != 0,
            payload: raw & 0x7fff_ffff,
        }
    }
}
