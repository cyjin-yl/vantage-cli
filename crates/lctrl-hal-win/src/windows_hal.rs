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

        let wmi_available = match active_instance(&self.wmi, "LENOVO_UTILITY_DATA") {
            Ok(_) => {
                capabilities.record("channel.wmi.root", Availability::Available, None)?;
                true
            }
            Err(error) => {
                capabilities.record(
                    "channel.wmi.root",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
                false
            }
        };
        let energy_available = match EnergyDriver::new(&self.ioctl).gbmd_status() {
            Ok(_) => {
                capabilities.record("channel.energy_driver", Availability::Available, None)?;
                true
            }
            Err(error) => {
                capabilities.record(
                    "channel.energy_driver",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
                false
            }
        };

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

        match active_instance(&self.wmi, "LENOVO_LIGHTING_DATA") {
            Ok(_) => {
                capabilities.record("kbd.backlight", Availability::Available, None)?;
            }
            Err(error) => {
                capabilities.record(
                    "kbd.backlight",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
            }
        }
        let battery_availability = if energy_available {
            Availability::Available
        } else {
            Availability::Unavailable
        };
        let battery_detail =
            (!energy_available).then(|| "EnergyDrv is unavailable for battery telemetry".into());
        for feature in ["battery.info", "battery.status", "battery.adapter"] {
            capabilities.record(feature, battery_availability, battery_detail.clone())?;
        }
        capabilities.record(
            "battery.charge_mode",
            Availability::Unavailable,
            Some("target has no independently verified charge-mode readback channel".into()),
        )?;
        capabilities.record(
            "diagnostics.inventory",
            if wmi_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!wmi_available).then(|| "root WMI inventory channel is unavailable".into()),
        )?;
        capabilities.record(
            "magicbay.inventory",
            Availability::Limited,
            Some("SetupAPI inventory is provided by the separate MagicBay service".into()),
        )?;
        for feature in [
            "privacy.camera",
            "privacy.microphone",
            "privacy.fingerprint",
        ] {
            capabilities.record(
                feature,
                if wmi_available {
                    Availability::Limited
                } else {
                    Availability::Unavailable
                },
                Some("persistent BIOS privacy service is required for writes".into()),
            )?;
        }
        capabilities.record(
            "panel.refresh",
            if wmi_available {
                Availability::Limited
            } else {
                Availability::Unavailable
            },
            Some("panel rate metadata is readable; verified mode writer is unavailable".into()),
        )?;
        capabilities.record(
            "perf.temp",
            Availability::Unavailable,
            Some("GAMEZONE temperature methods are not implemented by target firmware".into()),
        )?;
        capabilities.record(
            "power.scheme",
            Availability::Available,
            Some("Windows Power API service is attached by the composition root".into()),
        )?;
        for (feature, detail) in [
            (
                "perf.mode",
                "target performance-mode write path is unavailable",
            ),
            (
                "perf.fan.mode",
                "target GAMEZONE fan methods are unavailable",
            ),
            ("tune.pl1", "Windows raw MSR/RAPL writes are unavailable"),
            ("tune.pl2", "Windows raw MSR/RAPL writes are unavailable"),
            ("tune.tau", "Windows raw MSR/RAPL writes are unavailable"),
            ("tune.epp", "no verified Windows EPP mutator"),
            ("tune.turbo", "no verified Windows turbo mutator"),
            ("gpu.mode", "no verified target GPU-mode mutator"),
            ("tune.background", "no process-policy tuning executor"),
        ] {
            capabilities.record(feature, Availability::Unavailable, Some(detail.into()))?;
        }
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
