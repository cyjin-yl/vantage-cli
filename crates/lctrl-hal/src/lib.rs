mod battery;
mod bios;
mod capability;
mod diagnostics;
mod magicbay;
mod performance;
mod peripherals;
mod power;
mod setting;

pub use battery::BatteryControl;
pub use bios::BiosControl;
pub use capability::Hal;
pub use diagnostics::{DiagnosticsControl, UpdateControl};
pub use magicbay::MagicBayControl;
pub use performance::{FanControl, PerformanceControl};
pub use peripherals::{KeyboardControl, PanelControl};
pub use power::PowerControl;
pub use setting::{Setting, apply_setting};
