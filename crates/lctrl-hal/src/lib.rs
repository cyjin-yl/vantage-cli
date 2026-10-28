mod battery;
mod bios;
mod capability;
mod command_capability;
mod conflict;
mod diagnostics;
mod events;
mod magicbay;
mod network;
mod performance;
mod peripherals;
mod power;
mod process;
mod setting;
mod update;
mod verify;

pub use battery::{BatteryControl, BatteryElectricalSource};
pub use bios::{BiosAssistantControl, BiosControl, apply_bios_transaction, plan_bios_apply};
pub use capability::Hal;
pub use command_capability::{
    CommandCapability, DispatchSupport, FeatureBinding, HardwareObservation, ServiceBinding,
    ServiceRole, SwitchAction, command_capabilities, command_capability, command_capability_for,
    command_registry, minimal_dependency_checklist,
};
pub use conflict::ControlConflictDetection;
pub use diagnostics::{DiagnosticsControl, UpdateControl};
pub use events::{EventCancellation, EventSource, EventValue, RawEvent, RawEventReader};
pub use magicbay::MagicBayControl;
pub use network::{
    MAX_WIFI_HISTORY_ENTRIES, WifiConnectionState, WifiHistoryPersistence, WifiHistoryPolicy,
    WifiHistorySnapshot, WifiInterfaceSnapshot, WifiLiveNetwork, WifiSavedProfile,
    WifiSecurityAssessment, WifiSecurityControl, WifiSecurityFinding, WifiSecurityReason,
    WifiSecurityReport, assess_wifi_security,
};
pub use performance::{
    FanControl, PerformanceControl, PowerLimitKind, TemperatureControl, TuningControl,
};
pub use peripherals::{KeyboardControl, PanelControl, PrivacyControl, TouchpadControl};
pub use power::{PowerControl, ProcessorPowerControl};
pub use process::ProcessControl;
pub use setting::{Setting, apply_setting};
pub use update::LocalUpdateControl;
pub use verify::poll_readback;
