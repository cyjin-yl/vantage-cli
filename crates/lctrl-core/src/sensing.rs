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

/// Presence distance selector shared by approach and leave
/// (`SET_APPROACH_DISTANCE`/`SET_LEAVE_DISTANCE`, pipe enum 0/1/2).
/// Replaces the earlier speculative 15/30/50/80 cm mapping, which had no
/// binary evidence (docs/RE-interfaces.md §13).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseDistance {
    Near,
    Middle,
    Far,
}

impl SenseDistance {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Near => 0,
            Self::Middle => 1,
            Self::Far => 2,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Near),
            1 => Some(Self::Middle),
            2 => Some(Self::Far),
            _ => None,
        }
    }
}

/// Presence-leave wait selector (`SET_LEAVE_WAIT`, pipe enum 0/1/2).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SenseWait {
    Short,
    Middle,
    Long,
}

impl SenseWait {
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Short => 0,
            Self::Middle => 1,
            Self::Long => 2,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Short),
            1 => Some(Self::Middle),
            2 => Some(Self::Long),
            _ => None,
        }
    }
}

/// Snapshot of the SmartSense HPD settings store
/// (`HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`,
/// all REG_DWORD; docs/RE-interfaces.md §13).
///
/// Fields are `Option` because the key may be absent when the SmartSense
/// service has never run; absence is reported, never synthesized. Two
/// encodings exist on the wire: legacy booleans are 0/1, the newer
/// camera/video commands use 1=enable/2=disable — both are normalized here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SenseState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_on_leave: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leave_distance: Option<SenseDistance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leave_wait: Option<SenseWait>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wake_on_approach: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_distance: Option<SenseDistance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_alert: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_tracking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensor_type: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facial_feature_registered: Option<bool>,
}

impl std::fmt::Display for SenseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lock-on-leave={} wake-on-approach={} pause-video={} attention-tracking={}",
            self.lock_on_leave
                .map_or("unknown", |v| if v { "on" } else { "off" }),
            self.wake_on_approach
                .map_or("unknown", |v| if v { "on" } else { "off" }),
            self.pause_video
                .map_or("unknown", |v| if v { "on" } else { "off" }),
            self.attention_tracking
                .map_or("unknown", |v| if v { "on" } else { "off" }),
        )
    }
}
