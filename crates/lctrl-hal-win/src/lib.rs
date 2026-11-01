mod battery_sources;
mod bios;
#[cfg(windows)]
mod brightness;
mod codec;
mod conflict;
mod diagnostics;
#[cfg(windows)]
mod display;
#[cfg(windows)]
#[cfg(windows)]
mod dolby;
mod error;
mod events;
mod gamezone;
mod ioctl_contract;
#[cfg(windows)]
mod local_updates;
#[cfg(windows)]
mod machine_fix;
#[cfg(windows)]
mod magicbay;
mod msr;
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
mod performance_p0;
mod peripherals;
#[cfg(windows)]
mod power_overlay;
mod power_p0;
mod power_tuning;
#[cfg(windows)]
mod process;
#[cfg(windows)]
mod runtime_devices;
#[cfg(windows)]
#[cfg(windows)]
mod sense;
mod system_inventory;
mod thermal;
#[cfg(windows)]
mod wifi_security;
#[cfg(windows)]
mod windows_hal;
mod wmi_contract;

mod usb;

pub use battery_sources::WindowsBatterySources;
pub use bios::WindowsBiosController;
#[cfg(windows)]
pub use brightness::WindowsPanelBrightness;
pub use codec::{
    AdapterDetail, BatteryDetail83, GbmdCommand, GenericGet, IOCTL_BATTERY_CONFIG,
    IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_NIGHT_CHARGE,
    IOCTL_TEMP_CHARGE, IOCTL_USB_CHARGER, NightChargeSelector, TempChargeSelector,
    UsbChargerSelector,
};
pub use conflict::WindowsControlConflictDetector;
pub use diagnostics::WindowsSystemInventory;
#[cfg(windows)]
#[cfg(windows)]
pub use display::NativeDisplayApi;
#[cfg(windows)]
pub use display::{DisplayApi, WindowsPanelController};
#[cfg(windows)]
#[cfg(windows)]
pub use dolby::{DolbyRpc, NativeDolbyRpc, WindowsDolbyAudio};
pub use error::{map_win_error, map_wmi_hresult};
pub use events::DOCUMENTED_LENOVO_EVENT_CLASSES;
#[cfg(windows)]
pub use events::{NativeWmiEventReader, NativeWmiEventSource};
pub use gamezone::{FeatureId, GameZone, SupportProbe};
pub use ioctl_contract::{EnergyDriver, IoctlTransport};
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
#[cfg(windows)]
pub use msr::WIN_MSR_TRANSPORT;
pub use msr::{
    IOCTL_MSR_READ, IOCTL_MSR_WRITE, IOCTL_NOTIFY_PLATFORM, MsrSelector, PLATFORM_ID_21VG,
    PLATFORM_WHITELIST, WIN_MSR_DEVICE_PATH, WinMsrDev,
};
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
    IntelControlLib, IntelCtlApi, WindowsPanelFeatures, classify_lut, eye_care_lut, identity_lut,
};
#[cfg(windows)]
pub use performance_p0::{PerformanceRegistryReader, WindowsPerformanceP0, read_effective_mode};
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
pub use system_inventory::WindowsInventory;
pub use thermal::WindowsThermal;
pub use usb::WindowsUsbPower;
#[cfg(windows)]
pub use wifi_security::{NetshCommandRunner, WindowsWifiSecurity, WlanNetshRunner};
#[cfg(windows)]
pub use windows_hal::WindowsHal;
pub use wmi_contract::{
    WmiInstance, WmiMethodResult, WmiObject, WmiTransport, WmiValue, active_instance,
};
#[cfg(windows)]
pub type NativeWindowsHal = WindowsHal<NativeWmi, NativeIoctl>;
