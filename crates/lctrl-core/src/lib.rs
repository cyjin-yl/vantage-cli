mod audio;
mod battery;
mod bios;
mod capability;
mod change;
mod command_capability;
mod cpu;
mod diagnostics;
mod display;
mod edid;
mod error;
mod firmware;
mod inventory;
mod keyboard;
mod machine_fix;
mod magicbay;
mod panel;
mod performance;
mod power;
mod privacy;
mod process;
mod scene_policy;
mod sensing;
mod sensor;
mod storage;
mod update;
mod usb;

pub use audio::{
    AudioDataFlow, AudioEndpoint, AudioEndpointState, DolbyProfile, DolbyState, EndpointVolume,
    NoiseCancelState, NoiseCancellationMode, percent_to_scalar, volume_percent_from_scalar,
    volume_readback_tolerance_percent,
};
pub use battery::{
    AcPowerObservation, AdapterAuthentication, AdapterConnectorType, AdapterDetailValues,
    AdapterInfo, AdapterStatus, BatteryDate, BatteryElectricalObservation, BatteryFieldProvenance,
    BatteryHealth, BatteryRateState, BatterySourceIssue, BatteryTelemetry, BatteryWearReport,
    ChargeMode, ChargeModeActual, ChargePrimitive, ChargeStatus, NightChargeState,
    OsBatterySnapshot, PowerDrawObservation, PowerFieldUnavailable, SystemPowerStatus,
    TemporaryChargeMode, TemporaryChargeState, UsbChargingState, decode_charge_mode,
    decode_gbmd_charge_mode, plan_charge_mode,
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
pub use cpu::{
    CpuClockCluster, CpuClockSource, CpuStatus, derive_current_clock_mhz,
};
pub use diagnostics::{
    CatalogPackage, DEFAULT_DIAGNOSTIC_ITEM_DEADLINE, DiagnosticCancellationReason,
    DiagnosticCancellationToken, DiagnosticKind, DiagnosticMeasurement, DiagnosticOutcome,
    DiagnosticReason, DiagnosticResult, DiagnosticRunOptions, DiagnosticRunState,
    DiagnosticStatusSnapshot, DiagnosticThreshold, DiagnosticThresholdComparison, DiagnosticValue,
    UpdateCapability, UpdateCheckReport, UpdateHistoryLedger, UpdateHistoryRecord,
    UpdateHistoryReport,
};
pub use display::{
    ActiveColorMode, ColorStateSource, DisplayColorEncoding, DisplayColorState, DisplayConnection,
    DisplayPathState, DisplayScope, DisplayScaling, DisplayScalingObservation, DisplaySession,
    HdrCapabilityObservation, MonitorColorState, MonitorColorStore, ScalingSource, SessionDesktop,
};
pub use edid::{
    PanelConnection, PanelEdidVersion, PanelIdentity, PanelInput, PanelManufacture,
    PanelHdrCapability, PanelPhysicalSize, PanelRangeLimits, PanelResolution,
    decode_panel_edid, decode_panel_hdr_capability,
};
pub use error::{ErrorReport, LctrlError, Result};
pub use firmware::{
    FirmwareAttempt, FirmwareIdentity, FirmwareResource, FirmwareStatus, FirmwareType,
    firmware_class, firmware_revision, format_firmware_version, last_attempt_status_meaning,
};
pub use inventory::{
    BiosInventory, CpuInventory, DiskInventory, FirmwareInventory, GpuInventory, InventoryDiff,
    InventoryField, InventoryModuleDiff, InventoryRecord, InventoryRecordDiff, InventorySnapshot,
    MemoryModuleInventory, MotherboardInventory, NetworkAdapterInventory, OperatingSystemInventory,
    ProductInventory, SystemIdentity,
};
pub use keyboard::{
    BacklightState, DeviceState, LightingEffect, LockState, TouchpadRecoveryLeaf,
    TouchpadAxis, TouchpadButtonType, TouchpadButtons, TouchpadCapabilities, TouchpadContacts,
    TouchpadDerivedSize, TouchpadIdentity, TouchpadRecoveryReport, TouchpadRecoveryStrategy,
    TouchpadReports, TouchpadSupportBits, TouchpadSurface, TouchpadUsage,
};
pub use machine_fix::{
    FixIssue, FixLedger, FixLedgerEntry, FixObservation, FixRepairKind, FixReport, FixRule,
};
pub use magicbay::{
    KNOWN_MAGICBAY_DEVICES, KnownMagicBayDevice, MAGICBAY_LTE_PIDS, MAGICBAY_VENDOR_ID,
    MagicBayDevice, MagicBayInventory, MagicBayKind, MagicBayLteInterface, MagicBayLteMediaState,
    MagicBayLteOperational, MagicBayLteStatus, coalesce_magicbay_devices, identify_magicbay,
    merge_magicbay_interfaces, parse_magicbay_device_id,
};
pub use panel::{
    EyeCareLevel, GamutMode, LowLatencyMode, PanelAdapterId, PanelBrightness, PanelDisplayMode,
    PanelMode, PanelRefreshCapability, PanelRefreshRequest, PanelRotation, PanelSupportBits,
    PanelTarget, RefreshMode,
};
pub use performance::{
    DbdcProfile, DispatcherVersion, FanDescriptor, FanId, FanMode, FanStep, FanTable,
    GpuOcParameter, GpuOcStatus, GpuOcSupportProbe, ItsApMinStdFan, ItsEppTurbo, ItsPlxSetMode,
    ItsPolicy, ItsPolicyIssue, ItsPowerSlider, ItsSlotTable, ItsTurboTime, ItsTurboValue,
    PerformanceCapabilities, PerformanceMode, PerformanceState, SensorId, TemperatureLocation,
    TemperatureSensor, TemperatureSensorMetadata, TemperatureSource,
};
pub use power::{
    PowerGuid, PowerMutation, PowerRangeObservation, PowerScheme, PowerSchemeId, PowerSettingKey,
    PowerSettingObservation, PowerSettingValue, PowerSource, PowerSourceValues, PowerValueRange,
    ProcessorBoostMode, validate_power_write,
};
pub use privacy::{
    CAMERA_CONSENT_AS_RECORDED, CAMERA_IN_USE_RULE, CameraAppPrivacyPolicy, CameraConsent,
    CameraConsentEntry, CameraConsentReport, CameraConsentScope, CameraConsentScopeReport,
    CameraPrivacyPolicy, CameraStamp, PrivacyDevice, PrivacyDeviceState, PrivacyLayer,
    PrivacyState, camera_in_use, decode_non_packaged_path, filetime_to_rfc3339,
    parse_camera_consent,
};
pub use process::{
    DEFAULT_PROCESS_TOP_LIMIT, MAX_PROCESS_TOP_LIMIT, ProcessAffinityPolicy, ProcessIdentity,
    ProcessInventory, ProcessInventoryEntry, ProcessPolicy, ProcessPriorityClass, ProcessSnapshot,
    ProcessState, ProcessTarget, ProcessWorkingSet, validate_process_top_limit,
};
pub use scene_policy::{
    SceneAction, ScenePolicyLedger, ScenePolicyLedgerEntry, ScenePolicyMatch, ScenePolicyReport,
    ScenePolicyRule,
};
pub use sensing::{SenseDistance, SenseGlobal, SenseMode, SenseState, SenseWait};
pub use sensor::{
    SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX_NAME, SensorDescriptor, SensorField, SensorFreshness,
    SensorGuid, SensorInventory, SensorReading, SensorState, SensorValue, category_name,
    data_field_name, format_utc_millis, match_sensor, sensor_type_name, unix_millis_from_filetime,
    unix_millis_from_utc,
};
pub use storage::{
    StorageBusType, StorageCriticalWarning, StorageDevice, StorageFieldProvenance, StorageHealth,
    StorageMediaType, StorageSourceIssue, StorageStatus, StorageTemperatureSensor,
};
pub use update::{
    DriverIdentity, DriverPackage, DriverRollbackReport, DriverSignature, DriverUpdateReport,
    DriverVersion, FIRMWARE_DEVICE_CLASS_GUID, IgnoredDriverRule, InstalledDriver,
    LOCAL_UPDATE_POLICY_SCHEMA, LocalDriverRollbackRequest, LocalDriverUpdateRequest,
    LocalPackageInstallReport, LocalPackageInstallRequest, LocalPackagePayload, LocalUpdatePolicy,
    LocalUpdatePolicyStore, MAX_LOCAL_PACKAGE_SCRIPT_BYTES, MAX_LOCAL_UPDATE_DEVICES,
    MAX_LOCAL_UPDATE_HISTORY, MAX_LOCAL_UPDATE_IGNORES, MAX_LOCAL_UPDATE_PACKAGE_FILES,
    MAX_LOCAL_UPDATE_POLICY_BYTES, MAX_WINDOWS_DEVICE_ID_UTF16, PresentPnpDevice, SignedDriverFile,
    UpdateHistoryEntry, UpdateHistoryOperation, UpdateSchedule, UpdateWeekday, is_firmware_class,
    is_firmware_update, validate_device_instance_id, validate_hardware_id, validate_local_directory,
    validate_local_path,
};
pub use usb::{
    PowerDeliveryReport, UsbController, UsbControllerKind, UsbHub, UsbPdController, UsbPort,
    UsbPortConnection, UsbPortDevice, UsbPortInventory, UsbPortPowerDelivery, UsbPortSpeed,
    UsbPowerProtocol,
};
