use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result};
use lctrl_hal::Hal;

use crate::{
    EnergyDriver, IoctlTransport, WmiObject, WmiTransport, WmiValue, active_instance,
    wmi_contract::ROOT_WMI,
};

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

        let staging_isolated = self.wmi.bios_staging_isolation_verified();
        capabilities.record(
            "bios.staging.owned",
            if staging_isolated {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!staging_isolated)
                .then(|| "BIOS staging-buffer ownership and isolation are unverified".into()),
        )?;

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

        let battery_detail_error = if energy_available {
            match EnergyDriver::new(&self.ioctl).battery_detail(0) {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            }
        } else {
            Some("EnergyDrv is unavailable for battery telemetry".into())
        };
        let battery_telemetry_available = battery_detail_error.is_none();
        let os_battery_error = match self.wmi.query(ROOT_WMI, "SELECT * FROM BatteryStatus") {
            Ok(objects) if !objects.is_empty() => None,
            Ok(_) => Some("the Windows battery class exposed no instance".into()),
            Err(error) => Some(error.to_string()),
        };
        let os_battery_available = os_battery_error.is_none();
        let inventory_error = match self
            .wmi
            .query(ROOT_CIMV2, "SELECT Model FROM Win32_ComputerSystem")
        {
            Ok(objects) if !objects.is_empty() => None,
            Ok(_) => Some("Win32_ComputerSystem returned no inventory records".into()),
            Err(error) => Some(error.to_string()),
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
        let telemetry_detail = battery_detail_error.clone();
        for feature in ["battery.info", "battery.status"] {
            capabilities.record(
                feature,
                if battery_telemetry_available || os_battery_available {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                match (battery_telemetry_available, os_battery_available) {
                    (true, true) => None,
                    (true, false) => telemetry_detail.clone(),
                    (false, true) => None,
                    (false, false) => telemetry_detail.clone(),
                },
            )?;
        }
        capabilities.record(
            "battery.adapter",
            if energy_available || os_battery_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            match (energy_available, os_battery_available) {
                (true, _) => None,
                (false, true) => None,
                (false, false) => {
                    Some("neither EnergyDrv nor the Windows battery class is available".into())
                }
            },
        )?;
        capabilities.record(
            "battery.charge_mode",
            Availability::Unavailable,
            Some(
                "GBMD conservation/express writes are accepted, but 21VG firmware exposes no \
                 independent charge-mode readback (docs/C-acceptance.md C2)"
                    .into(),
            ),
        )?;
        capabilities.record(
            "diagnostics.inventory",
            if inventory_error.is_none() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            inventory_error,
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
        match self.wmi.query(
            ROOT_WMI,
            "SELECT Mode FROM LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA WHERE Active = TRUE",
        ) {
            Ok(objects) if !objects.is_empty() => {
                capabilities.record(
                    "panel.refresh",
                    Availability::Available,
                    Some(
                        "verified 60/120 Hz internal-panel transitions; requires the interactive \
                         desktop session (docs/C-acceptance.md C1)"
                            .into(),
                    ),
                )?;
            }
            Ok(_) => {
                capabilities.record(
                    "panel.refresh",
                    Availability::Unavailable,
                    Some("firmware exposed no active internal-panel refresh record".into()),
                )?;
            }
            Err(error) => {
                capabilities.record(
                    "panel.refresh",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
            }
        }
        let thermal_zone_available = matches!(
            self.wmi
                .query(ROOT_WMI, "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature"),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "perf.temp",
            if thermal_zone_available || battery_telemetry_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            match (thermal_zone_available, battery_telemetry_available) {
                (true, true) => None,
                (true, false) => {
                    Some("ACPI thermal zones only; the battery channel is unavailable".into())
                }
                (false, true) => {
                    Some("battery temperature only; no ACPI thermal zone is present".into())
                }
                (false, false) => {
                    Some("no ACPI thermal zone and no battery temperature channel".into())
                }
            },
        )?;
        let fan_inventory_available = matches!(
            self.wmi.query(
                ROOT_WMI,
                "SELECT NumOfFans, FanId, FanMinSpeed, FanMaxSpeed FROM LENOVO_FAN_TEST_DATA",
            ),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "perf.fan.inventory",
            if fan_inventory_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            if fan_inventory_available {
                None
            } else {
                Some("firmware exposed no fan inventory record".into())
            },
        )?;
        capabilities.record(
            "power.scheme",
            Availability::Limited,
            Some("Power API readiness is probed by the power service at command time".into()),
        )?;
        for (feature, availability, detail) in [
            (
                "perf.mode",
                Availability::Unavailable,
                "target performance-mode write path is unavailable",
            ),
            (
                "perf.fan.mode",
                Availability::Unavailable,
                "no verified fan-mode write channel on 21VG firmware",
            ),
            (
                "tune.pl1",
                Availability::Unavailable,
                "Windows raw MSR/RAPL writes are unavailable",
            ),
            (
                "tune.pl2",
                Availability::Unavailable,
                "Windows raw MSR/RAPL writes are unavailable",
            ),
            (
                "tune.tau",
                Availability::Unavailable,
                "Windows raw MSR/RAPL writes are unavailable",
            ),
            (
                "tune.epp",
                Availability::Limited,
                "PERFEPP policy is probed at command time",
            ),
            (
                "tune.turbo",
                Availability::Limited,
                "PERFBOOSTMODE policy is probed at command time",
            ),
            (
                "gpu.mode",
                Availability::Unavailable,
                "no verified target GPU-mode mutator",
            ),
            (
                "tune.background",
                Availability::Unavailable,
                "no process-policy tuning executor",
            ),
        ] {
            capabilities.record(feature, availability, Some(detail.into()))?;
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
