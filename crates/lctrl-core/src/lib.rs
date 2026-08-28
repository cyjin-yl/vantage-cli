mod audio;
mod battery;
mod bios;
mod capability;
mod change;
mod diagnostics;
mod error;
mod keyboard;
mod magicbay;
mod panel;
mod performance;
mod power;
mod privacy;
mod sensing;

pub use audio::{DolbyProfile, NoiseCancellationMode};
pub use battery::{
    AdapterAuthentication, AdapterDetailValues, AdapterInfo, BatteryDate, BatteryHealth,
    BatteryTelemetry, ChargeMode, ChargeModeActual, ChargePrimitive, ChargeStatus,
    decode_charge_mode, plan_charge_mode,
};
pub use bios::{
    BiosChange, BiosItem, BiosName, BiosPasswordStatus, BiosRisk, BiosValue, classify_risk,
    is_success, parse_current_setting, parse_selections, save_parameter,
};
pub use capability::{Availability, Capability, CapabilitySet, HardwareInfo, Platform};
pub use change::{ApplyMode, ChangeReport};
pub use diagnostics::{DiagnosticKind, DiagnosticOutcome, DiagnosticResult, UpdateCapability};
pub use error::{ErrorReport, LctrlError, Result};
pub use keyboard::{BacklightState, DeviceState, LightingEffect, LockState};
pub use magicbay::{
    KNOWN_MAGICBAY_DEVICES, KnownMagicBayDevice, MAGICBAY_VENDOR_ID, MagicBayDevice, MagicBayKind,
    identify_magicbay,
};
pub use panel::{
    GamutMode, LowLatencyMode, PanelDisplayMode, PanelRefreshCapability, PanelSupportBits,
    RefreshMode,
};
pub use performance::{
    DispatcherVersion, FanDescriptor, FanId, FanMode, FanStep, FanTable, PerformanceCapabilities,
    PerformanceMode, PerformanceState, SensorId, TemperatureLocation, TemperatureSensor,
    TemperatureSensorMetadata, TemperatureSource,
};
pub use power::{
    PowerGuid, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey, PowerSettingValue,
    PowerSource, PowerValueRange, validate_power_write,
};
pub use privacy::{PrivacyDevice, PrivacyLayer, PrivacyState};
pub use sensing::{LeaveAction, PresenceDistance, SenseGlobal, SenseMode};
