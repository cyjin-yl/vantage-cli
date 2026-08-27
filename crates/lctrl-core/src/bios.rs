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
}

impl BiosPasswordStatus {
    #[must_use]
    pub const fn from_raw(min_length: u32, max_length: u32, password_state: u32) -> Self {
        Self {
            min_length,
            max_length,
            password_state,
            supervisor_set: password_state & 0x2 != 0,
        }
    }
}
