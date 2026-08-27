mod battery;
mod capability;
mod change;
mod error;
mod performance;
mod power;

pub use battery::{
    AdapterAuthentication, AdapterDetailValues, AdapterInfo, BatteryDate, BatteryHealth,
    BatteryTelemetry, ChargeMode, ChargeModeActual, ChargePrimitive, ChargeStatus,
    decode_charge_mode, plan_charge_mode,
};
pub use capability::{Availability, Capability, CapabilitySet, HardwareInfo, Platform};
pub use change::{ApplyMode, ChangeReport};
pub use error::{ErrorReport, LctrlError, Result};
pub use performance::{
    DispatcherVersion, FanDescriptor, FanId, FanMode, FanStep, FanTable, PerformanceCapabilities,
    PerformanceMode, PerformanceState, SensorId, TemperatureLocation, TemperatureSensor,
    TemperatureSensorMetadata, TemperatureSource,
};
pub use power::{
    PowerGuid, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey, PowerSettingValue,
    PowerSource, PowerValueRange, validate_power_write,
};
