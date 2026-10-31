mod battery;
mod audio;
mod bios;
mod capability;
mod command_capability;
mod conflict;
mod diagnostics;
mod events;
mod hotkey;
mod machine_fix;
mod magicbay;
mod network;
mod osd;
mod performance;
mod peripherals;
mod power;
mod process;
mod scene_policy;
mod sense;
mod setting;
mod update;
mod verify;

pub use audio::{AudioControl, NoiseCancelControl};
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
pub use hotkey::{HotkeyControl, HotkeyOutcome};
pub use machine_fix::MachineFixControl;
pub use magicbay::MagicBayControl;
pub use network::{
    MAX_WIFI_HISTORY_ENTRIES, WifiConnectionState, WifiHistoryPersistence, WifiHistoryPolicy,
    WifiHistorySnapshot, WifiInterfaceSnapshot, WifiLiveNetwork, WifiSavedProfile,
    WifiSecurityAssessment, WifiSecurityControl, WifiSecurityFinding, WifiSecurityReason,
    WifiSecurityReport, assess_wifi_security,
};
pub use osd::{OsdControl, OsdOutcome};
pub use performance::{
    DbdcControl, FanControl, PerformanceControl, PowerLimitKind, TemperatureControl, TuningControl,
};
pub use peripherals::{
    KeyboardControl, PanelBrightnessControl, PanelControl, PrivacyControl, TouchpadControl,
};
pub use power::{PowerControl, ProcessorPowerControl, UsbControl};
pub use process::ProcessControl;
pub use scene_policy::ScenePolicyControl;
pub use sense::{SenseControl, SenseFeature};
pub use setting::{Setting, apply_setting};
pub use update::LocalUpdateControl;
pub use verify::poll_readback;
