use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyDevice {
    Camera,
    Microphone,
    Fingerprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLayer {
    Runtime,
    Persistent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PrivacyState {
    pub device: PrivacyDevice,
    pub runtime: Option<bool>,
    pub persistent: Option<bool>,
}

impl PrivacyState {
    #[must_use]
    pub const fn layers_disagree(&self) -> bool {
        match (self.runtime, self.persistent) {
            (Some(rt), Some(pers)) => rt != pers,
            _ => false,
        }
    }
}
