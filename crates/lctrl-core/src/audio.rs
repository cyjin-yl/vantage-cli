use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DolbyProfile {
    Movie,
    Music,
    Game,
    Voice,
    Personalize,
    Dynamic,
    Off,
}

impl DolbyProfile {
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Movie => 0,
            Self::Music => 1,
            Self::Game => 2,
            Self::Voice => 3,
            Self::Personalize => 4,
            Self::Dynamic => 5,
            Self::Off => 6,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Movie),
            1 => Some(Self::Music),
            2 => Some(Self::Game),
            3 => Some(Self::Voice),
            4 => Some(Self::Personalize),
            5 => Some(Self::Dynamic),
            6 => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseCancellationMode {
    Off,
    Shared,
    Single,
    Spatial,
    VoiceId,
    FarField,
    Unknown(u8),
}

impl NoiseCancellationMode {
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Off,
            1 => Self::Shared,
            2 => Self::Single,
            3 => Self::Spatial,
            4 => Self::VoiceId,
            10 => Self::FarField,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Shared => 1,
            Self::Single => 2,
            Self::Spatial => 3,
            Self::VoiceId => 4,
            Self::FarField => 10,
            Self::Unknown(raw) => raw,
        }
    }
}
