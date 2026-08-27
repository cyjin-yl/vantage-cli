mod codec;
mod error;
mod ioctl_contract;
#[cfg(windows)]
mod native_ioctl;
#[cfg(windows)]
mod native_performance;
#[cfg(windows)]
mod native_power;
#[cfg(windows)]
mod native_wmi;
mod p0;
mod performance_p0;
mod power_p0;
mod windows_hal;
mod wmi_contract;

pub use codec::{
    AdapterDetail, BatteryDetail83, GbmdCommand, GenericGet, GenericSet, IOCTL_BATTERY_CONFIG,
    IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_GENERIC_GET_VARIANT,
    IOCTL_GENERIC_SET,
};
pub use error::{map_win_error, map_wmi_hresult};
pub use ioctl_contract::{EnergyDriver, IoctlTransport};
#[cfg(windows)]
pub use native_ioctl::NativeIoctl;
#[cfg(windows)]
pub use native_performance::NativePerformanceRegistry;
#[cfg(windows)]
pub use native_power::NativePowerApi;
#[cfg(windows)]
pub use native_wmi::NativeWmi;
pub use p0::{ChargeModeReader, UnverifiedChargeModeReader, WindowsBatteryP0};
pub use performance_p0::{PerformanceRegistryReader, WindowsPerformanceP0};
pub use power_p0::{PowerApi, WindowsPowerP0};
pub use windows_hal::WindowsHal;
pub use wmi_contract::{
    WmiInstance, WmiMethodResult, WmiObject, WmiTransport, WmiValue, active_instance,
};
#[cfg(windows)]
pub type NativeWindowsHal = WindowsHal<NativeWmi, NativeIoctl>;
