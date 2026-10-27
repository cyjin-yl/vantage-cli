use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseGlobal {
    Disabled,
    Enabled,
}

impl SenseGlobal {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Disabled => 2,
            Self::Enabled => 3,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            2 => Some(Self::Disabled),
            3 => Some(Self::Enabled),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseMode {
    Browsing,
    FaceDown,
    Walking,
}

impl SenseMode {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Browsing => 2,
            Self::FaceDown => 3,
            Self::Walking => 5,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            2 => Some(Self::Browsing),
            3 => Some(Self::FaceDown),
            5 => Some(Self::Walking),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceDistance {
    Cm15,
    Cm30,
    Cm50,
    Cm80,
}

impl PresenceDistance {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Cm15 => 0,
            Self::Cm30 => 1,
            Self::Cm50 => 2,
            Self::Cm80 => 3,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Cm15),
            1 => Some(Self::Cm30),
            2 => Some(Self::Cm50),
            3 => Some(Self::Cm80),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaveAction {
    LockAndPause,
    LockOnly,
    Prompt,
}

impl LeaveAction {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::LockAndPause => 0,
            Self::LockOnly => 1,
            Self::Prompt => 2,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::LockAndPause),
            1 => Some(Self::LockOnly),
            2 => Some(Self::Prompt),
            _ => None,
        }
    }
}
