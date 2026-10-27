mod audio;
mod battery;
mod bios;
mod capability;
mod change;
mod command_capability;
mod diagnostics;
mod error;
mod inventory;
mod keyboard;
mod magicbay;
mod panel;
mod performance;
mod power;
mod privacy;
mod process;
mod sensing;
mod update;

pub use audio::{DolbyProfile, NoiseCancellationMode};
pub use battery::{
    AdapterAuthentication, AdapterConnectorType, AdapterDetailValues, AdapterInfo, AdapterStatus,
    BatteryDate, BatteryHealth, BatteryTelemetry, ChargeMode, ChargeModeActual, ChargePrimitive,
    ChargeStatus, OsBatterySnapshot, SystemPowerStatus, decode_charge_mode, plan_charge_mode,
};
pub use bios::{
    BiosAppliedChange, BiosApplyEffect, BiosApplyPlan, BiosApplyResult, BiosAssistantCapabilities,
    BiosAssistantValue, BiosChange, BiosItem, BiosName, BiosPasswordStatus, BiosPlannedChange,
    BiosRisk, BiosValue, classify_risk, is_success, parse_current_setting, parse_selections,
    save_parameter,
};
pub use capability::{Availability, Capability, CapabilitySet, HardwareInfo, Platform};
pub use change::{ApplyMode, ChangeReport};
pub use command_capability::{
    CommandAxes, CommandDependency, CommandId, CommandSelector, Dependency, DependencyUse,
    HardwarePresence, HardwareRequirement, Readback, Rollback, StateLayer, optional, required,
};
pub use diagnostics::{
    DEFAULT_DIAGNOSTIC_ITEM_DEADLINE, DiagnosticCancellationReason, DiagnosticCancellationToken,
    DiagnosticKind, DiagnosticMeasurement, DiagnosticOutcome, DiagnosticReason, DiagnosticResult,
    DiagnosticRunOptions, DiagnosticRunState, DiagnosticStatusSnapshot, DiagnosticThreshold,
    DiagnosticThresholdComparison, DiagnosticValue, UpdateCapability,
};
pub use error::{ErrorReport, LctrlError, Result};
pub use inventory::{
    BiosInventory, CpuInventory, DiskInventory, FirmwareInventory, GpuInventory, InventoryDiff,
    InventoryField, InventoryModuleDiff, InventoryRecord, InventoryRecordDiff, InventorySnapshot,
    MemoryModuleInventory, MotherboardInventory, NetworkAdapterInventory, OperatingSystemInventory,
    ProductInventory,
};
pub use keyboard::{BacklightState, DeviceState, LightingEffect, LockState};
pub use magicbay::{
    KNOWN_MAGICBAY_DEVICES, KnownMagicBayDevice, MAGICBAY_VENDOR_ID, MagicBayDevice,
    MagicBayInventory, MagicBayKind, identify_magicbay,
};
pub use panel::{
    GamutMode, LowLatencyMode, PanelAdapterId, PanelDisplayMode, PanelMode, PanelRefreshCapability,
    PanelRefreshRequest, PanelRotation, PanelSupportBits, PanelTarget, RefreshMode,
};
pub use performance::{
    DispatcherVersion, FanDescriptor, FanId, FanMode, FanStep, FanTable, PerformanceCapabilities,
    PerformanceMode, PerformanceState, SensorId, TemperatureLocation, TemperatureSensor,
    TemperatureSensorMetadata, TemperatureSource,
};
pub use power::{
    PowerGuid, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey, PowerSettingValue,
    PowerSource, PowerSourceValues, PowerValueRange, ProcessorBoostMode, validate_power_write,
};
pub use privacy::{PrivacyDevice, PrivacyLayer, PrivacyState};
pub use process::{
    DEFAULT_PROCESS_TOP_LIMIT, MAX_PROCESS_TOP_LIMIT, ProcessAffinityPolicy, ProcessIdentity,
    ProcessInventory, ProcessInventoryEntry, ProcessPolicy, ProcessPriorityClass, ProcessSnapshot,
    ProcessState, ProcessTarget, ProcessWorkingSet, validate_process_top_limit,
};
pub use sensing::{LeaveAction, PresenceDistance, SenseGlobal, SenseMode};
pub use update::{
    DriverIdentity, DriverPackage, DriverRollbackReport, DriverSignature, DriverUpdateReport,
    DriverVersion, FIRMWARE_DEVICE_CLASS_GUID, IgnoredDriverRule, InstalledDriver,
    LOCAL_UPDATE_POLICY_SCHEMA, LocalDriverRollbackRequest, LocalDriverUpdateRequest,
    LocalUpdatePolicy, LocalUpdatePolicyStore, MAX_LOCAL_UPDATE_DEVICES, MAX_LOCAL_UPDATE_HISTORY,
    MAX_LOCAL_UPDATE_IGNORES, MAX_LOCAL_UPDATE_PACKAGE_FILES, MAX_LOCAL_UPDATE_POLICY_BYTES,
    MAX_WINDOWS_DEVICE_ID_UTF16, PresentPnpDevice, SignedDriverFile, UpdateHistoryEntry,
    UpdateHistoryOperation, UpdateSchedule, UpdateWeekday, is_firmware_class,
    validate_device_instance_id, validate_hardware_id, validate_local_path,
};
