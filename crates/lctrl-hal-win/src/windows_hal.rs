use lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result, SystemIdentity};
use lctrl_hal::{
    BatteryElectricalSource, BluetoothControl, Hal, HotkeyControl, SecurityAvailability,
    TouchpadControl, WifiStatusControl,
};

use crate::HpdRpc;
use crate::MmDeviceApi;
use crate::{
    EnergyDriver, GameZone, IoctlTransport, NativePerformanceRegistry, PerformanceRegistryReader,
    WmiObject, WmiTransport, WmiValue, active_instance, its_policy::record_its_policy_capability,
    mobile_broadband::probe_mobile_broadband,
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

    /// Read the identity modules from `root\CIMV2` (product, baseboard, BIOS).
    ///
    /// This is the same bounded query set as
    /// [`crate::WindowsInventory::capture_identity`]; it adds no vendor WMI
    /// class, driver, or registry fallback.
    fn system_identity(&self) -> Result<SystemIdentity> {
        crate::system_inventory::identity_from_wmi(&self.wmi)
    }

    /// Read processor identity and the derived current clock.
    ///
    /// Identity comes from `Win32_Processor` plus the firmware block under
    /// `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`; the current clock
    /// is derived from the documented processor performance counter and stays
    /// absent when that counter has no sample
    /// (`docs/evidence/re-cpu-20261105.md`).
    fn cpu_status(&self) -> Result<lctrl_core::CpuStatus> {
        crate::cpu::cpu_status(&self.wmi, &crate::NativeCpuRegistry)
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

        // Firmware inventory: the platform's own ESRT projection is the only
        // prerequisite, so the probe is the firmware-class device query the
        // `firmware status` read itself uses.
        let firmware_ok = crate::firmware::probe_firmware_status(&self.wmi);
        capabilities.record(
            "firmware.status",
            if firmware_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!firmware_ok)
                .then(|| "no firmware-class (ESRT) device is projected".into()),
        )?;

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
        // Wear facts need the root\WMI battery class chain: BatteryStaticData
        // carries the design capacity that BatteryStatus alone does not.
        capabilities.record(
            "battery.health",
            if os_battery_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            os_battery_error.clone(),
        )?;
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

        // Two independent backlight planes exist: the ACPI `KBLC` method the
        // Lenovo virtual power controller re-exposes on `\\.\EnergyDrv`
        // (recovered on 21VG, four states including auto), and the WMI
        // lighting plane some RGB models publish instead. Either answering is
        // a usable channel; the probe reports which one answered.
        //
        // The same answer is the keyboard role's transport proof: `channel.keyboard`
        // is the id `ServiceRole::Keyboard` joins on, and the Linux backend records
        // that id for the same hardware channel (`channel.keyboard` in
        // `lctrl-hal-linux`). One probe, two ids, no second driver call.
        let (keyboard_availability, keyboard_detail) =
            match crate::kbd_backlight::probe_vpc_keyboard(&self.ioctl) {
                Ok(layout) => (
                    Availability::Available,
                    Some(format!(
                        "ACPI VPC KBLC keyboard group {} exposes states 0..={}{}",
                        layout.group(),
                        layout.max_state(),
                        if layout.auto_state().is_some() {
                            " (auto included)"
                        } else {
                            ""
                        }
                    )),
                ),
                Err(kblc_error) => match active_instance(&self.wmi, "LENOVO_LIGHTING_DATA") {
                    Ok(_) => (
                        Availability::Available,
                        Some("LENOVO_LIGHTING_DATA exposes a lighting instance".into()),
                    ),
                    Err(lighting_error) => (
                        Availability::Unavailable,
                        Some(format!(
                            "ACPI VPC KBLC: {kblc_error}; LENOVO_LIGHTING_DATA: {lighting_error}"
                        )),
                    ),
                },
            };
        for feature in ["kbd.backlight", "channel.keyboard"] {
            capabilities.record(feature, keyboard_availability, keyboard_detail.clone())?;
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
                .temporary_charge_probe()
                .is_ok();
        capabilities.record(
            "battery.temporary-mode",
            if temporary_charge_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!temporary_charge_available).then(|| {
                "EnergyDrv temporary-charge probe (0x83102148/0x13) is unavailable".into()
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
        // Storage identity/health: the disk interface class and
        // `IOCTL_STORAGE_QUERY_PROPERTY` answer on a handle opened with no
        // requested access, so the probe is that enumeration plus one
        // device-descriptor request rather than a full per-drive report
        // (docs/evidence/re-storage-20261105.md §2, §7). A device that refuses
        // an individual field is recorded per field by the command itself, not
        // by this capability.
        let storage_probe = crate::storage::probe_storage();
        let storage_detail = storage_probe.as_ref().err().map(ToString::to_string);
        for channel in ["channel.storage", "storage.status"] {
            capabilities.record(
                channel,
                if storage_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                storage_detail.clone(),
            )?;
        }
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
        // Mobile Broadband / MagicBay LTE status: available once both CIM
        // namespaces answer (accessory query and WWAN interface query). A
        // machine without an accessory still answers with zero rows, which the
        // dispatcher reports as `ChannelUnavailable` for the command itself.
        let mobile_broadband = probe_mobile_broadband(&self.wmi);
        let mobile_broadband_detail = match &mobile_broadband {
            Ok(probe) => Some(format!(
                "{} MagicBay accessory record(s), {} WWAN interface(s)",
                probe.inventory.devices.len(),
                probe.wwan_interfaces
            )),
            Err(error) => Some(error.to_string()),
        };
        for channel in ["channel.mobilebroadband", "magicbay.lte"] {
            capabilities.record(
                channel,
                if mobile_broadband.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                if mobile_broadband.is_ok() {
                    None
                } else {
                    mobile_broadband_detail.clone()
                },
            )?;
        }
        for feature in [
            "privacy.camera",
            "privacy.microphone",
            "privacy.fingerprint",
        ] {
            capabilities.record(
                feature,
                if bios_settings_ok && staging_verified {
                    Availability::Available
                } else if bios_settings_ok {
                    Availability::Limited
                } else {
                    Availability::Unavailable
                },
                if bios_settings_ok && staging_verified {
                    Some("persistent BIOS privacy toggle writes through the verified staging buffer".into())
                } else if bios_settings_ok {
                    Some("persistent BIOS privacy toggle reads are available; staging-buffer writes unverified".into())
                } else {
                    Some("Lenovo_BiosSetting exposes no privacy toggles".into())
                },
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
            for feature in ["channel.touchpad", "touchpad", "touchpad.status"] {
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
            // Recovery is admitted by the input leaf group, not by the
            // aggregate state: a split group is exactly what recovery repairs,
            // so probing the aggregate read would report recovery unavailable
            // whenever it is needed.
            let leaves = runtime.touchpad_leaves();
            capabilities.record(
                "touchpad.recover",
                if leaves.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                leaves.as_ref().err().map(|error| error.to_string()),
            )?;
            // The capability descriptor is read from the admitted digitizer
            // collection itself, so its probe is the descriptor read and not
            // the input-leaf group the switches use.
            let caps = runtime
                .selected_device(crate::RuntimeDeviceKind::Touchpad)
                .and_then(|device| crate::read_touchpad_capabilities(&device.instance_id));
            capabilities.record(
                "touchpad.caps",
                if caps.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                caps.as_ref().err().map(|error| error.to_string()),
            )?;
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
        // The identity channel joins the monitor connection classification with
        // the per-instance EDID value; the probe only reads the classification,
        // so a machine without an embedded panel stays honestly unavailable
        // instead of reporting a stale registry record.
        let panel_identity = crate::panel_identity::internal_panel_device(&self.wmi);
        let panel_identity_ok = panel_identity.is_ok();
        let panel_identity_reason = panel_identity.err().map(|error| error.to_string());
        capabilities.record(
            "channel.panel-identity",
            if panel_identity_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            panel_identity_reason.clone(),
        )?;
        capabilities.record(
            "panel.identity",
            if panel_identity_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            panel_identity_reason,
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
        // GPU OC is a read-only support probe plus an optional parameter
        // block; capability discovery must stay query-only, so this reports
        // the parameter class and lets `perf gpu status` read the probe.
        let gpu_oc_ok = matches!(
            self.wmi
                .query(ROOT_WMI, "SELECT * FROM LENOVO_GAMEZONE_GPU_OC_DATA"),
            Ok(objects) if !objects.is_empty()
        );
        capabilities.record(
            "perf.gpu",
            if gpu_oc_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!gpu_oc_ok).then(|| "LENOVO_GAMEZONE_GPU_OC_DATA returned no instance".into()),
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
        // Intel ESIF participants are the third readable temperature source.
        // An absent class (no Intel Dynamic Tuning) is not a failure, and a
        // participant that reports 0 is not a reading
        // (docs/evidence/re-thermal-20261105.md §3).
        let mut esif_source_failed = false;
        let mut esif_readable = false;
        let esif_temperature_error: Option<String> = match self.wmi.query(
            ROOT_WMI,
            "SELECT InstanceName, Temperature FROM EsifDeviceInformation",
        ) {
            Ok(objects) => {
                esif_readable = objects.iter().any(|object| {
                    matches!(
                        crate::thermal::required_u32(object, "Temperature"),
                        Ok(temperature) if temperature != 0
                    )
                });
                objects
                    .iter()
                    .find_map(|object| crate::thermal::required_u32(object, "Temperature").err())
                    .map(|error| {
                        esif_source_failed = true;
                        error.to_string()
                    })
            }
            Err(error) if matches!(error, lctrl_core::LctrlError::Unsupported { .. }) => None,
            Err(error) => {
                esif_source_failed = true;
                Some(error.to_string())
            }
        };
        let temperature_readable = thermal_zone_error.is_none()
            || battery_temperature_error.is_none()
            || esif_readable;
        let mut source_failures = Vec::new();
        if let Some(reason) = &thermal_zone_error {
            source_failures.push(format!("ACPI: {reason}"));
        }
        if let Some(reason) = &battery_temperature_error {
            source_failures.push(format!("Windows battery class: {reason}"));
        }
        if let Some(reason) = &esif_temperature_error {
            source_failures.push(format!("Intel ESIF participants: {reason}"));
        }
        let mut readable_sources = Vec::new();
        if thermal_zone_error.is_none() {
            readable_sources.push("ACPI thermal zones");
        }
        if battery_temperature_error.is_none() {
            readable_sources.push("Windows battery class");
        }
        if esif_readable {
            readable_sources.push("Intel ESIF participants");
        }
        let temperature_detail = if source_failures.is_empty() {
            None
        } else if readable_sources.is_empty() {
            Some(source_failures.join("; "))
        } else {
            Some(format!(
                "{} readable; {}",
                readable_sources.join(", "),
                source_failures.join("; ")
            ))
        };
        capabilities.record(
            "perf.temp",
            if !thermal_source_failed
                && !battery_source_failed
                && !esif_source_failed
                && temperature_readable
            {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            temperature_detail,
        )?;
        capabilities.record(
            "channel.temperature",
            if !thermal_source_failed && !battery_source_failed && !esif_source_failed {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (thermal_source_failed || battery_source_failed || esif_source_failed)
                .then(|| "a thermal source query failed at the transport level".into()),
        )?;
        record_fan_capabilities(&mut capabilities, &self.wmi, &NativePerformanceRegistry)?;
        // `perf policy` only reads the dispatcher's published `Policy_*`
        // values, so its row is probed with the same read the command does.
        record_its_policy_capability(&mut capabilities, &NativePerformanceRegistry)?;
        // The Lenovo event records are classic Application-channel entries whose
        // provider string is a service name; the probe only discovers provider
        // names and opens (then closes) the channel query, so `info` stays cheap.
        crate::event_log::record_event_log_capability(&mut capabilities)?;
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
        // One-shot power saver composes scheme activation, CPU policy
        // (EPP/boost) and panel dimming. The EPP/boost legs ride the same
        // Windows power-setting channel as power.scheme.*; panel dimming is
        // an optional leg rather than a hard dependency of the workflow.
        let saver_once_ok = power_scheme_probe_ok();
        capabilities.record(
            "power.saver-once",
            if saver_once_ok {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!saver_once_ok).then(|| "Power API active-scheme probe failed".into()),
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
                if power_scheme_probe_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                "PERFEPP write+readback verified via PowerSetting mutation",
            ),
            (
                "tune.turbo",
                if power_scheme_probe_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                "PERFBOOSTMODE write+readback verified via PowerSetting mutation",
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
            if power_scheme_probe_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            Some("PERFBOOSTMODE policy channel is verified write+readback via PowerSetting".into()),
        )?;
        // `cpu status` reads identity from Win32_Processor plus the firmware
        // block, and derives its clock from the documented processor
        // performance counter. The probe is the command's own read, so an
        // unavailable counter is reported as unavailable rather than as a
        // rated frequency (docs/evidence/re-cpu-20261105.md §4, §7).
        let cpu_probe = crate::cpu::cpu_status(&self.wmi, &crate::NativeCpuRegistry);
        let (cpu_availability, cpu_detail) = match &cpu_probe {
            Ok(status) => (
                status.current_clock_availability,
                status
                    .current_clock_detail
                    .clone()
                    .or_else(|| Some("derived from the processor performance counter".into())),
            ),
            Err(error) => (Availability::Unavailable, Some(error.to_string())),
        };
        capabilities.record("cpu.status", cpu_availability, cpu_detail)?;
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
            // The link-state verb speaks the WLAN API directly, so it is probed
            // through its own query rather than inferred from the netsh channel.
            let wifi_status_probe = crate::WindowsWifiStatus.wifi_status();
            capabilities.record(
                "wifi.status",
                if wifi_status_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                wifi_status_probe.as_ref().err().map(|error| error.to_string()),
            )?;
            let bluetooth_probe = crate::WindowsBluetooth.bluetooth_devices();
            let bluetooth_detail = match &bluetooth_probe {
                Ok(inventory) => format!(
                    "{} radio(s), {} known device(s) enumerated without an inquiry",
                    inventory.adapters.len(),
                    inventory.devices.len()
                ),
                Err(error) => error.to_string(),
            };
            capabilities.record(
                "bluetooth.list",
                if bluetooth_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                Some(bluetooth_detail.clone()),
            )?;
            capabilities.record(
                "channel.bluetooth",
                if bluetooth_probe.is_ok() {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                Some(bluetooth_detail),
            )?;
            // The sensor channel is the public Sensor API served by the UMDF
            // sensor device. The probe is the real enumeration (one read per
            // sensor), so the detail states what this machine actually
            // instantiates instead of assuming a sensor type exists
            // (docs/evidence/re-sensors-20261105.md §3).
            let sensor_probe = lctrl_hal::SensorControl::list(&crate::WindowsSensors);
            let sensor_detail = match &sensor_probe {
                Ok(inventory) => format!(
                    "{} sensor report(s); {} enumerated sensor(s) returned no report",
                    inventory.sensors.len(),
                    inventory.unavailable.len(),
                ),
                Err(error) => error.to_string(),
            };
            let sensor_availability = if sensor_probe.is_ok() {
                Availability::Available
            } else {
                Availability::Unavailable
            };
            for channel in ["channel.sensor", "sensor.list"] {
                capabilities.record(
                    channel,
                    sensor_availability,
                    Some(sensor_detail.clone()),
                )?;
            }
            // Secure Boot / TPM / BitLocker. Every component is read-only and
            // reports its own reason: the channel is `Available` only when all
            // three answered, `Limited` when some did, and `Unavailable` when
            // none did — never `Available` because the APIs merely exist.
            let security_report =
                crate::observe_security(&self.wmi, &crate::NativeFirmwareEnvironment);
            let security_availability = match security_report.availability() {
                SecurityAvailability::Available => Availability::Available,
                SecurityAvailability::Limited => Availability::Limited,
                SecurityAvailability::Unavailable => Availability::Unavailable,
            };
            let security_reasons = security_report.details().join("; ");
            let security_detail = if security_reasons.is_empty() {
                security_report.summary()
            } else {
                format!("{} ({security_reasons})", security_report.summary())
            };
            for channel in ["channel.security", "security.status"] {
                capabilities.record(
                    channel,
                    security_availability,
                    Some(security_detail.clone()),
                )?;
            }
            // Radio plane: the WinRT radio manager enumerates every
            // selectable radio kind in one read-only query — no switch is
            // touched, and a machine with no radio of some kind answers with
            // that kind listed in `absent_kinds` rather than failing. The same
            // answer is the transport proof for `ServiceRole::Radio`, so the
            // role no longer depends on an unmapped id (capability audit §6.2).
            // `radio.off`/`radio.on` stay unprobed on purpose: a status query
            // cannot prove an owned set-state with readback.
            let radio_probe = lctrl_hal::RadioControl::radio_status(&crate::NativeRadio::new());
            let (radio_availability, radio_detail) = match &radio_probe {
                Ok(report) => (
                    Availability::Available,
                    Some(format!(
                        "{} radio(s): {}; no radio for: {}",
                        report.radios.len(),
                        report
                            .radios
                            .iter()
                            .map(|radio| format!("{}={:?}", radio.kind.as_str(), radio.state))
                            .collect::<Vec<_>>()
                            .join(", "),
                        report
                            .absent_kinds
                            .iter()
                            .map(|kind| kind.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    )),
                ),
                Err(error) => (Availability::Unavailable, Some(error.to_string())),
            };
            for channel in ["channel.radio", "radio.status"] {
                capabilities.record(channel, radio_availability, radio_detail.clone())?;
            }
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
            // The OS endpoint surface is independent of both vendor audio
            // stacks: it is the MMDevice API (MMDevAPI.dll), so it answers
            // whenever the audio services run.
            let mmdevice_probe = crate::NativeMmDeviceApi
                .endpoints(false)
                .and_then(|endpoints| {
                    // An empty enumeration is not a working surface: the
                    // volume/mute verbs need an endpoint, so the probe must
                    // not report these features available when it returns
                    // nothing to address.
                    if endpoints.is_empty() {
                        Err(lctrl_core::LctrlError::ChannelUnavailable {
                            channel: "the MMDevice API reported no active audio endpoint".into(),
                        })
                    } else {
                        Ok(())
                    }
                });
            let mmdevice_ok = mmdevice_probe.is_ok();
            let mmdevice_detail = mmdevice_probe.err().map(|error| error.to_string());
            capabilities.record(
                "channel.mmdevice",
                if mmdevice_ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                mmdevice_detail.clone(),
            )?;
            for feature in ["audio.endpoints", "audio.volume", "audio.mute"] {
                capabilities.record(
                    feature,
                    if mmdevice_ok {
                        Availability::Available
                    } else {
                        Availability::Unavailable
                    },
                    mmdevice_detail.clone(),
                )?;
            }
            // The audio service role is present when at least one audio
            // surface answers; each feature stays gated by its own probe
            // record. The Dolby and Elevoc legs keep their semantics and the
            // OS MMDevice surface is an independent third leg, so a machine
            // without the vendor stacks still reports a usable audio role.
            let audio_legs = [
                ("dolby", dolby_ok),
                ("elevoc", elevoc_ok),
                ("mmdevice", mmdevice_ok),
            ];
            let audio_legs_up = audio_legs.iter().filter(|(_, ok)| *ok).count();
            capabilities.record(
                "channel.audio",
                if audio_legs_up == audio_legs.len() {
                    Availability::Available
                } else if audio_legs_up > 0 {
                    Availability::Limited
                } else {
                    Availability::Unavailable
                },
                (audio_legs_up < audio_legs.len()).then(|| {
                    let legs = audio_legs
                        .iter()
                        .map(|(name, ok)| {
                            format!("{name}={}", if *ok { "ok" } else { "unavailable" })
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("audio endpoints: {legs}")
                }),
            )?;
            record_fn_osd(&mut capabilities)?;
            record_hotkey_status(&mut capabilities)?;
            crate::record_usb_port_capabilities(&mut capabilities)?;
            for (feature, availability, detail) in [
                (
                    "channel.osd",
                    Availability::Available,
                    "topmost overlay window; creation is session-gated at show time",
                ),
                (
                    "osd.enable",
                    Availability::Available,
                    "Lenovo Fn OSD via service controls 146/147 (CapsLk) and 148/149 (NumLk), \
                     verified by registry readback",
                ),
                (
                    "osd.disable",
                    Availability::Available,
                    "Lenovo Fn OSD via service controls 146/147 (CapsLk) and 148/149 (NumLk), \
                     verified by registry readback",
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
            // The local half of `update check`: the catalogue the UDC service
            // already synced into `HKLM\SOFTWARE\Lenovo\UDC\Packages`. The probe
            // is one registry read plus a JSON parse — no network, no package
            // evaluation, no install — and it stops at "a config exists and
            // parses", so a synced-but-empty catalogue is `Limited`, not
            // `Available`. The *remote* refresh needs the Lenovo-held UDC mTLS
            // client credential and stays unsupported
            // (docs/evidence/udc-mtls-credential-hunt-20261105.md): the detail
            // names that split so the row cannot be read as "remote check works".
            let catalog_probe = crate::update_catalog::catalog_sync();
            let (catalog_availability, catalog_detail) = match &catalog_probe {
                Ok(Some(sync)) => (
                    if sync.stale || sync.packages == 0 {
                        Availability::Limited
                    } else {
                        Availability::Available
                    },
                    Some(format!(
                        "local synced catalogue: last sync {}, {} package(s) listed{}; remote \
                         refresh requires an authenticated UDC contract",
                        sync.last_sync.as_deref().unwrap_or("unknown"),
                        sync.packages,
                        if sync.stale {
                            " (config marked update-required)"
                        } else {
                            ""
                        }
                    )),
                ),
                Ok(None) => (
                    Availability::Unavailable,
                    Some(
                        "local synced catalogue: the UDC service has written no LastUpdateConfig; \
                         remote refresh requires an authenticated UDC contract"
                            .into(),
                    ),
                ),
                Err(error) => (
                    Availability::Unavailable,
                    Some(format!(
                        "local synced catalogue: {error}; remote refresh requires an \
                         authenticated UDC contract"
                    )),
                ),
            };
            capabilities.record("update.catalog.local", catalog_availability, catalog_detail)?;
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

/// Fan inventory, fan-mode read and fan-mode write capability rows.
///
/// Split out of `WindowsHal::capabilities` so the three fan rows can be
/// exercised with an injected transport and registry reader.
///
/// The read and the write are recorded separately because they are separate
/// firmware surfaces: on ThinkBook 21VG every fan getter returns
/// `WBEM_E_INVALID_OBJECT` while `SetSmartFanMode` still acknowledges calls, so
/// a write-only channel exists that nothing can verify
/// (docs/evidence/re-fan-write-20261105.md). A write is reported available only
/// where the firmware can also report the resulting mode.
pub fn record_fan_capabilities<W, R>(
    capabilities: &mut CapabilitySet,
    wmi: &W,
    registry: &R,
) -> Result<()>
where
    W: WmiTransport + ?Sized,
    R: PerformanceRegistryReader + ?Sized,
{
    let fan_inventory_available = matches!(
        wmi.query(
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

    // The probe never invokes a setter: it reads the mode through the vendor
    // getter (a read-only method call) plus the dispatcher registry value. A
    // write probe would change hardware state.
    let firmware_fan_mode = GameZone::new(wmi).smart_fan_mode();
    // `ITS_FanMode` is the dispatcher's stored fan-policy value. Nothing in
    // LNVDispatcherService reads it back into firmware, so it is reported as a
    // projection, never as a firmware readback.
    let dispatcher_projection = registry.read_dword("ITS_FanMode").ok();
    capabilities.record(
        "perf.fan.mode",
        match (&firmware_fan_mode, dispatcher_projection) {
            (Ok(_), _) => Availability::Available,
            (Err(_), Some(_)) => Availability::Limited,
            (Err(_), None) => Availability::Unavailable,
        },
        match (&firmware_fan_mode, dispatcher_projection) {
            (Ok(raw), _) => Some(format!("GAMEZONE GetSmartFanMode readable (raw={raw})")),
            (Err(error), Some(value)) => Some(format!(
                "GAMEZONE fan mode rejected: {error}; reporting the dispatcher PowerSlider \
                 projection ITS_FanMode={value}, which no firmware read confirms"
            )),
            (Err(error), None) => Some(format!(
                "GAMEZONE fan mode rejected: {error}; ITS_FanMode registry unreadable"
            )),
        },
    )?;
    capabilities.record(
        "perf.fan.mode.write",
        match &firmware_fan_mode {
            Ok(_) => Availability::Available,
            Err(_) => Availability::Unavailable,
        },
        Some(match &firmware_fan_mode {
            Ok(_) => {
                "GAMEZONE GetSmartFanMode readable; fan-mode writes are verified by readback"
                    .to_string()
            }
            Err(error) => format!(
                "GetSmartFanMode unavailable ({error}); SetSmartFanMode acknowledges calls \
                 without validating the value and its result cannot be read back, so a \
                 fan-mode write would be unverifiable (docs/evidence/re-fan-write-20261105.md)"
            ),
        }),
    )?;
    capabilities.record(
        "channel.fan",
        if fan_inventory_available || firmware_fan_mode.is_ok() || dispatcher_projection.is_some() {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        (!fan_inventory_available && firmware_fan_mode.is_err() && dispatcher_projection.is_none())
            .then(|| "firmware exposed neither a fan inventory nor a fan mode".into()),
    )?;
    Ok(())
}

fn first_string(objects: &[WmiObject], key: &str) -> Option<String> {
    objects.iter().find_map(|object| match object.get(key) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    })
}

/// Record the Lenovo Fn OSD rows.
///
/// The probe is a registry read of `VantageToast\ShowCapslkOSD` /
/// `ShowNumlkOSD` plus `Capbility\SupportNumlkKey`; it never sends a service
/// control code, so probing cannot change the preference. See
/// `docs/evidence/re-osd-20261105.md`.
#[cfg(windows)]
fn record_fn_osd(capabilities: &mut CapabilitySet) -> Result<()> {
    let status = crate::NativeFnOsd::new().status();
    let (detail, caps_osd, num_osd) = match &status {
        Ok(status) => {
            let supported = |state: lctrl_hal::OsdIndicatorState, running: bool| {
                match state {
                    lctrl_hal::OsdIndicatorState::On | lctrl_hal::OsdIndicatorState::Off => {
                        running
                    }
                    lctrl_hal::OsdIndicatorState::NotSupported => false,
                }
            };
            (
                status.capslock.detail.clone(),
                supported(status.capslock.state, status.service_running),
                supported(status.numlock.state, status.service_running),
            )
        }
        Err(error) => (error.to_string(), false, false),
    };
    capabilities.record(
        "channel.fn-osd",
        if caps_osd || num_osd {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        Some(detail),
    )?;
    capabilities.record(
        "osd.fn.capslock",
        if caps_osd {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        Some(
            "LenovoFnAndFunctionKeys user-defined controls 146/147; readback \
             VantageToast\\ShowCapslkOSD"
                .into(),
        ),
    )?;
    capabilities.record(
        "osd.fn.numlock",
        if num_osd {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        Some(
            "LenovoFnAndFunctionKeys user-defined controls 148/149; gated on \
             Capbility\\SupportNumlkKey=1"
                .into(),
        ),
    )?;
    Ok(())
}

#[cfg(not(windows))]
fn record_fn_osd(capabilities: &mut CapabilitySet) -> Result<()> {
    for (feature, detail) in [
        ("channel.fn-osd", "the Lenovo Fn OSD channel is a Windows service"),
        (
            "osd.fn.capslock",
            "the Lenovo Fn OSD channel is a Windows service",
        ),
        (
            "osd.fn.numlock",
            "the Lenovo Fn OSD channel is a Windows service",
        ),
    ] {
        capabilities.record(feature, Availability::Unavailable, Some(detail.into()))?;
    }
    Ok(())
}

/// Record the Lenovo Fn hotkey package row behind `hotkey status`.
///
/// The probe is read-only: the owner service state, the `Capbility` flags, the
/// session-1 renderers and the persisted per-keyboard tables. It never sends a
/// service control code, a BIOS write or a key remap, so probing cannot change
/// the machine; the firmware switches are read by `vantage hotkey status`
/// itself, not by this probe. See `docs/evidence/re-hotkeys-20261105.md`.
#[cfg(windows)]
fn record_hotkey_status(capabilities: &mut CapabilitySet) -> Result<()> {
    match crate::NativeHotkey::new().capability_status() {
        Ok(status) => {
            let readable =
                status.service_running || status.utility_running || !status.tables.is_empty();
            let detail = format!(
                "LenovoFnAndFunctionKeys running={}, FnHotkeyUtility running={}, \
                 {} hotkey table(s); `vantage hotkey status` reads the firmware switches",
                status.service_running,
                status.utility_running,
                status.tables.len(),
            );
            capabilities.record(
                "hotkey.status",
                if readable {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                Some(detail),
            )?;
        }
        Err(error) => {
            capabilities.record(
                "hotkey.status",
                Availability::Unavailable,
                Some(error.to_string()),
            )?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn record_hotkey_status(capabilities: &mut CapabilitySet) -> Result<()> {
    capabilities.record(
        "hotkey.status",
        Availability::Unavailable,
        Some("the Lenovo Fn hotkey package is a Windows service".into()),
    )?;
    Ok(())
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
