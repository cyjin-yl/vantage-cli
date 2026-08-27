//! Linux hardware abstraction backed only by standard sysfs nodes.
//!
//! This backend deliberately does not issue ACPI calls or invoke vendor
//! utilities.  All paths are relative to the root supplied to [`LinuxHal`],
//! which keeps the implementation testable and makes a mounted sysroot
//! possible.
//!
//! The Linux power-supply ABI reports energy in micro-watt-hours (`uWh`),
//! voltage in micro-volts (`uV`), current in micro-amps (`uA`), and battery
//! temperature in tenths of a degree Celsius (`deci-C`).  [`BatteryTelemetry`]
//! exposes milli-units and tenths of a degree Kelvin, so this module converts
//! `uWh / 1000 -> mWh`, `uV / 1000 -> mV`, `uA / 1000 -> mA`, and
//! `deci-C + 273.2 -> deci-K` (rounded to the nearest representable tenth).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lctrl_core::{
    AdapterAuthentication, AdapterInfo, ApplyMode, Availability, BatteryTelemetry, CapabilitySet,
    ChangeReport, ChargeMode, ChargeModeActual, ChargePrimitive, ChargeStatus, HardwareInfo,
    LctrlError, Platform, Result, plan_charge_mode,
};
use lctrl_hal::{BatteryControl, Hal};

const SYSFS_ROOT: &str = "sys";
const DMI_ROOT: &str = "sys/class/dmi/id";
const POWER_SUPPLY_ROOT: &str = "sys/class/power_supply";
const BATTERY_NAME: &str = "BAT0";
const CONSERVATION_MODE: &str = "sys/devices/platform/ideapad/conservation_mode";
const FAST_CHARGE: &str = "sys/devices/platform/ideapad/fast_charge";
const KBD_BACKLIGHT: &str = "sys/devices/platform/ideapad/kbd_backlight";
const KBD_BACKLIGHT_LEDS: [&str; 3] = [
    "sys/class/leds/laptop:kbd_backlight/brightness",
    "sys/class/leds/platform::kbd_backlight/brightness",
    "sys/class/leds/ideapad::kbd_backlight/brightness",
];
const TOUCHPAD: &str = "sys/devices/platform/ideapad/touchpad";
const CAMERA_POWER: &str = "sys/devices/platform/ideapad/camera_power";
const DRM_ROOT: &str = "sys/class/drm";
const RAPL_ROOTS: [&str; 4] = [
    "sys/class/powercap/intel-rapl/intel-rapl:0",
    "sys/class/powercap/intel-rapl:0",
    "sys/devices/virtual/powercap/intel-rapl/intel-rapl:0",
    "sys/devices/virtual/powercap/intel-rapl:0",
];
const DMI_FALLBACK_ROOT: &str = "sys/devices/virtual/dmi/id";
const PERMISSION_NEED: &str = "root or configured udev rule";

/// A Linux HAL whose sysfs root can be replaced by a fixture or a mounted
/// namespace.  [`LinuxHal::new`] uses the host root (`/`).
#[derive(Clone, Debug)]
pub struct LinuxHal {
    root: PathBuf,
}

impl LinuxHal {
    /// Construct a backend rooted at the host filesystem.
    #[must_use]
    pub fn new() -> Self {
        Self::with_root(PathBuf::from("/"))
    }

