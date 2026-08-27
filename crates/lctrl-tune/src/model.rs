use lctrl_core::{ApplyMode, ChargeMode, LctrlError, Result};
use serde::Serialize;
use std::borrow::Borrow;
use std::fmt;

/// The only profile document version understood by this crate.
pub const PROFILE_SCHEMA_V1: u32 = 1;

/// Where a profile came from. Later origins replace an equal-name profile
/// from an earlier origin before inheritance is resolved.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileOrigin {
    Builtin,
    System,
    User,
}

impl ProfileOrigin {
    #[must_use]
    pub const fn precedence(self) -> u8 {
        match self {
            Self::Builtin => 0,
            Self::System => 1,
            Self::User => 2,
        }
    }
}

/// A profile name with the basic checks required by the DSL.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProfileName(String);

impl ProfileName {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(invalid("profile name must not be empty or whitespace"));
        }
        if name.contains('\0') {
            return Err(invalid("profile name must not contain NUL"));
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProfileName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl Borrow<str> for ProfileName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// EC/firmware performance mode from the tuning DSL.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EcMode {
    Smart,
    PowerSave,
    Beast,
}

impl EcMode {
    pub fn from_raw(raw: i64) -> Result<Self> {
        match raw {
            0 => Ok(Self::Smart),
            1 => Ok(Self::PowerSave),
            2 => Ok(Self::Beast),
            other => Err(invalid(format!("ec_mode must be 0, 1, or 2; got {other}"))),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Smart => 0,
            Self::PowerSave => 1,
            Self::Beast => 2,
        }
    }
}

impl fmt::Display for EcMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Smart => "smart",
            Self::PowerSave => "power-save",
            Self::Beast => "beast",
        })
    }
}

/// HWP energy-performance preference. Presets remain semantic; Raw is the
/// documented 0..=255 DSL value and is not silently clamped for any backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Epp {
    Performance,
    BalancePerformance,
    BalancePower,
    Power,
    Raw(u8),
}

impl Epp {
    pub fn from_text(text: &str) -> Result<Self> {
        match text {
            "performance" => Ok(Self::Performance),
            "balance-performance" => Ok(Self::BalancePerformance),
            "balance-power" => Ok(Self::BalancePower),
            "power" => Ok(Self::Power),
            other => Err(invalid(format!(
                "unknown epp preset {other:?}; expected performance, balance-performance, balance-power, power, or an integer 0..=255"
            ))),
        }
    }

    pub fn from_raw(raw: i64) -> Result<Self> {
        u8::try_from(raw).map(Self::Raw).map_err(|_| {
            invalid(format!(
                "epp raw value must be between 0 and 255; got {raw}"
            ))
        })
    }

    #[must_use]
    pub const fn raw(self) -> Option<u8> {
        match self {
            Self::Performance => Some(0),
            Self::BalancePerformance => Some(128),
            Self::BalancePower => Some(192),
            Self::Power => Some(255),
            Self::Raw(raw) => Some(raw),
        }
    }
}

impl fmt::Display for Epp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Performance => formatter.write_str("performance"),
            Self::BalancePerformance => formatter.write_str("balance-performance"),
            Self::BalancePower => formatter.write_str("balance-power"),
            Self::Power => formatter.write_str("power"),
            Self::Raw(raw) => raw.fmt(formatter),
        }
    }
}

/// Semantic fan request. `Performance` is intentionally part of the profile
/// DSL even though the older main-config enum omitted it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Auto,
    Manual,
    Fullspeed,
    Smart,
    Off,
    Performance,
}

impl fmt::Display for FanMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
            Self::Fullspeed => "fullspeed",
            Self::Smart => "smart",
            Self::Off => "off",
            Self::Performance => "performance",
        })
    }
}

/// Panel refresh request. Adaptive is the VRR/automatic semantic mode; a
/// fixed refresh is represented explicitly in hertz.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum PanelRefresh {
    Hz(u16),
    Adaptive,
}

impl PanelRefresh {
    pub fn hz(raw: i64) -> Result<Self> {
        let hz = u16::try_from(raw)
            .map_err(|_| invalid(format!("panel_hz must be a positive u16; got {raw}")))?;
        if hz == 0 {
            return Err(invalid("panel_hz must be positive"));
        }
        Ok(Self::Hz(hz))
    }
}

