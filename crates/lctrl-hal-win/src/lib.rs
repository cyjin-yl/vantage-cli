#[cfg(windows)]
mod audio_endpoints;
mod battery_sources;
mod bios;
#[cfg(windows)]
mod bluetooth;
#[cfg(windows)]
mod brightness;
#[cfg(windows)]
mod camera_consent;
mod codec;
mod conflict;
mod cpu;
mod diagnostics;
#[cfg(windows)]
mod display;
#[cfg(windows)]
mod display_status;
#[cfg(windows)]
#[cfg(windows)]
mod dolby;
mod error;
mod event_log;
mod events;
mod firmware;
mod gamezone;
mod gpu_oc;
mod ioctl_contract;
#[cfg(windows)]
mod its_policy;
mod kbd_backlight;
mod local_package;
#[cfg(windows)]
mod local_updates;
#[cfg(windows)]
mod machine_fix;
#[cfg(windows)]
mod magicbay;
mod mobile_broadband;
mod msr;
#[cfg(windows)]
mod native_cpu;
mod native_battery_electrical;
#[cfg(windows)]
#[cfg(windows)]
mod native_ioctl;
#[cfg(windows)]
#[cfg(windows)]
mod native_performance;
#[cfg(windows)]
#[cfg(windows)]
mod native_power;
#[cfg(windows)]
#[cfg(windows)]
mod native_power_status;
#[cfg(windows)]
mod native_wmi;
#[cfg(windows)]
mod wide;
// The NDR descriptor generators are pure Rust with no platform types, so
// they stay visible to the Linux CI job that compiles this crate.
mod ndr;
#[cfg(windows)]
mod noise;
mod os_battery;
mod p0;
#[cfg(windows)]
#[cfg(windows)]
mod panel_features;
#[cfg(windows)]
mod panel_identity;
#[cfg(windows)]
mod performance_p0;
mod peripherals;
#[cfg(windows)]
mod power_overlay;
mod power_p0;
mod power_tuning;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod radio;
#[cfg(windows)]
mod runtime_devices;
#[cfg(windows)]
#[cfg(windows)]
mod sense;
#[cfg(windows)]
mod sense_rpc;
#[cfg(windows)]
mod sensor;
mod security;
mod storage;
mod system_inventory;
mod thermal;
mod touchpad_caps;
mod update_catalog;
mod update_history;
#[cfg(windows)]
mod wifi_security;
#[cfg(windows)]
mod wifi_status;
#[cfg(windows)]
mod windows_hal;
mod wmi_contract;

mod usb;
mod usb_port_decode;
#[cfg(windows)]
mod usb_ports;

