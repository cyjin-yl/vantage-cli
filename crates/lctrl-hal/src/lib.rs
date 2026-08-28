mod battery;
mod bios;
mod capability;
mod performance;
mod peripherals;
mod power;
mod setting;

pub use battery::BatteryControl;
pub use bios::BiosControl;
pub use capability::Hal;
pub use performance::{FanControl, PerformanceControl};
pub use peripherals::{KeyboardControl, PanelControl};
pub use power::PowerControl;
pub use setting::{Setting, apply_setting};