impl fmt::Display for PanelRefresh {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hz(hz) => write!(formatter, "{hz}"),
            Self::Adaptive => formatter.write_str("adaptive"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DgpuMode {
    IgpPriority,
}

impl DgpuMode {
    pub fn from_text(text: &str) -> Result<Self> {
        match text {
            "igp-priority" => Ok(Self::IgpPriority),
            other => Err(invalid(format!(
                "unknown dgpu mode {other:?}; expected igp-priority"
            ))),
        }
    }
}

impl fmt::Display for DgpuMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("igp-priority")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundPriority {
    Low,
    Normal,
    High,
}

impl BackgroundPriority {
    pub fn from_text(text: &str) -> Result<Self> {
        match text {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            other => Err(invalid(format!(
                "unknown background priority {other:?}; expected low, normal, or high"
            ))),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct BackgroundGoal {
    pub workset_trim: Option<bool>,
    pub priority: Option<BackgroundPriority>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Goal {
    pub ec_mode: Option<EcMode>,
    pub pl1_w: Option<u32>,
    pub pl2_w: Option<u32>,
    pub tau_s: Option<u32>,
    pub epp: Option<Epp>,
    pub turbo: Option<bool>,
    pub fan_mode: Option<FanMode>,
    pub panel_hz: Option<PanelRefresh>,
    pub dgpu: Option<DgpuMode>,
    pub charge_mode: Option<ChargeMode>,
    pub backlight: Option<u8>,
    pub background: Option<BackgroundGoal>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Constraints {
    pub temp_max_c: Option<u32>,
    pub fan_rpm_max: Option<u32>,
    pub battery_only: Option<bool>,
    pub min_dwell_s: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum FallbackTarget {
    Profile(ProfileName),
    RestoreFactory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictAction {
    RestoreFactory,
    GiveUp,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Fallback {
    pub on_temp_exceed: Option<FallbackTarget>,
    pub on_conflict: Option<ConflictAction>,
    pub max_reapply: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TimeRange {
    pub from_minutes: u16,
    pub to_minutes: u16,
}

impl TimeRange {
    pub fn parse(from: &str, to: &str) -> Result<Self> {
        let from_minutes = parse_clock(from)?;
        let to_minutes = parse_clock(to)?;
        if from_minutes == to_minutes {
            return Err(invalid("time_range from and to must differ"));
        }
        Ok(Self {
            from_minutes,
            to_minutes,
        })
    }

    #[must_use]
    pub fn contains(&self, minute: u16) -> bool {
        if self.from_minutes < self.to_minutes {
            (self.from_minutes..self.to_minutes).contains(&minute)
        } else {
            minute >= self.from_minutes || minute < self.to_minutes
        }
    }
}

fn parse_clock(text: &str) -> Result<u16> {
    let (hour, minute) = text
        .split_once(':')
        .ok_or_else(|| invalid(format!("invalid time {text:?}; expected HH:MM")))?;
    if hour.len() != 2 || minute.len() != 2 {
        return Err(invalid(format!("invalid time {text:?}; expected HH:MM")));
    }
    let hour = hour
        .parse::<u16>()
        .map_err(|_| invalid(format!("invalid time {text:?}; expected HH:MM")))?;
    let minute = minute
        .parse::<u16>()
        .map_err(|_| invalid(format!("invalid time {text:?}; expected HH:MM")))?;
    if hour >= 24 || minute >= 60 {
        return Err(invalid(format!("invalid time {text:?}; expected HH:MM")));
    }
    Ok(hour * 60 + minute)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum Trigger {
    OnAc,
    OnBattery,
    ProcessMatch {
        names: Vec<String>,
        paths: Vec<String>,
    },
    TempAbove {
        celsius: u32,
        hysteresis: u32,
    },
    TempBelow {
        celsius: u32,
        hysteresis: u32,
    },
    PowerAbove {
        watts: u32,
        hysteresis: u32,
    },
    PowerBelow {
        watts: u32,
        hysteresis: u32,
    },
    TimeRange(TimeRange),
    BatteryBelow {
        percent: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TriggerClass {
    Resident,
    Time,
    PowerSource,
    Sensor,
    Process,
}

impl Trigger {
    #[must_use]
    pub const fn class(&self) -> TriggerClass {
        match self {
            Self::ProcessMatch { .. } => TriggerClass::Process,
            Self::TempAbove { .. }
            | Self::TempBelow { .. }
            | Self::PowerAbove { .. }
            | Self::PowerBelow { .. } => TriggerClass::Sensor,
            Self::OnAc | Self::OnBattery | Self::BatteryBelow { .. } => TriggerClass::PowerSource,
            Self::TimeRange(_) => TriggerClass::Time,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProfileMetadata {
    pub name: ProfileName,
    pub description: Option<String>,
    pub priority: i32,
    pub inherits: Option<ProfileName>,
    #[serde(skip)]
    pub(crate) priority_explicit: bool,
    #[serde(skip)]
    pub(crate) description_explicit: bool,
}

impl ProfileMetadata {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        Ok(Self {
            name: ProfileName::new(name)?,
            description: None,
            priority: 0,
            inherits: None,
            priority_explicit: false,
            description_explicit: false,
        })
    }
}

/// A parsed schema-v1 profile. Optional goal/table fields remain patches until
/// a `ProfileCatalog` resolves inheritance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProfileDocument {
    pub schema: u32,
    pub profile: ProfileMetadata,
    pub goal: Goal,
    pub triggers: Option<Vec<Trigger>>,
    pub constraints: Option<Constraints>,
    pub fallback: Option<Fallback>,
    pub origin: ProfileOrigin,
}

impl ProfileDocument {
    pub fn new(name: impl Into<String>, origin: ProfileOrigin) -> Result<Self> {
        Ok(Self {
            schema: PROFILE_SCHEMA_V1,
            profile: ProfileMetadata::new(name)?,
            goal: Goal::default(),
            triggers: None,
            constraints: None,
            fallback: None,
            origin,
        })
    }
}

/// Fully inherited and validated profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedProfile {
    pub name: ProfileName,
    pub description: Option<String>,
    pub priority: i32,
    pub goal: Goal,
    pub triggers: Vec<Trigger>,
    pub constraints: Constraints,
    pub fallback: Fallback,
    pub origin: ProfileOrigin,
}

impl ResolvedProfile {
    #[must_use]
    pub fn trigger_class(&self) -> TriggerClass {
        self.triggers
            .iter()
            .map(Trigger::class)
            .max()
            .unwrap_or(TriggerClass::Resident)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum TuningTarget {
    EcMode,
    Pl1,
    Pl2,
    Tau,
    Epp,
    Turbo,
    FanMode,
    PanelRefresh,
    Dgpu,
    ChargeMode,
    Backlight,
    Background,
}

impl TuningTarget {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EcMode => "ec_mode",
            Self::Pl1 => "pl1_w",
            Self::Pl2 => "pl2_w",
            Self::Tau => "tau_s",
            Self::Epp => "epp",
            Self::Turbo => "turbo",
            Self::FanMode => "fan_mode",
            Self::PanelRefresh => "panel_hz",
            Self::Dgpu => "dgpu",
            Self::ChargeMode => "charge_mode",
            Self::Backlight => "backlight",
            Self::Background => "background",
        }
    }
}

impl fmt::Display for TuningTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum TuneSetting {
    EcMode(EcMode),
    Pl1(u32),
    Pl2(u32),
    Tau(u32),
    Epp(Epp),
    Turbo(bool),
    FanMode(FanMode),
    PanelRefresh(PanelRefresh),
    Dgpu(DgpuMode),
    ChargeMode(ChargeMode),
    Backlight(u8),
    Background(BackgroundGoal),
}

impl TuneSetting {
    #[must_use]
    pub const fn target(&self) -> TuningTarget {
        match self {
            Self::EcMode(_) => TuningTarget::EcMode,
            Self::Pl1(_) => TuningTarget::Pl1,
            Self::Pl2(_) => TuningTarget::Pl2,
            Self::Tau(_) => TuningTarget::Tau,
            Self::Epp(_) => TuningTarget::Epp,
            Self::Turbo(_) => TuningTarget::Turbo,
            Self::FanMode(_) => TuningTarget::FanMode,
            Self::PanelRefresh(_) => TuningTarget::PanelRefresh,
            Self::Dgpu(_) => TuningTarget::Dgpu,
            Self::ChargeMode(_) => TuningTarget::ChargeMode,
            Self::Backlight(_) => TuningTarget::Backlight,
            Self::Background(_) => TuningTarget::Background,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnavailableTarget {
    pub target: TuningTarget,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TunePlan {
    pub profile: ProfileName,
    pub mode: ApplyMode,
    pub snapshot_targets: Vec<TuningTarget>,
    pub writes: Vec<TuneSetting>,
    pub skipped: Vec<UnavailableTarget>,
    pub warnings: Vec<String>,
}

pub(crate) fn invalid(detail: impl Into<String>) -> LctrlError {
    LctrlError::InvalidArgument {
        detail: detail.into(),
    }
}
