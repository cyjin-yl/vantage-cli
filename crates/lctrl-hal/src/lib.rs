mod battery;
mod bios;
mod capability;
mod conflict;
mod diagnostics;
mod magicbay;
mod performance;
mod peripherals;
mod power;
mod setting;
mod verify;

pub use battery::BatteryControl;
pub use bios::BiosControl;
pub use capability::Hal;
pub use conflict::ControlConflictDetection;
pub use diagnostics::{DiagnosticsControl, UpdateControl};
pub use magicbay::MagicBayControl;
pub use performance::{
    FanControl, PerformanceControl, PowerLimitKind, TemperatureControl, TuningControl,
};
pub use peripherals::{KeyboardControl, PanelControl, PrivacyControl, TouchpadControl};
pub use power::PowerControl;
pub use setting::{Setting, apply_setting};
pub use verify::poll_readback;
