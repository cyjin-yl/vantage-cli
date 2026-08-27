use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result};
use lctrl_hal::Hal;

use crate::{EnergyDriver, IoctlTransport, WmiObject, WmiTransport, WmiValue, active_instance};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";

#[derive(Debug)]
pub struct WindowsHal<W, I> {
    wmi: W,
    ioctl: I,
}

impl<W, I> WindowsHal<W, I> {
    #[must_use]
    pub const fn new(wmi: W, ioctl: I) -> Self {
        Self { wmi, ioctl }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn ioctl(&self) -> &I {
        &self.ioctl
    }
}

impl<W, I> Hal for WindowsHal<W, I>
where
    W: WmiTransport,
    I: IoctlTransport,
{
    fn platform(&self) -> Platform {
        Platform::Windows
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        let product =
            self.wmi_string_query("SELECT Name FROM Win32_ComputerSystemProduct", "Name")?;
        let system = self.wmi.query(
            ROOT_CIMV2,
            "SELECT Model, SystemFamily FROM Win32_ComputerSystem",
        )?;
        let bios = self
            .wmi
            .query(ROOT_CIMV2, "SELECT SMBIOSBIOSVersion FROM Win32_BIOS")?;

        Ok(HardwareInfo {
            product_name: product.or_else(|| first_string(&system, "Model")),
            family: first_string(&system, "SystemFamily"),
            bios_version: first_string(&bios, "SMBIOSBIOSVersion"),
        })
    }

    fn capabilities(&self) -> Result<CapabilitySet> {
        let mut capabilities = CapabilitySet::new(Platform::Windows);

        match active_instance(&self.wmi, "LENOVO_UTILITY_DATA") {
            Ok(_) => {
                capabilities.record("channel.wmi.root", Availability::Available, None)?;
            }
            Err(error) => {
                capabilities.record(
                    "channel.wmi.root",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
            }
        }

        match EnergyDriver::new(&self.ioctl).gbmd_status() {
            Ok(_) => {
                capabilities.record("channel.energy_driver", Availability::Available, None)?;
            }
            Err(error) => {
                capabilities.record(
                    "channel.energy_driver",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
            }
        }

        capabilities.record(
            "battery.thresholds",
            Availability::Unavailable,
            Some("target firmware has no verified arbitrary-threshold write channel".into()),
        )?;
        capabilities.record(
            "tune.windows_rapl",
            Availability::Unavailable,
            Some("VBS/HVCI blocks ring-0 MSR and DPTF continuously owns RAPL".into()),
        )?;
        capabilities.record(
            "wmi.gamezone.methods",
            Availability::Unavailable,
            Some("21VG firmware returns Invalid object for the GAMEZONE method family".into()),
        )?;

        Ok(capabilities)
    }
}

impl<W, I> WindowsHal<W, I>
where
    W: WmiTransport,
{
    fn wmi_string_query(&self, wql: &str, key: &str) -> Result<Option<String>> {
        let objects = self.wmi.query(ROOT_CIMV2, wql)?;
        Ok(first_string(&objects, key))
    }
}

fn first_string(objects: &[WmiObject], key: &str) -> Option<String> {
    objects.iter().find_map(|object| match object.get(key) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    })
}