    /// Construct a backend rooted at `root`.
    #[must_use]
    pub const fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Return the injected filesystem root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        let relative = relative.as_ref();
        if relative.is_absolute() {
            relative.to_path_buf()
        } else {
            self.root.join(relative)
        }
    }

    fn node_exists(&self, relative: impl AsRef<Path>) -> Result<bool> {
        match fs::metadata(self.path(relative)) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(map_io_error(error)),
        }
    }

    fn read_required(&self, relative: impl AsRef<Path>) -> Result<String> {
        fs::read_to_string(self.path(relative)).map_err(map_io_error)
    }

    fn read_optional(&self, relative: impl AsRef<Path>) -> Result<Option<String>> {
        match fs::read_to_string(self.path(relative)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(map_io_error(error)),
        }
    }

    fn node_exists_path(&self, path: impl AsRef<Path>) -> Result<bool> {
        match fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(map_io_error(error)),
        }
    }

    fn read_required_path(&self, path: impl AsRef<Path>) -> Result<String> {
        fs::read_to_string(path).map_err(map_io_error)
    }

    fn read_optional_path(&self, path: impl AsRef<Path>) -> Result<Option<String>> {
        match fs::read_to_string(path) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(map_io_error(error)),
        }
    }

    fn dmi_value(&self, name: &str) -> Result<Option<String>> {
        let primary = Path::new(DMI_ROOT).join(name);
        let fallback = Path::new(DMI_FALLBACK_ROOT).join(name);
        let value = match self.read_optional(&primary)? {
            Some(value) => Some(value),
            None => self.read_optional(&fallback)?,
        };
        Ok(value.and_then(|value| non_empty_trimmed(&value)))
    }

    fn battery_dir(&self, index: u32) -> PathBuf {
        self.path(Path::new(POWER_SUPPLY_ROOT).join(format!("BAT{index}")))
    }

    fn battery_value(&self, index: u32, name: &str) -> Result<Option<String>> {
        self.read_optional_path(self.battery_dir(index).join(name))
    }

    fn rapl_constraint_exists(&self, constraint: u8) -> Result<bool> {
        let names = match constraint {
            0 => ["constraint_0_max_power_uw", "constraint_0_power_limit_uw"],
            1 => ["constraint_1_max_power_uw", "constraint_1_power_limit_uw"],
            _ => return Ok(false),
        };
        for root in RAPL_ROOTS {
            for name in names {
                if self.node_exists(Path::new(root).join(name))? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn rapl_energy_readable(&self) -> Result<bool> {
        for root in RAPL_ROOTS {
            let energy = Path::new(root).join("energy_uj");
            match self.read_optional_path(self.path(energy)) {
                Ok(Some(_)) => return Ok(true),
                Ok(None) | Err(LctrlError::PermissionDenied { .. }) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(false)
    }

    fn drm_modes_exists(&self) -> Result<bool> {
        let root = self.path(DRM_ROOT);
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(map_io_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            if self.node_exists_path(entry.path().join("modes"))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn mode_state(&self) -> Result<(u32, u32)> {
        if !self.node_exists(CONSERVATION_MODE)? {
            return Err(LctrlError::Unsupported {
                feature: "battery.conservation".into(),
            });
        }
        // Both nodes are required for a safe, unambiguous mode state.  In
        // particular, conservation=0 cannot prove Normal when fast_charge is
        // not exposed by the running kernel.
        if !self.node_exists(FAST_CHARGE)? {
            return Err(LctrlError::Unsupported {
                feature: "battery.fast_charge".into(),
            });
        }
        let conservation =
            parse_switch_raw(&self.read_required(CONSERVATION_MODE)?, CONSERVATION_MODE)?;
        let rapid = parse_switch_raw(&self.read_required(FAST_CHARGE)?, FAST_CHARGE)?;
        Ok((conservation, rapid))
    }

    fn mode_actual(&self) -> Result<ChargeModeActual> {
        let (conservation, rapid) = self.mode_state()?;
        if conservation <= 1 && rapid <= 1 {
            return Ok(match (conservation, rapid) {
                (0, 0) => ChargeModeActual::Normal,
                (1, 0) => ChargeModeActual::Conservation,
                (0, 1) => ChargeModeActual::Rapid,
                (1, 1) => ChargeModeActual::Conflict,
                _ => unreachable!("binary mode values were checked"),
            });
        }
        let rapid_bits = rapid
            .checked_mul(2)
            .ok_or_else(|| malformed(FAST_CHARGE, &rapid.to_string()))?;
        let raw = conservation
            .checked_add(rapid_bits)
            .ok_or_else(|| malformed(CONSERVATION_MODE, &conservation.to_string()))?;
        Ok(ChargeModeActual::Unknown(raw))
    }

    fn mode_for_write(&self) -> Result<ChargeMode> {
        match self.mode_actual()? {
            ChargeModeActual::Normal => Ok(ChargeMode::Normal),
            ChargeModeActual::Conservation => Ok(ChargeMode::Conservation),
            ChargeModeActual::Rapid => Ok(ChargeMode::Rapid),
            ChargeModeActual::Conflict => Err(LctrlError::FirmwareRejected {
                detail: "battery charge mode has both conservation and fast-charge enabled".into(),
            }),
            ChargeModeActual::Unknown(raw) => Err(LctrlError::FirmwareRejected {
                detail: format!("battery charge mode has unknown state {raw}"),
            }),
        }
    }

    fn write_primitive(&self, primitive: ChargePrimitive) -> Result<()> {
        let (relative, value) = match primitive {
            ChargePrimitive::Conservation(enabled) => (CONSERVATION_MODE, enabled),
            ChargePrimitive::Rapid(enabled) => (FAST_CHARGE, enabled),
        };
        fs::write(self.path(relative), if value { "1" } else { "0" }).map_err(map_io_error)
    }

    fn ac_online(&self) -> Result<Option<bool>> {
        let root = self.path(POWER_SUPPLY_ROOT);
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(map_io_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            let path = entry.path();
            if !self.node_exists_path(path.join("online"))? {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let known_name = matches!(
                name.as_ref(),
                "AC" | "AC0" | "AC1" | "ACAD" | "ADP" | "ADP0" | "ADP1" | "ADP2" | "Mains"
            );
            let supply_type = self.read_optional_path(path.join("type"))?;
            let is_mains = supply_type
                .as_deref()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("mains"));
            if !known_name && !is_mains {
                continue;
            }
            let value = self.read_required_path(path.join("online"))?;
            match parse_switch(&value, &format!("{name}/online")) {
                Ok(value) => return Ok(Some(value)),
                Err(LctrlError::FirmwareRejected { .. }) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn record_node(
        &self,
        capabilities: &mut CapabilitySet,
        feature: &str,
        relative: impl AsRef<Path>,
    ) -> Result<()> {
        let relative = relative.as_ref();
        let available = self.node_exists(relative)?;
        let detail =
            (!available).then(|| format!("sysfs node unavailable: {}", relative.display()));
        capabilities.record(
            feature,
            if available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            detail,
        )?;
        Ok(())
    }

    fn record_fixed_unsupported(
        capabilities: &mut CapabilitySet,
        feature: &str,
        detail: &str,
    ) -> Result<()> {
        capabilities.record(feature, Availability::Unavailable, Some(detail.into()))?;
        Ok(())
    }
}

impl Default for LinuxHal {
    fn default() -> Self {
        Self::new()
    }
}

impl Hal for LinuxHal {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: self.dmi_value("product_name")?,
            family: self.dmi_value("product_family")?,
            bios_version: self.dmi_value("bios_version")?,
        })
    }

    fn capabilities(&self) -> Result<CapabilitySet> {
        let mut capabilities = CapabilitySet::new(Platform::Linux);
        self.record_node(&mut capabilities, "channel.sysfs", SYSFS_ROOT)?;
        self.record_node(
            &mut capabilities,
            "battery.info",
            Path::new(POWER_SUPPLY_ROOT).join(BATTERY_NAME),
        )?;
        self.record_node(
            &mut capabilities,
            "battery.status",
            Path::new(POWER_SUPPLY_ROOT)
                .join(BATTERY_NAME)
                .join("status"),
        )?;

        let adapter_available = self.ac_online()?.is_some();
        capabilities.record(
            "battery.adapter",
            if adapter_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!adapter_available).then(|| "no reliable AC online sysfs node".into()),
        )?;

        self.record_node(&mut capabilities, "battery.conservation", CONSERVATION_MODE)?;
        self.record_node(&mut capabilities, "battery.fast_charge", FAST_CHARGE)?;

        let ideapad_backlight = self.node_exists(KBD_BACKLIGHT)?;
        let mut led_backlight = false;
        for led in KBD_BACKLIGHT_LEDS {
            if self.node_exists(led)? {
                led_backlight = true;
                break;
            }
        }
        let backlight_available = ideapad_backlight || led_backlight;
        capabilities.record(
            "kbd.backlight",
            if backlight_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!backlight_available).then(|| "no ideapad or LED backlight node".into()),
        )?;

        self.record_node(&mut capabilities, "touchpad", TOUCHPAD)?;
        self.record_node(&mut capabilities, "privacy.camera", CAMERA_POWER)?;

        let rapl_energy = self.rapl_energy_readable()?;
        capabilities.record(
            "tune.rapl",
            if rapl_energy {
                Availability::Limited
            } else {
                Availability::Unavailable
            },
            Some(if rapl_energy {
                "Linux powercap telemetry/read-only surface".into()
            } else {
                "no readable Intel RAPL energy_uj node".into()
            }),
        )?;
        for (feature, constraint) in [("tune.pl1", 0_u8), ("tune.pl2", 1_u8)] {
            let available = self.rapl_constraint_exists(constraint)?;
            capabilities.record(
                feature,
                if available {
                    Availability::Limited
                } else {
                    Availability::Unavailable
                },
                Some(if available {
                    "Linux powercap only".into()
                } else {
                    "no Intel RAPL constraint sysfs node".into()
                }),
            )?;
        }

        let drm_modes = self.drm_modes_exists()?;
        capabilities.record(
            "panel.refresh",
            if drm_modes {
                Availability::Limited
            } else {
                Availability::Unavailable
            },
            Some(if drm_modes {
                "DRM connector modes detected; no mode mutator is exposed".into()
            } else {
                "no DRM connector modes sysfs node".into()
            }),
        )?;

        Self::record_fixed_unsupported(
            &mut capabilities,
            "battery.thresholds",
            "Linux exposes no arbitrary percentage threshold mutator",
        )?;
        Self::record_fixed_unsupported(
            &mut capabilities,
            "channel.acpi_call",
            "raw ACPI calls are intentionally not exposed by this backend",
        )?;
        Self::record_fixed_unsupported(
            &mut capabilities,
            "kbd.fnlock",
            "no standard sysfs channel for Fn/Ctrl or F1-F12 function mode",
        )?;
        Self::record_fixed_unsupported(
            &mut capabilities,
            "privacy.microphone",
            "microphone power sysfs is not part of this backend contract",
        )?;
        Self::record_fixed_unsupported(
            &mut capabilities,
            "tune.epp",
            "EPP is observable but no Linux tuning mutator is exposed here",
        )?;

        Ok(capabilities)
    }
}

impl BatteryControl for LinuxHal {
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        if index != 0 {
            return Err(LctrlError::Unsupported {
                feature: "battery.status".into(),
            });
        }
        let battery = self.battery_dir(index);
        if !self.node_exists(&battery)? {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("BAT{index} power_supply"),
            });
        }

        let energy_full_design = self
            .battery_value(index, "energy_full_design")?
            .map(|value| parse_energy(&value, "energy_full_design"))
            .transpose()?;
        let energy_full = self
            .battery_value(index, "energy_full")?
            .map(|value| parse_energy(&value, "energy_full"))
            .transpose()?;
        let energy_now = self
            .battery_value(index, "energy_now")?
            .map(|value| parse_energy(&value, "energy_now"))
            .transpose()?;
        let voltage_now = self
            .battery_value(index, "voltage_now")?
            .map(|value| parse_voltage(&value, "voltage_now"))
            .transpose()?;
        let current_now = self
            .battery_value(index, "current_now")?
            .map(|value| parse_current(&value, "current_now"))
            .transpose()?;
        let temperature = self
            .battery_value(index, "temp")?
            .map(|value| parse_temperature(&value, "temp"))
            .transpose()?;
        let design_voltage = self
            .battery_value(index, "voltage_min_design")?
            .map(|value| parse_voltage(&value, "voltage_min_design"))
            .transpose()?;
        let remaining_percent = self
            .battery_value(index, "capacity")?
            .map(|value| parse_u16(&value, "capacity"))
            .transpose()?;
        let cycle_count = self
            .battery_value(index, "cycle_count")?
            .map(|value| parse_u16(&value, "cycle_count"))
            .transpose()?;
        let charge_status = self
            .battery_value(index, "status")?
            .and_then(|value| parse_status(&value));

        Ok(BatteryTelemetry {
            design_capacity_mwh: energy_full_design,
            full_charge_capacity_mwh: energy_full,
            remaining_capacity_mwh: energy_now,
            voltage_mv: voltage_now,
            current_ma: current_now,
            temperature_deci_kelvin: temperature,
            manufacture_date: None,
            first_used_date: None,
            design_voltage_mv: design_voltage,
            remaining_percent,
            life_percent: None,
            charge_status,
            remaining_time_min: None,
            charge_completion_time_min: None,
            wattage_w: None,
            cycle_count,
        })
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        if self.ac_online()?.is_none() {
            return Err(LctrlError::Unsupported {
                feature: "battery.adapter".into(),
            });
        }
        // Linux power_supply has no portable equivalent of the Lenovo GBMD
        // authentication/detail words.  Do not invent them from an AC online
        // bit: only the channel's existence is reported.
        Ok(AdapterInfo {
            authentication: AdapterAuthentication::Unknown,
            has_detail: false,
            detail: None,
        })
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        self.mode_actual()
    }

    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        let previous = self.mode_for_write()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }

        // `plan_charge_mode` places the opposite feature's off write before
        // the requested feature's on write for Conservation and Rapid.
        for primitive in plan_charge_mode(mode) {
            self.write_primitive(primitive)?;
        }

        let actual = self.mode_actual()?;
        let expected = match actual {
            ChargeModeActual::Normal => ChargeMode::Normal,
            ChargeModeActual::Conservation => ChargeMode::Conservation,
            ChargeModeActual::Rapid => ChargeMode::Rapid,
            ChargeModeActual::Conflict => {
                return Err(LctrlError::VerifyMismatch {
                    requested: mode.to_string(),
                    actual: actual.to_string(),
                });
            }
            ChargeModeActual::Unknown(raw) => {
                return Err(LctrlError::VerifyMismatch {
                    requested: mode.to_string(),
                    actual: format!("unknown ({raw})"),
                });
            }
        };
        if expected != mode {
            return Err(LctrlError::VerifyMismatch {
                requested: mode.to_string(),
                actual: actual.to_string(),
            });
        }
        Ok(ChangeReport::committed(previous, mode, actual_mode(actual)))
    }
}

