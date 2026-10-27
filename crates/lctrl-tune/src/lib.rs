//! Pure schema-v1 tuning profiles and capability-aware plan compilation.
//!
//! This crate owns no OS channels, filesystem lookup, process enumeration, or
//! writes. It parses/validates semantic goals and compiles them only against a
//! caller-provided `CapabilitySet`.

mod catalog;
mod model;
mod parser;
mod plan;

pub use catalog::ProfileCatalog;
pub use lctrl_core::ChargeMode;
pub use model::{
    BackgroundGoal, BackgroundPriority, ConflictAction, Constraints, DgpuMode, EcMode, Epp,
    Fallback, FallbackTarget, FanMode, Goal, PROFILE_SCHEMA_V1, PanelRefresh, ProfileDocument,
    ProfileMetadata, ProfileName, ProfileOrigin, ResolvedProfile, TimeRange, Trigger, TriggerClass,
    TunePlan, TuneSetting, TuningTarget, UnavailableTarget,
};
pub use parser::parse_profile_toml;
pub use plan::Planner;