#[cfg(windows)]
pub use audio_endpoints::{
    MmDeviceApi, NativeMmDeviceApi, WindowsAudioEndpoints,
};
pub use battery_sources::WindowsBatterySources;
#[cfg(windows)]
pub use bluetooth::WindowsBluetooth;
pub use bios::WindowsBiosController;
#[cfg(windows)]
pub use brightness::WindowsPanelBrightness;
pub use codec::{
    AdapterDetail, BatteryDetail83, EM_SPEC_VERSION_REQUEST, EM_SPEC_VERSION_STORAGE_80_MIN,
    GbmdCommand, GenericGet, IOCTL_BATTERY_CONFIG, IOCTL_BATTERY_DETAIL, IOCTL_EM_SPEC_VERSION,
    IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_NIGHT_CHARGE, IOCTL_TEMP_CHARGE,
    IOCTL_USB_CHARGER, NightChargeSelector, TempChargeSelector, UsbChargerSelector,
};
pub use conflict::WindowsControlConflictDetector;
pub use cpu::{
    CpuRegistryReader, IDENTIFIER_VALUE, PROCESSOR_NAME_VALUE, VENDOR_IDENTIFIER_VALUE,
    APPROXIMATE_CLOCK_VALUE, cpu_status,
};
pub use diagnostics::WindowsSystemInventory;
pub use firmware::WindowsFirmware;
#[cfg(windows)]
#[cfg(windows)]
pub use display::NativeDisplayApi;
#[cfg(windows)]
pub use display::{DisplayApi, WindowsPanelController};
#[cfg(windows)]
pub use display_status::{
    DisplayStateApi, MonitorColorStoreReader, NativeDisplayState, NativeMonitorColorStore,
    WindowsDisplayStatus,
};
#[cfg(windows)]
#[cfg(windows)]
pub use dolby::{DolbyRpc, NativeDolbyRpc, WindowsDolbyAudio};
pub use error::{map_win_error, map_wmi_hresult};
#[cfg(windows)]
pub use event_log::WindowsEventLog;
pub use events::DOCUMENTED_LENOVO_EVENT_CLASSES;
#[cfg(windows)]
pub use events::{NativeWmiEventReader, NativeWmiEventSource};
pub use gamezone::{FeatureId, GameZone, SupportProbe};
pub use gpu_oc::WindowsGpuOc;
pub use ioctl_contract::{EnergyDriver, IoctlTransport};
pub use kbd_backlight::{
    IOCTL_VPC_KBLC, KblcLayout, WindowsKeyboardBacklight, WindowsKeyboardSources,
    kblc_argument_writes, probe_vpc_keyboard,
};
#[cfg(windows)]
#[cfg(windows)]
pub use local_updates::NativeLocalDriverUpdateApi;
#[cfg(windows)]
pub use local_updates::{LocalDriverUpdateApi, WindowsLocalUpdateController};
#[cfg(windows)]
pub use machine_fix::WindowsMachineFix;
#[cfg(windows)]
#[cfg(windows)]
pub use magicbay::NativeMagicBay;
#[cfg(windows)]
pub use magicbay::{coalesce_magicbay_devices, parse_magicbay_instance_id};
pub use mobile_broadband::{MobileBroadbandProbe, WindowsMobileBroadband, probe_mobile_broadband};
#[cfg(windows)]
pub use msr::WIN_MSR_TRANSPORT;
pub use msr::{
    IOCTL_MSR_READ, IOCTL_MSR_WRITE, IOCTL_NOTIFY_PLATFORM, MsrSelector, PLATFORM_ID_21VG,
    PLATFORM_WHITELIST, WIN_MSR_DEVICE_PATH, WinMsrDev,
};
#[cfg(windows)]
pub use native_cpu::NativeCpuRegistry;
#[cfg(windows)]
#[cfg(windows)]
pub use native_ioctl::DeviceIoTransport;
#[cfg(windows)]
mod scene_policy;
#[cfg(windows)]
pub use scene_policy::NativeScenePolicy;
#[cfg(windows)]
mod osd;
#[cfg(windows)]
pub use osd::NativeOsd;
#[cfg(windows)]
mod fn_osd;
#[cfg(windows)]
pub use fn_osd::NativeFnOsd;
#[cfg(windows)]
mod hotkey;
#[cfg(windows)]
pub use hotkey::{NativeHotkey, take_last_hotkey};
pub use native_battery_electrical::NativeBatteryElectrical;
#[cfg(windows)]
#[cfg(windows)]
pub use native_ioctl::NativeIoctl;
#[cfg(windows)]
#[cfg(windows)]
pub use native_performance::NativePerformanceRegistry;
#[cfg(windows)]
#[cfg(windows)]
pub use native_power::NativePowerApi;
#[cfg(windows)]
#[cfg(windows)]
pub use native_power_status::NativeSystemPowerStatus;
#[cfg(windows)]
pub use native_wmi::NativeWmi;
#[cfg(windows)]
#[cfg(windows)]
pub use noise::{ElevocRpc, NativeElevocRpc, WindowsNoiseCancel};
pub use os_battery::{OsBatteryChannel, SystemPowerStatusReader, WindowsOsBattery};
pub use p0::{
    ChargeModeReader, GbmdChargeModeReader, UnverifiedChargeModeReader, WindowsBatteryP0,
};
#[cfg(windows)]
#[cfg(windows)]
pub use panel_features::{
    GammaRampApi, IntelControlLib, IntelCtlApi, NativeGammaRamp, PixtxBlock, PixtxCapability,
    RampSamples, SuperResolutionCaps, WindowsPanelFeatures, classify_lut, eye_care_lut,
    identity_lut,
};
#[cfg(windows)]
pub use panel_identity::{
    NativePanelRegistry, PanelRegistry, WindowsPanelIdentity,
};
#[cfg(windows)]
pub use performance_p0::{PerformanceRegistryReader, WindowsPerformanceP0, read_effective_mode};
#[cfg(windows)]
pub use its_policy::{
    CURVE_SLOTS, POWER_BUCKET_SLOTS, read_policy, record_its_policy_capability,
};
pub use peripherals::WindowsPeripheralController;
#[cfg(windows)]
pub use power_overlay::{
    NativePowerOverlay, OverlayGuid, PowerOverlayControl, mode_for_overlay, overlay_for_mode,
};
pub use power_p0::{PowerApi, WindowsPowerP0};
pub use power_tuning::{
    PROCESSOR_BOOST_SETTING, PROCESSOR_EPP_SETTING, PROCESSOR_SETTINGS_SUBGROUP,
    epp_percent_from_raw, epp_raw_from_percent,
};
#[cfg(windows)]
#[cfg(windows)]
pub use process::NativeProcessApi;
#[cfg(windows)]
pub use process::{ProcessApi, ProcessReadbackPolicy, WindowsProcessController};
#[cfg(windows)]
pub use radio::NativeRadio;
#[cfg(windows)]
pub use runtime_devices::{
    AudioEndpointFormFactor, PnpDeviceProblem, PnpDeviceStatus, RuntimeDevice, RuntimeDeviceApi,
    RuntimeDeviceClass, RuntimeDeviceKind, WindowsRuntimeDevices, select_runtime_device,
    select_runtime_devices,
};
#[cfg(windows)]
#[cfg(windows)]
pub use runtime_devices::{NativeRuntimeDeviceApi, NativeWindowsRuntimeDevices};
#[cfg(windows)]
#[cfg(windows)]
pub use sense::{
    HpdSettingsStore, NativeHpdStore, NativeSensePipe, SensePipe, WindowsSenseControl,
};
#[cfg(windows)]
pub use sense_rpc::{HpdRpc, NativeHpdRpc};
#[cfg(windows)]
pub use sensor::WindowsSensors;
pub use security::{FirmwareEnvironment, FirmwareVariableState, WindowsSecurity, observe_security};
#[cfg(windows)]
pub use security::NativeFirmwareEnvironment;
pub use local_package::{
    AdmittedPackage, LocalPackageApi, LogFile, PackageBinding, RunOutcome, ScriptContract,
    WindowsLocalPackageInstaller, parse_script_contract, udc_package_from_dir,
};
#[cfg(windows)]
pub use local_package::NativeLocalPackageApi;
pub use system_inventory::WindowsInventory;
pub use storage::{DiskGeometryFacts, StorageDisk, StorageProbeApi, StorageQuery, WindowsStorage};
#[cfg(windows)]
pub use storage::{NativeStorageProbe, probe_storage};
pub use thermal::WindowsThermal;
pub use touchpad_caps::{
    ButtonCap as HidButtonCap, CapabilityReport as HidCapabilityReport,
    TouchpadDescriptor as HidTouchpadDescriptor, ValueCap as HidValueCap,
    derive_capabilities as derive_touchpad_capabilities,
    read_capabilities as read_touchpad_capabilities,
};
pub use usb::WindowsUsbPower;
pub use usb_port_decode::{
    CONNECTION_INFORMATION_EX_FIXED_BYTES, DESCRIPTOR_REQUEST_HEADER_BYTES,
    DeviceConfigurationFacts, HUB_NODE_INFORMATION_BYTES, HubNodeFacts, PortConnectionFacts,
    PortDeviceDescriptor, decode_config_descriptor, decode_hub_node_information,
    decode_port_connection_information, format_usb_revision, max_power_ma,
};
#[cfg(windows)]
pub use usb_ports::{WindowsUsbPorts, record_usb_port_capabilities};
#[cfg(windows)]
pub use wifi_security::{NetshCommandRunner, WindowsWifiSecurity, WlanNetshRunner};
#[cfg(windows)]
pub use wifi_status::WindowsWifiStatus;
#[cfg(windows)]
pub use windows_hal::{WindowsHal, record_fan_capabilities};
pub use wmi_contract::{
    WmiInstance, WmiMethodResult, WmiObject, WmiTransport, WmiValue, active_instance,
};
#[cfg(windows)]
pub type NativeWindowsHal = WindowsHal<NativeWmi, NativeIoctl>;