fn actual_mode(actual: ChargeModeActual) -> ChargeMode {
    match actual {
        ChargeModeActual::Normal => ChargeMode::Normal,
        ChargeModeActual::Conservation => ChargeMode::Conservation,
        ChargeModeActual::Rapid => ChargeMode::Rapid,
        ChargeModeActual::Conflict | ChargeModeActual::Unknown(_) => ChargeMode::Normal,
    }
}

fn map_io_error(error: io::Error) -> LctrlError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        LctrlError::PermissionDenied {
            need: PERMISSION_NEED.into(),
        }
    } else {
        LctrlError::Io(error)
    }
}

fn non_empty_trimmed(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn malformed(field: &str, value: &str) -> LctrlError {
    LctrlError::FirmwareRejected {
        detail: format!("invalid sysfs value for {field}: {}", value.trim()),
    }
}

fn parse_unsigned(value: &str, field: &str) -> Result<u128> {
    value
        .trim()
        .parse::<u128>()
        .map_err(|_| malformed(field, value))
}

fn parse_signed(value: &str, field: &str) -> Result<i128> {
    value
        .trim()
        .parse::<i128>()
        .map_err(|_| malformed(field, value))
}

fn parse_energy(value: &str, field: &str) -> Result<u32> {
    let milliwatt_hours = parse_unsigned(value, field)? / 1_000;
    u32::try_from(milliwatt_hours).map_err(|_| malformed(field, value))
}

fn parse_voltage(value: &str, field: &str) -> Result<u16> {
    let millivolts = parse_unsigned(value, field)? / 1_000;
    u16::try_from(millivolts).map_err(|_| malformed(field, value))
}

fn parse_current(value: &str, field: &str) -> Result<i32> {
    let milliamps = parse_signed(value, field)? / 1_000;
    i32::try_from(milliamps).map_err(|_| malformed(field, value))
}

fn parse_temperature(value: &str, field: &str) -> Result<u16> {
    let deci_celsius = parse_signed(value, field)?;
    let deci_kelvin = deci_celsius
        .checked_add(2_732)
        .ok_or_else(|| malformed(field, value))?;
    u16::try_from(deci_kelvin).map_err(|_| malformed(field, value))
}

fn parse_u16(value: &str, field: &str) -> Result<u16> {
    let value = parse_unsigned(value, field)?;
    u16::try_from(value).map_err(|_| malformed(field, &value.to_string()))
}

fn parse_switch_raw(value: &str, field: &str) -> Result<u32> {
    let value = parse_unsigned(value, field)?;
    u32::try_from(value).map_err(|_| malformed(field, &value.to_string()))
}

fn parse_switch(value: &str, field: &str) -> Result<bool> {
    match parse_switch_raw(value, field)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(malformed(field, value)),
    }
}

fn parse_status(value: &str) -> Option<ChargeStatus> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("charging") {
        Some(ChargeStatus::Charging)
    } else if value.eq_ignore_ascii_case("discharging") {
        Some(ChargeStatus::Discharging)
    } else if value.eq_ignore_ascii_case("discharging with ac") {
        Some(ChargeStatus::DischargingWithAc)
    } else if value.eq_ignore_ascii_case("not charging") || value.eq_ignore_ascii_case("full") {
        Some(ChargeStatus::NoActivity)
    } else {
        None
    }
}
