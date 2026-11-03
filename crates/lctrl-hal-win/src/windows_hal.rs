use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result};
use lctrl_hal::{BatteryElectricalSource, Hal, TouchpadControl};

use crate::HpdRpc;
use crate::{
    EnergyDriver, GameZone, IoctlTransport, WmiObject, WmiTransport, WmiValue, active_instance,
    power_overlay::NativePowerOverlay, power_overlay::PowerOverlayControl, wmi_contract::ROOT_WMI,
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

        let staging_verified = self.wmi.bios_staging_contract_verified();
        capabilities.record(
            "bios.staging.contract",
            if staging_verified {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!staging_verified)
                .then(|| "BIOS staging-buffer ownership contract is unverified".into()),
        )?;

        // BIOS settings service role: the persistent settings channel is the
        // LENOVO WMI BIOS surface (enumeration through Lenovo_BiosSetting,
        // writes through the staging buffer when the contract is verified).
        let bios_settings_ok = matches!(
            self.wmi
                .query(ROOT_WMI, "SELECT CurrentSetting FROM Lenovo_BiosSetting"),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "channel.bios",
            if bios_settings_ok && staging_verified {
                Availability::Available
            } else if bios_settings_ok {
                Availability::Limited
            } else {
                Availability::Unavailable
            },
            if bios_settings_ok && staging_verified {
                Some("Lenovo_BiosSetting readable; staging-buffer writes contract-verified".into())
            } else if bios_settings_ok {
                Some("Lenovo_BiosSetting readable; staging-buffer writes unverified".into())
            } else {
                Some("Lenovo_BiosSetting exposed no settings".into())
            },
        )?;
        let bios_feature = if bios_settings_ok {
            Availability::Available
        } else {
            Availability::Unavailable
        };
        for feature in ["bios", "bios.discard", "kbd.fnlock", "kbd.fn-ctrl-swap"] {
            capabilities.record(
                feature,
                bios_feature,
                (!bios_settings_ok)
                    .then(|| "Lenovo_BiosSetting enumeration returned no settings".into()),
            )?;
        }
        let bios_password_ok = matches!(
            self.wmi.query(
                ROOT_WMI,
                "SELECT PasswordState FROM Lenovo_BiosPasswordSettings",
            ),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "bios.password.status",
            if bios_password_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!bios_password_ok).then(|| "Lenovo_BiosPasswordSettings returned no instance".into()),
        )?;
        for (feature, class) in [
            ("bios.defaults", "Lenovo_LoadDefaultSettings"),
            ("bios.password.verify", "Lenovo_SetBiosPassword"),
        ] {
            let present = active_instance(&self.wmi, class).is_ok();
            capabilities.record(
                feature,
                if present {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                (!present).then(|| format!("{class} has no active instance")),
            )?;
        }

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
        let perf_mode_probe = NativePowerOverlay.effective();
        match &perf_mode_probe {
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
        let usb_charging_available =
            energy_available && EnergyDriver::new(&self.ioctl).usb_charger_flags().is_ok();
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
        let night_charge_available =
            energy_available && EnergyDriver::new(&self.ioctl).night_charge_probe().is_ok();
        capabilities.record(
            "battery.night-charge",
            if night_charge_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!night_charge_available)
                .then(|| "EnergyDrv night-charge probe (0x83102150/0x13) is unavailable".into()),
        )?;
        let temporary_charge_available = energy_available
            && EnergyDriver::new(&self.ioctl)
                .temporary_charge_status()
                .is_ok();
        capabilities.record(
            "battery.temporary-mode",
            if temporary_charge_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!temporary_charge_available).then(|| {
                "EnergyDrv temporary-charge probe (0x83102148/0x11) is unavailable".into()
            }),
        )?;
        // Battery service role: present when either telemetry path reads.
        capabilities.record(
            "channel.battery",
            if battery_telemetry_available || os_battery_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!battery_telemetry_available && !os_battery_available).then(|| {
                "neither EnergyDrv nor the Windows battery class produced telemetry".into()
            }),
        )?;
        capabilities.record(
            "channel.usb",
            if usb_charging_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!usb_charging_available).then(|| "EnergyDrv USB charger query is unavailable".into()),
        )?;
        capabilities.record(
            "diagnostics.inventory",
            if inventory_error.is_none() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            inventory_error.clone(),
        )?;
        capabilities.record(
            "channel.diagnostics",
            if inventory_error.is_none() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            inventory_error.clone(),
        )?;
        capabilities.record(
            "magicbay.inventory",
            Availability::Limited,
            Some("SetupAPI inventory is provided by the separate MagicBay service".into()),
        )?;
        capabilities.record(
            "channel.magicbay",
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
        // Runtime privacy channel: probe the internal camera and
        // microphone PnP groups. A successful state read proves the whole
        // PnP enable/disable write path, so the service role and the
        // per-device runtime features can report availability honestly.
        #[cfg(windows)]
        {
            let runtime = crate::NativeWindowsRuntimeDevices::new(crate::NativeRuntimeDeviceApi);
            let camera = runtime.camera_group_state();
            let microphone = runtime.microphone_state();
            let camera_detail = camera.as_ref().err().map(|error| error.to_string());
            let microphone_detail = microphone.as_ref().err().map(|error| error.to_string());
            let runtime_ok = camera.is_ok() && microphone.is_ok();
            capabilities.record(
                "channel.privacy-runtime",
                if runtime_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                (!runtime_ok).then(|| {
                    format!(
                        "runtime privacy probe failed (camera: {}, microphone: {})",
                        camera_detail.clone().unwrap_or_else(|| "ok".into()),
                        microphone_detail.clone().unwrap_or_else(|| "ok".into())
                    )
                }),
            )?;
            for (feature, detail) in [
                ("privacy.camera.runtime", camera_detail),
                ("privacy.microphone.runtime", microphone_detail),
            ] {
                capabilities.record(
                    feature,
                    if detail.is_none() {
                        Availability::Available
                    } else {
                        Availability::Unavailable
                    },
                    detail,
                )?;
            }
            // Touchpad PnP group: a successful state read proves the
            // enable/disable write path for the input leaves.
            let touchpad = runtime.touchpad_state();
            let touchpad_detail = touchpad.as_ref().err().map(|error| error.to_string());
            for feature in [
                "channel.touchpad",
                "touchpad",
                "touchpad.status",
                "touchpad.recover",
            ] {
                capabilities.record(
                    feature,
                    if touchpad.is_ok() {
                        Availability::Available
                    } else {
                        Availability::Unavailable
                    },
                    touchpad_detail.clone(),
                )?;
            }
        }
        // HPD settings channel: a live read of op-5 capability + op-6
        // settings proves the RPC endpoint, the NDR contract and the
        // registry readback all work. Recorded names match the registry
        // FeatureBinding strings so `capability` reports availability
        // instead of blanket "unverified" for implemented sense commands.
        let hpd = match crate::sense_rpc::NativeHpdRpc::new().read_state() {
            Ok(state) => Some(state),
            Err(error) => {
                capabilities.record(
                    "channel.hpd-rpc",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
                None
            }
        };
        if let Some(state) = &hpd {
            capabilities.record("channel.hpd-rpc", Availability::Available, None)?;
            let bit = |b: u64| state.advertised.is_some_and(|cap| cap & b != 0);
            for (feature, avail) in [
                ("sense.status", true),
                ("sense.lock-on-leave", bit(0x0010)),
                ("sense.wake-on-approach", bit(0x0002)),
                ("sense.pause-video", bit(0x0200)),
                ("sense.camera-assist", bit(0x0040)),
                ("sense.approach-auto-adjust", bit(0x0008)),
                ("sense.leave-auto-adjust", bit(0x0100)),
                ("sense.reset", true),
                ("sense.attention-tracking", true),
            ] {
                capabilities.record(
                    feature,
                    if avail {
                        Availability::Available
                    } else {
                        Availability::Unavailable
                    },
                    (!avail).then(|| "HPD op-5 capability bit not advertised".into()),
                )?;
            }
            // privacy.alert has no RPC setter and the legacy pipe is rejected
            // by this service version; the field decodes from the registry.
            capabilities.record(
                "sense.privacy-alert",
                Availability::Unavailable,
                Some("legacy SmartSense settings pipe rejects all probed codes on 1.0.8.5".into()),
            )?;
        }
        let panel_refresh_ok = matches!(
            self.wmi.query(
                ROOT_WMI,
                "SELECT Mode FROM LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA WHERE Active = TRUE",
            ),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "channel.panel",
            if panel_refresh_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!panel_refresh_ok)
                .then(|| "firmware exposed no active internal-panel refresh record".into()),
        )?;
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
        let brightness_read_ok = matches!(
            self.wmi
                .query(ROOT_WMI, "SELECT CurrentBrightness FROM WmiMonitorBrightness"),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "channel.panel-brightness",
            if brightness_read_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!brightness_read_ok)
                .then(|| "WmiMonitorBrightness exposed no readable instance".into()),
        )?;
        capabilities.record(
            "panel.brightness.status",
            if brightness_read_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!brightness_read_ok)
                .then(|| "WmiMonitorBrightness exposed no readable instance".into()),
        )?;
        let brightness_methods_ok = matches!(
            self.wmi
                .query(ROOT_WMI, "SELECT * FROM WmiMonitorBrightnessMethods"),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "panel.brightness.set",
            if brightness_methods_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!brightness_methods_ok)
                .then(|| "WmiMonitorBrightnessMethods exposed no instance".into()),
        )?;
        let dbdc_ok = matches!(
            self.wmi.query(
                ROOT_WMI,
                "SELECT Counts FROM LENOVO_REPORT_DBDC_DATA",
            ),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "perf.dbdc",
            if dbdc_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!dbdc_ok).then(|| "LENOVO_REPORT_DBDC_DATA returned no instance".into()),
        )?;
        capabilities.record(
            "channel.performance",
            if perf_mode_probe.is_ok() || dbdc_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (perf_mode_probe.is_err() && !dbdc_ok)
                .then(|| "neither the power overlay nor the DBDC report read successfully".into()),
        )?;
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
        capabilities.record(
            "channel.temperature",
            if !thermal_source_failed && !battery_source_failed {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (thermal_source_failed || battery_source_failed)
                .then(|| "a thermal source query failed at the transport level".into()),
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
            "channel.fan",
            if fan_inventory_available || gamezone_fan_mode.is_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!fan_inventory_available && gamezone_fan_mode.is_err())
                .then(|| "firmware exposed neither a fan inventory nor a fan mode".into()),
        )?;
        // Process telemetry and per-process policy control ride the public
        // Win32 process APIs (ToolHelp32 enumeration, GetProcessTimes,
        // SetPriorityClass, SetProcessAffinityMask, SetProcessWorkingSetSizeEx);
        // no vendor service or driver is involved.
        capabilities.record(
            "perf.top",
            Availability::Available,
            Some("Win32 process enumeration with interval CPU sampling".into()),
        )?;
        capabilities.record(
            "perf.boost",
            Availability::Available,
            Some("Win32 per-process priority policy with independent readback".into()),
        )?;
        capabilities.record(
            "perf.throttle",
            Availability::Available,
            Some("Win32 per-process priority policy with independent readback".into()),
        )?;
        capabilities.record(
            "channel.process",
            Availability::Available,
            Some("Win32 process enumeration and policy APIs".into()),
        )?;
        capabilities.record(
            "power.scheme",
            if power_scheme_probe_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!power_scheme_probe_ok()).then(|| "Power API active-scheme probe failed".into()),
        )?;
        capabilities.record(
            "power.scheme.setting",
            if power_scheme_probe_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!power_scheme_probe_ok()).then(|| "Power API active-scheme probe failed".into()),
        )?;
        capabilities.record(
            "power.scheme.setting.read",
            if power_scheme_probe_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!power_scheme_probe_ok()).then(|| "Power API active-scheme probe failed".into()),
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
        capabilities.record(
            "channel.tuning",
            Availability::Limited,
            Some("PERFEPP policy channel is available; raw MSR/RAPL limits are not".into()),
        )?;
        capabilities.record(
            "channel.processor-power",
            Availability::Limited,
            Some("PERFBOOSTMODE policy channel is probed at command time".into()),
        )?;
        #[cfg(windows)]
        {
            use crate::{DolbyRpc, ElevocRpc, LocalDriverUpdateApi, PowerApi, WlanNetshRunner};
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
            let power_probe = crate::WindowsPowerP0::new(crate::NativePowerApi)
                .api()
                .active_scheme();
            capabilities.record(
                "channel.power",
                if power_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                power_probe.as_ref().err().map(|error| error.to_string()),
            )?;
            let wifi_probe = crate::NetshCommandRunner::default().show_interfaces();
            capabilities.record(
                "channel.wifi",
                if wifi_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                wifi_probe.as_ref().err().map(|error| error.to_string()),
            )?;
            capabilities.record(
                "wifi.security",
                if wifi_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                wifi_probe.err().map(|error| error.to_string()),
            )?;
            let dolby_probe =
                crate::NativeDolbyRpc::load().and_then(|rpc| rpc.dolby_enabled().map(|_| ()));
            let dolby_ok = dolby_probe.is_ok();
            let dolby_detail = dolby_probe.err().map(|error| error.to_string());
            capabilities.record(
                "channel.dolby",
                if dolby_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                dolby_detail.clone(),
            )?;
            capabilities.record(
                "audio.dolby",
                if dolby_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                dolby_detail.clone(),
            )?;
            let elevoc_probe =
                crate::NativeElevocRpc::load().and_then(|rpc| rpc.current_mode().map(|_| ()));
            let elevoc_ok = elevoc_probe.is_ok();
            let elevoc_detail = elevoc_probe.err().map(|error| error.to_string());
            capabilities.record(
                "channel.elevoc",
                if elevoc_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                elevoc_detail.clone(),
            )?;
            capabilities.record(
                "audio.noise-cancel",
                if elevoc_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                elevoc_detail,
            )?;
            // The audio service role is present when at least one endpoint
            // answers; each feature stays gated by its own probe record.
            capabilities.record(
                "channel.audio",
                if dolby_ok && elevoc_ok {
                    Availability::Available
                } else if dolby_ok || elevoc_ok {
                    Availability::Limited
                } else {
                    Availability::Unavailable
                },
                (!dolby_ok || !elevoc_ok).then(|| {
                    format!(
                        "audio endpoints: dolby={}, elevoc={}",
                        if dolby_ok { "ok" } else { "unavailable" },
                        if elevoc_ok { "ok" } else { "unavailable" },
                    )
                }),
            )?;
            for (feature, availability, detail) in [
                (
                    "channel.osd",
                    Availability::Available,
                    "topmost overlay window; creation is session-gated at show time",
                ),
                (
                    "osd.enable",
                    Availability::Available,
                    "local preference store plus session-gated overlay",
                ),
                (
                    "osd.disable",
                    Availability::Available,
                    "local preference store plus session-gated overlay",
                ),
                (
                    "osd.test",
                    Availability::Available,
                    "overlay creation is session-gated at show time",
                ),
                (
                    "osd.show",
                    Availability::Available,
                    "overlay creation is session-gated at show time",
                ),
                (
                    "channel.hotkey",
                    Availability::Available,
                    "RegisterHotKey path; registration is session-gated at command time",
                ),
                (
                    "hotkey.list",
                    Availability::Available,
                    "in-process hotkey registry",
                ),
                (
                    "hotkey.register",
                    Availability::Available,
                    "RegisterHotKey is session-gated at command time",
                ),
                (
                    "hotkey.trigger",
                    Availability::Available,
                    "in-process dispatch; requires an interactive session for real presses",
                ),
                (
                    "hotkey.unregister",
                    Availability::Available,
                    "in-process hotkey registry",
                ),
                (
                    "channel.machinefix",
                    Availability::Available,
                    "local rule store and service-control APIs",
                ),
                (
                    "machinefix.scan",
                    Availability::Available,
                    "local rule store and service-control APIs",
                ),
                (
                    "machinefix.run",
                    Availability::Available,
                    "local rule store and service-control APIs",
                ),
                (
                    "machinefix.track",
                    Availability::Available,
                    "local applied-fix ledger",
                ),
                (
                    "channel.scene",
                    Availability::Available,
                    "local scene ruleset evaluator",
                ),
                (
                    "scene.test",
                    Availability::Available,
                    "local scene ruleset evaluator",
                ),
                (
                    "scene.track",
                    Availability::Available,
                    "local applied-action ledger",
                ),
            ] {
                capabilities.record(feature, availability, Some(detail.into()))?;
            }
            let update_probe = crate::NativeLocalDriverUpdateApi.present_devices();
            let update_ok = update_probe.is_ok();
            let update_detail = update_probe.err().map(|error| error.to_string());
            capabilities.record(
                "channel.update-local",
                if update_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                update_detail.clone(),
            )?;
            for feature in ["update.local_driver", "update.local_driver.rollback"] {
                capabilities.record(
                    feature,
                    if update_ok {
                        Availability::Available
                    } else {
                        Availability::Unavailable
                    },
                    update_detail.clone(),
                )?;
            }
            capabilities.record(
                "update.local_driver.policy",
                Availability::Available,
                Some("local persisted update-policy store".into()),
            )?;
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

/// Readiness probe shared by the `power.scheme*` capability rows: the OS
/// power API is usable when it can answer the active scheme.
#[cfg(windows)]
fn power_scheme_probe_ok() -> bool {
    use crate::PowerApi;
    crate::WindowsPowerP0::new(crate::NativePowerApi)
        .api()
        .active_scheme()
        .is_ok()
}

#[cfg(not(windows))]
fn power_scheme_probe_ok() -> bool {
    false
}
