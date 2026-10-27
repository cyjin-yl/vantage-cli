mod battery_fallback;
mod bios;
mod codec;
mod conflict;
mod diagnostics;
mod display;
mod events;
mod error;
mod ioctl_contract;
mod local_updates;
mod magicbay;
#[cfg(windows)]
mod native_ioctl;
#[cfg(windows)]
mod native_performance;
#[cfg(windows)]
mod native_power;
#[cfg(windows)]
mod native_power_status;
#[cfg(windows)]
mod native_wmi;
mod os_battery;
mod p0;
mod performance_p0;
mod process;
mod runtime_devices;
mod system_inventory;
mod peripherals;
mod power_p0;
mod power_tuning;
mod thermal;
mod windows_hal;
mod wmi_contract;
mod wifi_security;

pub use battery_fallback::WindowsBatteryFallback;
pub use bios::WindowsBiosController;
pub use codec::{
    AdapterDetail, BatteryDetail83, GbmdCommand, GenericGet, IOCTL_BATTERY_CONFIG,
    IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_GENERIC_GET_VARIANT,
};
pub use conflict::WindowsControlConflictDetector;
pub use diagnostics::WindowsSystemInventory;
pub use events::DOCUMENTED_LENOVO_EVENT_CLASSES;
#[cfg(windows)]
pub use events::{NativeWmiEventReader, NativeWmiEventSource};
#[cfg(windows)]
pub use display::NativeDisplayApi;
pub use display::{DisplayApi, WindowsPanelController};
pub use error::{map_win_error, map_wmi_hresult};
pub use ioctl_contract::{EnergyDriver, IoctlTransport};
pub use local_updates::{LocalDriverUpdateApi, WindowsLocalUpdateController};
#[cfg(windows)]
pub use magicbay::NativeMagicBay;
pub use magicbay::{coalesce_magicbay_devices, parse_magicbay_instance_id};
#[cfg(windows)]
pub use native_ioctl::NativeIoctl;
#[cfg(windows)]
pub use local_updates::NativeLocalDriverUpdateApi;
pub use native_performance::NativePerformanceRegistry;
#[cfg(windows)]
pub use native_power::NativePowerApi;
#[cfg(windows)]
pub use native_power_status::NativeSystemPowerStatus;
#[cfg(windows)]
pub use native_wmi::NativeWmi;
pub use os_battery::{OsBatteryChannel, SystemPowerStatusReader, WindowsOsBattery};
pub use p0::{ChargeModeReader, UnverifiedChargeModeReader, WindowsBatteryP0};
pub use performance_p0::{PerformanceRegistryReader, WindowsPerformanceP0};
pub use process::{ProcessApi, ProcessReadbackPolicy, WindowsProcessController};
#[cfg(windows)]
pub use process::NativeProcessApi;
pub use runtime_devices::{
    AudioEndpointFormFactor, PnpDeviceProblem, PnpDeviceStatus, RuntimeDevice, RuntimeDeviceApi,
    RuntimeDeviceClass, RuntimeDeviceKind, WindowsRuntimeDevices, select_runtime_device,
};
#[cfg(windows)]
pub use runtime_devices::{NativeRuntimeDeviceApi, NativeWindowsRuntimeDevices};
pub use system_inventory::WindowsInventory;
pub use peripherals::WindowsPeripheralController;
pub use power_p0::{PowerApi, WindowsPowerP0};
pub use power_tuning::{
    PROCESSOR_BOOST_SETTING, PROCESSOR_EPP_SETTING, PROCESSOR_SETTINGS_SUBGROUP,
    epp_percent_from_raw, epp_raw_from_percent,
};
pub use thermal::WindowsThermal;
pub use windows_hal::WindowsHal;
pub use wifi_security::{NetshCommandRunner, WindowsWifiSecurity, WlanNetshRunner};
pub use wmi_contract::{
    WmiInstance, WmiMethodResult, WmiObject, WmiTransport, WmiValue, active_instance,
};
#[cfg(windows)]
pub type NativeWindowsHal = WindowsHal<NativeWmi, NativeIoctl>;
