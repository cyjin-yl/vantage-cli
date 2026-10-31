use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result};
use lctrl_hal::{BatteryElectricalSource, Hal};

use crate::{
    EnergyDriver, GameZone, IoctlTransport, WmiObject, WmiTransport, WmiValue, active_instance,
    power_overlay::NativePowerOverlay, power_overlay::PowerOverlayControl,
    wmi_contract::ROOT_WMI,
};
#[cfg(windows)]
use crate::{WIN_MSR_TRANSPORT, WinMsrDev};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";

#[derive(Debug)]
pub struct WindowsHal<W, I, E = crate::NativeBatteryElectrical> {
    wmi: W,
    ioctl: I,
    electrical: E,
}

impl<W, I> WindowsHal<W, I> {
    #[must_use]
    pub const fn new(wmi: W, ioctl: I) -> Self {
        Self {
            wmi,
            ioctl,
            electrical: crate::NativeBatteryElectrical,
        }
    }
}

impl<W, I, E> WindowsHal<W, I, E> {
    #[must_use]
    pub const fn with_electrical(wmi: W, ioctl: I, electrical: E) -> Self {
        Self {
            wmi,
            ioctl,
            electrical,
        }
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

impl<W, I, E> Hal for WindowsHal<W, I, E>
where
    W: WmiTransport,
    I: IoctlTransport,
    E: BatteryElectricalSource,
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
        // Performance mode is driven through the OS power overlay scheme, not
        // through any vendor service. Only the three schemes Windows defines
        // are portable, so the feature is reported as limited rather than
        // fully available.
        match NativePowerOverlay.effective() {
            Ok(_) => capabilities.record(
                "perf.mode",
                Availability::Limited,
                Some(
                    "Windows power overlay covers balanced, quiet and performance; vendor-only \
                     modes (geek, silent-high-performance, custom) have no OS equivalent"
                        .into(),
                ),
            )?,
            Err(error) => capabilities.record(
                "perf.mode",
                Availability::Unavailable,
                Some(error.to_string()),
            )?,
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
            if energy_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            if energy_available {
                None
            } else {
                Some("EnergyDrv is not available for GBMD charge-mode readback".into())
            },
        )?;
        let usb_charging_available = energy_available
            && EnergyDriver::new(&self.ioctl)
                .usb_charger_flags()
                .is_ok();
        capabilities.record(
            "usb.charging",
            if usb_charging_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!usb_charging_available)
                .then(|| "EnergyDrv USB charger query (0x831020e8/0x02) is unavailable".into()),
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
        let mut thermal_source_failed = false;
        let thermal_zone_error = match self.wmi.query(
            ROOT_WMI,
            "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
        ) {
            Ok(objects) if objects.is_empty() => {
                Some("ACPI exposed no readable thermal-zone temperature".to_owned())
            }
            Ok(objects) => {
                let error = objects.iter().find_map(|object| {
                    crate::thermal::required_u32(object, "CurrentTemperature").err()
                });
                thermal_source_failed = error.is_some();
                error.map(|error| error.to_string())
            }
            Err(error) => {
                thermal_source_failed =
                    !matches!(error, lctrl_core::LctrlError::Unsupported { .. });
                Some(error.to_string())
            }
        };
        let mut battery_source_failed = false;
        let battery_temperature_error = match self.electrical.read_electrical() {
            Ok(observation)
                if observation.source_issues.is_empty()
                    && observation.temperature_deci_kelvin.is_some() =>
            {
                None
            }
            Ok(observation) => Some(if observation.source_issues.is_empty() {
                "Windows battery class exposed no temperature".to_owned()
            } else {
                battery_source_failed = true;
                observation
                    .source_issues
                    .iter()
                    .map(|issue| format!("{}: {}", issue.source, issue.detail))
                    .collect::<Vec<_>>()
                    .join("; ")
            }),
            Err(error) => {
                battery_source_failed =
                    !matches!(error, lctrl_core::LctrlError::Unsupported { .. });
                Some(error.to_string())
            }
        };
        capabilities.record(
            "perf.temp",
            if !thermal_source_failed
                && !battery_source_failed
                && (thermal_zone_error.is_none() || battery_temperature_error.is_none())
            {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            match (thermal_zone_error, battery_temperature_error) {
                (None, None) => None,
                (Some(reason), None) => Some(format!(
                    "Windows battery class temperature readable; ACPI: {reason}"
                )),
                (None, Some(reason)) => Some(format!(
                    "ACPI thermal zones readable; Windows battery class: {reason}"
                )),
                (Some(acpi), Some(battery)) => {
                    Some(format!("ACPI: {acpi}; Windows battery class: {battery}"))
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
        let gamezone_fan_mode = GameZone::new(&self.wmi).smart_fan_mode();
        capabilities.record(
            "perf.fan.mode",
            match &gamezone_fan_mode {
                Ok(_) => Availability::Available,
                Err(_) => Availability::Unavailable,
            },
            match &gamezone_fan_mode {
                Ok(raw) => Some(format!("GAMEZONE GetSmartFanMode readable (raw={raw})")),
                Err(error) => Some(format!("GAMEZONE fan mode rejected: {error}")),
            },
        )?;
        capabilities.record(
            "power.scheme",
            Availability::Limited,
            Some("Power API readiness is probed by the power service at command time".into()),
        )?;
        capabilities.record(
            "power.scheme.setting.read",
            Availability::Limited,
            Some(
                "selected setting and group-policy availability are checked at command time".into(),
            ),
        )?;
        for (feature, availability, detail) in [
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
        #[cfg(windows)]
        {
            let msr = WinMsrDev::new(&WIN_MSR_TRANSPORT);
            match msr.probe() {
                Ok(()) => {
                    capabilities.record(
                        "tune.msr",
                        Availability::Available,
                        Some("WinMsrDev selector table enabled (SYSTEM context)".into()),
                    )?;
                }
                Err(error) => {
                    capabilities.record(
                        "tune.msr",
                        Availability::Unavailable,
                        Some(format!("WinMsrDev MSR channel unavailable: {error}")),
                    )?;
                }
            }
        }
        Ok(capabilities)
    }
}

impl<W, I, E> WindowsHal<W, I, E>
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
