use std::collections::BTreeMap;

use crate::error::{LctrlError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Linux,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Limited,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Capability {
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CapabilitySet {
    pub platform: Platform,
    pub features: BTreeMap<String, Capability>,
}

impl CapabilitySet {
    #[must_use]
    pub fn new(platform: Platform) -> Self {
        Self {
            platform,
            features: BTreeMap::new(),
        }
    }

    pub fn record(
        &mut self,
        feature_id: &str,
        availability: Availability,
        detail: Option<String>,
    ) -> Result<Option<Capability>> {
        if feature_id.trim().is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "feature id must not be empty or whitespace".into(),
            });
        }

        let previous = self.features.insert(
            feature_id.to_string(),
            Capability {
                availability,
                detail,
            },
        );

        Ok(previous)
    }

    #[must_use]
    pub fn get(&self, feature_id: &str) -> Option<&Capability> {
        self.features.get(feature_id)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct HardwareInfo {
    pub product_name: Option<String>,
    pub family: Option<String>,
    pub bios_version: Option<String>,
}
