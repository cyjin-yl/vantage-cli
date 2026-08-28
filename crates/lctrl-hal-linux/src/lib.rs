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
use std::time::Duration;

use lctrl_core::{
    AdapterAuthentication, AdapterInfo, AdapterStatus, ApplyMode, Availability, BacklightState,
    BatteryHealth, BatteryTelemetry, CapabilitySet, ChangeReport, ChargeMode, ChargeModeActual,
    ChargePrimitive, ChargeStatus, DeviceState, DiagnosticKind, DiagnosticOutcome,
    DiagnosticResult, DispatcherVersion, FanDescriptor, FanId, FanMode, FanTable, HardwareInfo,
    LctrlError, LightingEffect, MagicBayDevice, MagicBayInventory, MagicBayKind,
    PerformanceCapabilities, PerformanceMode, PerformanceState, Platform, PowerMutation,
    PowerScheme, PowerSchemeId, PowerValueRange, Result, SensorId, TemperatureLocation,
    TemperatureSensor, TemperatureSensorMetadata, TemperatureSource, UpdateCapability,
    identify_magicbay, plan_charge_mode,
};
use lctrl_hal::{
    BatteryControl, ControlConflictDetection, DiagnosticsControl, FanControl, Hal, KeyboardControl,
    MagicBayControl, PerformanceControl, PowerControl, PowerLimitKind, PrivacyControl,
    TemperatureControl, TouchpadControl, TuningControl, UpdateControl, poll_readback,
};

const SYSFS_ROOT: &str = "sys";
const DMI_ROOT: &str = "sys/class/dmi/id";
const POWER_SUPPLY_ROOT: &str = "sys/class/power_supply";
const BATTERY_NAME: &str = "BAT0";
const CONSERVATION_MODE: &str = "sys/devices/platform/ideapad/conservation_mode";
const FAST_CHARGE: &str = "sys/devices/platform/ideapad/fast_charge";
const KBD_BACKLIGHT: &str = "sys/devices/platform/ideapad/kbd_backlight";
const KBD_BACKLIGHT_MAX: &str = "sys/devices/platform/ideapad/kbd_backlight_max";
const KBD_BACKLIGHT_LEDS: [&str; 3] = [
    "sys/class/leds/laptop:kbd_backlight/brightness",
    "sys/class/leds/platform::kbd_backlight/brightness",
    "sys/class/leds/ideapad::kbd_backlight/brightness",
];
const THERMAL_MODE: &str = "sys/devices/platform/ideapad/thermal_mode";
const TOUCHPAD: &str = "sys/devices/platform/ideapad/touchpad";
const CAMERA_POWER: &str = "sys/devices/platform/ideapad/camera_power";
const HWMON_ROOT: &str = "sys/class/hwmon";
const IDEAPAD_FAN_MODE: &str = "sys/devices/platform/ideapad/fan_mode";
const DRM_ROOT: &str = "sys/class/drm";
const USB_DEVICES_ROOT: &str = "sys/bus/usb/devices";
const CPUFREQ_ROOT: &str = "sys/devices/system/cpu/cpufreq";
const EPP_FILE: &str = "energy_performance_preference";
const ACPI_DEVICES_ROOT: &str = "sys/bus/acpi/devices";
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

    fn commit_charge_mode(&self, mode: ChargeMode) -> Result<ChargeMode> {
        for primitive in plan_charge_mode(mode) {
            self.write_primitive(primitive)?;
        }
        poll_readback(&mode, 10, Duration::from_millis(50), || {
            self.mode_for_write()
        })
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

    fn backlight_nodes(&self) -> Result<(PathBuf, PathBuf)> {
        if self.node_exists(KBD_BACKLIGHT)? {
            if !self.node_exists(KBD_BACKLIGHT_MAX)? {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "keyboard backlight max level".into(),
                });
            }
            return Ok((
                PathBuf::from(KBD_BACKLIGHT),
                PathBuf::from(KBD_BACKLIGHT_MAX),
            ));
        }
        for brightness in KBD_BACKLIGHT_LEDS {
            if self.node_exists(brightness)? {
                let max = Path::new(brightness)
                    .parent()
                    .ok_or_else(|| LctrlError::ChannelUnavailable {
                        channel: format!("LED brightness path {brightness:?} has no parent"),
                    })?
                    .join("max_brightness");
                if self.node_exists(&max)? {
                    return Ok((PathBuf::from(brightness), max));
                }
                return Err(LctrlError::ChannelUnavailable {
                    channel: "keyboard LED max_brightness".into(),
                });
            }
        }
        Err(LctrlError::Unsupported {
            feature: "kbd.backlight".into(),
        })
    }

    fn read_backlight_state(&self) -> Result<BacklightState> {
        let (brightness, max) = self.backlight_nodes()?;
        let level_raw = parse_unsigned(&self.read_required(&brightness)?, "kbd backlight")?;
        let max_raw = parse_unsigned(&self.read_required(&max)?, "kbd backlight max")?;
        let level = u8::try_from(level_raw)
            .map_err(|_| malformed("kbd backlight", &level_raw.to_string()))?;
        let max_level = u8::try_from(max_raw)
            .map_err(|_| malformed("kbd backlight max", &max_raw.to_string()))?;
        BacklightState::new(level, max_level, LightingEffect::Static)
    }

    fn read_performance_mode(&self) -> Result<PerformanceMode> {
        if !self.node_exists(THERMAL_MODE)? {
            return Err(LctrlError::Unsupported {
                feature: "perf.mode".into(),
            });
        }
        match parse_switch_raw(&self.read_required(THERMAL_MODE)?, THERMAL_MODE)? {
            0 => Ok(PerformanceMode::Quiet),
            1 => Ok(PerformanceMode::Balanced),
            2 => Ok(PerformanceMode::Performance),
            3 => Ok(PerformanceMode::SilentHighPerformance),
            4 => Ok(PerformanceMode::Custom),
            raw => Err(LctrlError::ChannelUnavailable {
                channel: format!("unsupported Linux thermal_mode value {raw}"),
            }),
        }
    }

    fn fan_mode_path(&self) -> Result<PathBuf> {
        if self.node_exists(IDEAPAD_FAN_MODE)? {
            return Ok(self.path(IDEAPAD_FAN_MODE));
        }
        let root = self.path(HWMON_ROOT);
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(LctrlError::Unsupported {
                    feature: "perf.fan.mode".into(),
                });
            }
            Err(error) => return Err(map_io_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            let name = fs::read_to_string(entry.path().join("name")).unwrap_or_default();
            let candidate = entry.path().join("fan_mode");
            if name.trim().starts_with("ideapad") && self.node_exists_path(&candidate)? {
                return Ok(candidate);
            }
        }
        Err(LctrlError::Unsupported {
            feature: "perf.fan.mode".into(),
        })
    }

    fn read_fan_mode(&self) -> Result<FanMode> {
        let value = fs::read_to_string(self.fan_mode_path()?).map_err(map_io_error)?;
        match value.trim() {
            "balanced" | "standard" | "auto" => Ok(FanMode::Standard),
            "quiet" | "silent" => Ok(FanMode::Silent),
            "max" | "performance" | "fullspeed" => Ok(FanMode::Performance),
            "custom" | "manual" => Ok(FanMode::Custom),
            other => Err(LctrlError::ChannelUnavailable {
                channel: format!("unrecognized Linux fan_mode {other:?}"),
            }),
        }
    }

    fn epp_raw(&self) -> Result<u8> {
        let paths = self.epp_paths()?;
        let mut value: Option<u8> = None;
        for path in paths {
            let token = fs::read_to_string(path).map_err(map_io_error)?;
            let raw =
                epp_raw_for_token(token.trim()).ok_or_else(|| LctrlError::ChannelUnavailable {
                    channel: format!("unrecognized Linux EPP value {:?}", token.trim()),
                })?;
            if value.is_some_and(|current| current != raw) {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "CPU policies report different EPP values".into(),
                });
            }
            value = Some(raw);
        }
        value.ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "no CPU EPP policy was readable".into(),
        })
    }

    fn rapl_limit_path(&self, kind: PowerLimitKind, writable: bool) -> Result<PathBuf> {
        let (names, feature) = match kind {
            PowerLimitKind::Pl1 => (
                ["constraint_0_power_limit_uw", "constraint_0_max_power_uw"],
                "tune.pl1",
            ),
            PowerLimitKind::Pl2 => (
                ["constraint_1_power_limit_uw", "constraint_1_max_power_uw"],
                "tune.pl2",
            ),
            PowerLimitKind::Tau => (
                ["constraint_0_time_window_us", "constraint_1_time_window_us"],
                "tune.tau",
            ),
        };
        for root in RAPL_ROOTS {
            for (index, name) in names.into_iter().enumerate() {
                if writable && index == 1 {
                    continue;
                }
                let path = self.path(Path::new(root).join(name));
                if self.node_exists_path(&path)? {
                    return Ok(path);
                }
            }
        }
        Err(LctrlError::Unsupported {
            feature: feature.into(),
        })
    }

    fn read_rapl_limit(&self, kind: PowerLimitKind) -> Result<u64> {
        let path = self.rapl_limit_path(kind, false)?;
        let value = parse_unsigned(&self.read_required_path(path)?, "RAPL limit")?;
        u64::try_from(value).map_err(|_| LctrlError::FirmwareRejected {
            detail: "RAPL limit exceeds u64".into(),
        })
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

    fn epp_paths(&self) -> Result<Vec<PathBuf>> {
        let root = self.path(CPUFREQ_ROOT);
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(LctrlError::Unsupported {
                    feature: "power.scheme".into(),
                });
            }
            Err(error) => return Err(map_io_error(error)),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(map_io_error)?;
            if !entry.file_name().to_string_lossy().starts_with("policy") {
                continue;
            }
            let path = entry.path().join(EPP_FILE);
            if self.node_exists_path(&path)? {
                paths.push(path);
            }
        }
        paths.sort();
        if paths.is_empty() {
            Err(LctrlError::Unsupported {
                feature: "power.scheme".into(),
            })
        } else {
            Ok(paths)
        }
    }

    fn active_epp_scheme_id(&self) -> Result<PowerSchemeId> {
        let paths = self.epp_paths()?;
        let mut active: Option<&'static str> = None;
        for path in paths {
            let raw = fs::read_to_string(path).map_err(map_io_error)?;
            let scheme =
                epp_scheme_for_token(raw.trim()).ok_or_else(|| LctrlError::ChannelUnavailable {
                    channel: format!("unrecognized Linux EPP value {:?}", raw.trim()),
                })?;
            if active.is_some_and(|current| current != scheme) {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "CPU policies report different EPP power schemes".into(),
                });
            }
            active = Some(scheme);
        }
        PowerSchemeId::new(active.ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "no CPU EPP policy was readable".into(),
        })?)
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
        let charge_mode_available =
            self.node_exists(CONSERVATION_MODE)? && self.node_exists(FAST_CHARGE)?;
        capabilities.record(
            "battery.charge_mode",
            if charge_mode_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!charge_mode_available)
                .then(|| "both conservation_mode and fast_charge sysfs nodes are required".into()),
        )?;

        let ideapad_backlight = self.node_exists(KBD_BACKLIGHT)?;
        let mut led_backlight = false;
        for led in KBD_BACKLIGHT_LEDS {
            if self.node_exists(led)? {
                led_backlight = true;
                break;
            }
        }

        match self.epp_paths() {
            Ok(_) => {
                capabilities.record("power.scheme", Availability::Available, None)?;
            }
            Err(error) => {
                capabilities.record(
                    "power.scheme",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
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
        self.record_node(&mut capabilities, "perf.mode", THERMAL_MODE)?;
        match self.fan_mode_path() {
            Ok(_) => {
                capabilities.record("perf.fan.mode", Availability::Available, None)?;
            }
            Err(error) => {
                capabilities.record(
                    "perf.fan.mode",
                    Availability::Unavailable,
                    Some(error.to_string()),
                )?;
            }
        }
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
        for (feature, kind) in [
            ("tune.pl1", PowerLimitKind::Pl1),
            ("tune.pl2", PowerLimitKind::Pl2),
            ("tune.tau", PowerLimitKind::Tau),
        ] {
            let available = self.rapl_limit_path(kind, true).is_ok();
            capabilities.record(
                feature,
                if available {
                    Availability::Available
                } else {
                    Availability::Unavailable
                },
                (!available).then(|| "no writable Intel RAPL constraint sysfs node".into()),
            )?;
        }

        let drm_modes = self.drm_modes_exists()?;
        capabilities.record(
            "panel.refresh",
            Availability::Unavailable,
            Some(if drm_modes {
                "DRM connector modes detected; no verified mode mutator is exposed".into()
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
        let epp_available = self.epp_paths().is_ok();
        capabilities.record(
            "tune.epp",
            if epp_available {
                Availability::Available
            } else {
                Availability::Unavailable
            },
            (!epp_available).then(|| "Linux EPP sysfs policy is unavailable".into()),
        )?;
        for (feature, detail) in [
            ("tune.tau", "no verified Linux RAPL tau mutator"),
            ("tune.turbo", "no verified Linux turbo mutator"),
            ("gpu.mode", "no verified Linux GPU-mode mutator"),
            ("tune.background", "no process-policy tuning executor"),
        ] {
            Self::record_fixed_unsupported(&mut capabilities, feature, detail)?;
        }

        Ok(capabilities)
    }
}

impl PowerControl for LinuxHal {
    fn power_schemes(&self) -> Result<Vec<PowerScheme>> {
        let active = self.active_epp_scheme_id()?;
        [
            ("power-saver", "Power saver"),
            ("balanced", "Balanced"),
            ("performance", "Performance"),
        ]
        .into_iter()
        .map(|(id, name)| {
            let id = PowerSchemeId::new(id)?;
            let is_active = id == active;
            Ok(PowerScheme::new(id, name, is_active))
        })
        .collect()
    }

    fn active_power_scheme(&self) -> Result<PowerScheme> {
        self.power_schemes()?
            .into_iter()
            .find(|scheme| scheme.active)
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "no active Linux EPP power scheme".into(),
            })
    }

    fn power_value_range(&self, _key: &lctrl_core::PowerSettingKey) -> Result<PowerValueRange> {
        Err(LctrlError::Unsupported {
            feature: "power.scheme.setting".into(),
        })
    }

    fn apply_power_mutation(
        &self,
        mutation: PowerMutation,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>> {
        let requested_id = match &mutation {
            PowerMutation::Activate(requested_id) => requested_id.clone(),
            PowerMutation::SetValue { .. } => {
                return Err(LctrlError::Unsupported {
                    feature: "power.scheme.setting".into(),
                });
            }
        };
        let requested_token = epp_token_for_scheme(requested_id.as_str()).ok_or_else(|| {
            LctrlError::InvalidArgument {
                detail: format!("unknown Linux power scheme {:?}", requested_id.as_str()),
            }
        })?;
        let previous_id = self.active_epp_scheme_id()?;
        let previous = PowerMutation::Activate(previous_id.clone());
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mutation));
        }

        let paths = self.epp_paths()?;
        let mut previous_values = Vec::with_capacity(paths.len());
        for path in &paths {
            previous_values.push((
                path.clone(),
                fs::read_to_string(path).map_err(map_io_error)?,
            ));
        }
        for (index, path) in paths.iter().enumerate() {
            if let Err(error) = fs::write(path, requested_token).map_err(map_io_error) {
                return Err(power_write_with_rollback(error, &previous_values[..=index]));
            }
        }

        let readback = poll_readback(&requested_id, 10, Duration::from_millis(50), || {
            self.active_epp_scheme_id()
        });
        let actual_id = match readback {
            Ok(actual) => actual,
            Err(error) => return Err(power_write_with_rollback(error, &previous_values)),
        };
        Ok(ChangeReport::committed(
            previous,
            mutation,
            PowerMutation::Activate(actual_id),
        ))
    }
}

impl PerformanceControl for LinuxHal {
    fn performance_state(&self) -> Result<PerformanceState> {
        let mode = self.read_performance_mode()?;
        Ok(PerformanceState {
            requested: Some(mode),
            active: Some(mode),
            automatic: false,
            version: DispatcherVersion::Legacy(0),
            capabilities: PerformanceCapabilities::new(0x0b),
        })
    }

    fn set_performance_mode(
        &self,
        mode: PerformanceMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>> {
        let requested_raw = linux_thermal_mode(mode)?;
        let previous = self.read_performance_mode()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }
        let path = self.path(THERMAL_MODE);
        if let Err(error) = fs::write(&path, requested_raw.to_string()).map_err(map_io_error) {
            return Err(peripheral_write_with_rollback(
                error,
                &path,
                &linux_thermal_mode(previous)?.to_string(),
                "performance-mode",
            ));
        }
        match poll_readback(&mode, 10, Duration::from_millis(50), || {
            self.read_performance_mode()
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, mode, actual)),
            Err(error) => {
                let rollback = linux_thermal_mode(previous).and_then(|raw| {
                    fs::write(self.path(THERMAL_MODE), raw.to_string()).map_err(map_io_error)?;
                    poll_readback(&previous, 10, Duration::from_millis(50), || {
                        self.read_performance_mode()
                    })?;
                    Ok(())
                });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "performance-mode write failed ({error}); restoring {previous} also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}
impl FanControl for LinuxHal {
    fn fan_mode(&self) -> Result<FanMode> {
        self.read_fan_mode()
    }

    fn set_fan_mode(&self, mode: FanMode, apply: ApplyMode) -> Result<ChangeReport<FanMode>> {
        let token = linux_fan_mode_token(mode)?;
        let previous = self.read_fan_mode()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }
        let path = self.fan_mode_path()?;
        if let Err(error) = fs::write(&path, token).map_err(map_io_error) {
            return Err(peripheral_write_with_rollback(
                error,
                &path,
                linux_fan_mode_token(previous)?,
                "fan-mode",
            ));
        }
        match poll_readback(&mode, 10, Duration::from_millis(50), || {
            self.read_fan_mode()
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, mode, actual)),
            Err(error) => {
                let rollback = linux_fan_mode_token(previous).and_then(|token| {
                    fs::write(&path, token).map_err(map_io_error)?;
                    poll_readback(&previous, 10, Duration::from_millis(50), || {
                        self.read_fan_mode()
                    })?;
                    Ok(())
                });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "fan-mode write failed ({error}); restoring the prior mode also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }

    fn fans(&self) -> Result<Vec<FanDescriptor>> {
        Err(LctrlError::Unsupported {
            feature: "perf.fan.inventory".into(),
        })
    }

    fn fan_table(&self, _fan: FanId, _sensor: SensorId) -> Result<FanTable> {
        Err(LctrlError::Unsupported {
            feature: "perf.fan.curve".into(),
        })
    }
}

impl TuningControl for LinuxHal {
    fn epp(&self) -> Result<u8> {
        self.epp_raw()
    }

    fn set_epp(&self, value: u8, apply: ApplyMode) -> Result<ChangeReport<u8>> {
        let previous = self.epp_raw()?;
        let token = epp_token_for_raw(value).ok_or_else(|| LctrlError::InvalidArgument {
            detail: format!(
                "Linux EPP value {value} has no verified sysfs token; use 0, 128, 192, or 255"
            ),
        })?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, value));
        }
        let paths = self.epp_paths()?;
        let mut previous_values = Vec::with_capacity(paths.len());
        for path in &paths {
            previous_values.push((
                path.clone(),
                fs::read_to_string(path).map_err(map_io_error)?,
            ));
        }
        for (index, path) in paths.iter().enumerate() {
            if let Err(error) = fs::write(path, token).map_err(map_io_error) {
                return Err(power_write_with_rollback(error, &previous_values[..=index]));
            }
        }
        match poll_readback(&value, 10, Duration::from_millis(50), || self.epp_raw()) {
            Ok(actual) => Ok(ChangeReport::committed(previous, value, actual)),
            Err(error) => Err(power_write_with_rollback(error, &previous_values)),
        }
    }

    fn power_limit(&self, kind: PowerLimitKind) -> Result<u64> {
        self.read_rapl_limit(kind)
    }

    fn set_power_limit(
        &self,
        kind: PowerLimitKind,
        value: u64,
        apply: ApplyMode,
    ) -> Result<ChangeReport<u64>> {
        let previous = self.read_rapl_limit(kind)?;
        if matches!(kind, PowerLimitKind::Tau) && value == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: "RAPL tau must be nonzero".into(),
            });
        }
        let pl1 = if kind == PowerLimitKind::Pl1 {
            value
        } else {
            self.read_rapl_limit(PowerLimitKind::Pl1)?
        };
        let pl2 = if kind == PowerLimitKind::Pl2 {
            value
        } else {
            self.read_rapl_limit(PowerLimitKind::Pl2)?
        };
        if pl1 > pl2 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("RAPL safety invariant PL1 <= PL2 violated: {pl1} > {pl2} µW"),
            });
        }
        if pl1 < 7_000_000 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("RAPL PL1 below documented 7 W safety floor: {pl1} µW"),
            });
        }
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, value));
        }
        let path = self.rapl_limit_path(kind, true)?;
        let previous_text = fs::read_to_string(&path).map_err(map_io_error)?;
        if let Err(error) = fs::write(&path, value.to_string()).map_err(map_io_error) {
            return Err(peripheral_write_with_rollback(
                error,
                &path,
                &previous_text,
                "RAPL limit",
            ));
        }
        match poll_readback(&value, 10, Duration::from_millis(50), || {
            self.read_rapl_limit(kind)
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, value, actual)),
            Err(error) => Err(peripheral_write_with_rollback(
                error,
                &path,
                &previous_text,
                "RAPL limit",
            )),
        }
    }
}

impl TemperatureControl for LinuxHal {
    fn temperature_sensors(&self) -> Result<Vec<TemperatureSensor>> {
        let mut sensors = Vec::new();
        collect_hwmon_temperatures(self, &mut sensors)?;
        collect_thermal_zone_temperatures(self, &mut sensors)?;
        sensors.sort_by(|left, right| left.metadata.id.cmp(&right.metadata.id));
        if sensors.is_empty() {
            return Err(LctrlError::Unsupported {
                feature: "perf.temp".into(),
            });
        }
        Ok(sensors)
    }

    fn temperature(&self, id: &str) -> Result<TemperatureSensor> {
        self.temperature_sensors()?
            .into_iter()
            .find(|sensor| sensor.metadata.id == id)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("temperature sensor {id:?} was not found"),
            })
    }
}

impl ControlConflictDetection for LinuxHal {
    fn active_vendor_controllers(&self) -> Result<Vec<String>> {
        let mut controllers = Vec::new();
        for entry in fs::read_dir(self.path("proc")).map_err(map_io_error)? {
            let entry = entry.map_err(map_io_error)?;
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            let cmdline = fs::read(entry.path().join("cmdline"))
                .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
                .unwrap_or_default();
            let identity = format!("{comm} {cmdline}");
            if vendor_controller_name(&identity) {
                let name = comm.trim();
                controllers.push(if name.is_empty() {
                    cmdline
                        .split_whitespace()
                        .next()
                        .unwrap_or("unknown vendor controller")
                        .to_owned()
                } else {
                    name.to_owned()
                });
            }
        }
        controllers.sort();
        controllers.dedup();
        Ok(controllers)
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

        let voltage_now_text = self.battery_value(index, "voltage_now")?;
        let voltage_now_uv = voltage_now_text
            .as_deref()
            .map(|value| parse_unsigned(value, "voltage_now"))
            .transpose()?;
        let voltage_now = voltage_now_text
            .as_deref()
            .map(|value| parse_voltage(value, "voltage_now"))
            .transpose()?;
        let design_voltage_text = self.battery_value(index, "voltage_min_design")?;
        let design_voltage_uv = design_voltage_text
            .as_deref()
            .map(|value| parse_unsigned(value, "voltage_min_design"))
            .transpose()?;
        let design_voltage = design_voltage_text
            .as_deref()
            .map(|value| parse_voltage(value, "voltage_min_design"))
            .transpose()?;
        let design_capacity_mwh = energy_or_charge(
            self.battery_value(index, "energy_full_design")?,
            self.battery_value(index, "charge_full_design")?,
            design_voltage_uv.or(voltage_now_uv),
            "full_design",
        )?;
        let full_charge_capacity_mwh = energy_or_charge(
            self.battery_value(index, "energy_full")?,
            self.battery_value(index, "charge_full")?,
            design_voltage_uv.or(voltage_now_uv),
            "full",
        )?;
        let remaining_capacity_mwh = energy_or_charge(
            self.battery_value(index, "energy_now")?,
            self.battery_value(index, "charge_now")?,
            voltage_now_uv.or(design_voltage_uv),
            "now",
        )?;
        let current_now = self
            .battery_value(index, "current_now")?
            .map(|value| parse_current(&value, "current_now"))
            .transpose()?;
        let temperature = self
            .battery_value(index, "temp")?
            .map(|value| parse_temperature(&value, "temp"))
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
        let health = self
            .battery_value(index, "health")?
            .and_then(|value| parse_battery_health(&value));
        let wattage_w = self
            .battery_value(index, "power_now")?
            .map(|value| parse_power(&value, "power_now"))
            .transpose()?;
        let life_percent = match (full_charge_capacity_mwh, design_capacity_mwh) {
            (Some(full), Some(design)) if design > 0 => {
                u16::try_from(u64::from(full) * 100 / u64::from(design)).ok()
            }
            _ => None,
        };

        Ok(BatteryTelemetry {
            design_capacity_mwh,
            full_charge_capacity_mwh,
            remaining_capacity_mwh,
            voltage_mv: voltage_now,
            current_ma: current_now,
            temperature_deci_kelvin: temperature,
            manufacture_date: None,
            first_used_date: None,
            design_voltage_mv: design_voltage,
            remaining_percent,
            life_percent,
            charge_status,
            health,
            remaining_time_min: None,
            charge_completion_time_min: None,
            wattage_w,
            cycle_count,
            manufacturer: battery_text(self.battery_value(index, "manufacturer")?),
            model_name: battery_text(self.battery_value(index, "model_name")?),
            firmware_version: battery_text(self.battery_value(index, "firmware_version")?),
            serial_number: battery_text(self.battery_value(index, "serial_number")?),
            chemistry: battery_text(self.battery_value(index, "technology")?),
        })
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        let connected = self.ac_online()?.ok_or_else(|| LctrlError::Unsupported {
            feature: "battery.adapter".into(),
        })?;
        // Linux power_supply has no portable equivalent of the Lenovo GBMD
        // authentication or connector-type words. Preserve those as unknown.
        Ok(AdapterInfo {
            ac_connected: Some(connected),
            status: Some(if connected {
                AdapterStatus::UnsupportedDetection
            } else {
                AdapterStatus::Disconnected
            }),
            connector_type: None,
            wattage_w: None,
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
        if mode == ChargeMode::Rapid && !self.battery_telemetry(0)?.rapid_charge_allowed() {
            return Err(LctrlError::Unsupported {
                feature: "battery.rapid_charge".into(),
            });
        }
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }

        let result = (|| {
            // The opposite feature is disabled before enabling the request.
            for primitive in plan_charge_mode(mode) {
                self.write_primitive(primitive)?;
            }
            poll_readback(&mode, 10, Duration::from_millis(50), || {
                self.mode_for_write()
            })
        })();
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, mode, actual)),
            Err(error) if matches!(&error, LctrlError::PermissionDenied { .. }) => Err(error),
            Err(error) => match self.commit_charge_mode(previous) {
                Ok(_) => Err(error),
                Err(rollback) => Err(LctrlError::FirmwareRejected {
                    detail: format!(
                        "charge-mode transition failed ({error}); restoring {previous} also failed ({rollback})"
                    ),
                }),
            },
        }
    }
}

impl KeyboardControl for LinuxHal {
    fn backlight_state(&self) -> Result<BacklightState> {
        self.read_backlight_state()
    }

    fn set_backlight(
        &self,
        level: u8,
        effect: LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>> {
        if !matches!(effect, LightingEffect::Static) {
            return Err(LctrlError::Unsupported {
                feature: "kbd.backlight.effect".into(),
            });
        }
        let previous = self.read_backlight_state()?;
        let requested = BacklightState::new(level, previous.max_level, effect)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let (brightness, _) = self.backlight_nodes()?;
        let requested_text = level.to_string();
        if let Err(error) = fs::write(self.path(&brightness), &requested_text).map_err(map_io_error)
        {
            return Err(peripheral_write_with_rollback(
                error,
                &self.path(&brightness),
                &previous.level.to_string(),
                "keyboard-backlight",
            ));
        }
        match poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_backlight_state()
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => {
                let rollback = fs::write(self.path(&brightness), previous.level.to_string())
                    .map_err(map_io_error)
                    .and_then(|()| {
                        poll_readback(&previous, 10, Duration::from_millis(50), || {
                            self.read_backlight_state()
                        })
                        .map(|_| ())
                    });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "keyboard backlight write failed ({error}); rollback also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}

impl TouchpadControl for LinuxHal {
    fn touchpad_state(&self) -> Result<DeviceState> {
        if !self.node_exists(TOUCHPAD)? {
            return Err(LctrlError::Unsupported {
                feature: "touchpad".into(),
            });
        }
        let enabled = parse_switch(&self.read_required(TOUCHPAD)?, TOUCHPAD)?;
        Ok(if enabled {
            DeviceState::Enabled
        } else {
            DeviceState::Disabled
        })
    }

    fn set_touchpad(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let previous = self.touchpad_state()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, state));
        }
        let raw = if matches!(state, DeviceState::Enabled) {
            "1"
        } else {
            "0"
        };
        if let Err(error) = fs::write(self.path(TOUCHPAD), raw).map_err(map_io_error) {
            let previous_raw = if previous == DeviceState::Enabled {
                "1"
            } else {
                "0"
            };
            return Err(peripheral_write_with_rollback(
                error,
                &self.path(TOUCHPAD),
                previous_raw,
                "touchpad",
            ));
        }
        match poll_readback(&state, 10, Duration::from_millis(50), || {
            self.touchpad_state()
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, state, actual)),
            Err(error) => {
                let previous_raw = if previous == DeviceState::Enabled {
                    "1"
                } else {
                    "0"
                };
                let rollback = fs::write(self.path(TOUCHPAD), previous_raw)
                    .map_err(map_io_error)
                    .and_then(|()| {
                        poll_readback(&previous, 10, Duration::from_millis(50), || {
                            self.touchpad_state()
                        })
                        .map(|_| ())
                    });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "touchpad write failed ({error}); rollback also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}

impl PrivacyControl for LinuxHal {
    fn camera_state(&self) -> Result<DeviceState> {
        if !self.node_exists(CAMERA_POWER)? {
            return Err(LctrlError::Unsupported {
                feature: "privacy.cam.runtime".into(),
            });
        }
        let enabled = parse_switch(&self.read_required(CAMERA_POWER)?, CAMERA_POWER)?;
        Ok(if enabled {
            DeviceState::Enabled
        } else {
            DeviceState::Disabled
        })
    }

    fn set_camera(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let previous = self.camera_state()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, state));
        }
        let raw = if matches!(state, DeviceState::Enabled) {
            "1"
        } else {
            "0"
        };
        if let Err(error) = fs::write(self.path(CAMERA_POWER), raw).map_err(map_io_error) {
            let previous_raw = if previous == DeviceState::Enabled {
                "1"
            } else {
                "0"
            };
            return Err(peripheral_write_with_rollback(
                error,
                &self.path(CAMERA_POWER),
                previous_raw,
                "runtime camera",
            ));
        }
        match poll_readback(&state, 10, Duration::from_millis(50), || {
            self.camera_state()
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, state, actual)),
            Err(error) => {
                let previous_raw = if previous == DeviceState::Enabled {
                    "1"
                } else {
                    "0"
                };
                let rollback = fs::write(self.path(CAMERA_POWER), previous_raw)
                    .map_err(map_io_error)
                    .and_then(|()| {
                        poll_readback(&previous, 10, Duration::from_millis(50), || {
                            self.camera_state()
                        })
                        .map(|_| ())
                    });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "runtime camera write failed ({error}); rollback also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}

impl MagicBayControl for LinuxHal {
    fn detect_magicbay(&self) -> Result<MagicBayInventory> {
        let mut inventory = MagicBayInventory::default();
        let usb_root = self.path(USB_DEVICES_ROOT);
        match fs::read_dir(&usb_root) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(map_io_error)?;
                    let entry_name = entry.file_name().to_string_lossy().into_owned();
                    let path = entry.path();
                    let Some(vid) = read_hex_u16(path.join("idVendor"))? else {
                        continue;
                    };
                    if vid != lctrl_core::MAGICBAY_VENDOR_ID {
                        continue;
                    }
                    let Some(pid) = read_hex_u16(path.join("idProduct"))? else {
                        continue;
                    };
                    let kind = identify_magicbay(vid, pid)
                        .map_or(MagicBayKind::Unknown, |known| known.kind);
                    let interfaces =
                        if pid == 0x7005 && usb_root.join(format!("{entry_name}:1.0")).is_dir() {
                            vec!["mbim".into()]
                        } else {
                            Vec::new()
                        };
                    inventory.devices.push(MagicBayDevice {
                        path: path.display().to_string(),
                        bus: "usb".into(),
                        vid: Some(vid),
                        pid: Some(pid),
                        kind,
                        interfaces,
                        attached: true,
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }

        let acpi_root = self.path(ACPI_DEVICES_ROOT);
        match fs::read_dir(&acpi_root) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(map_io_error)?;
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    let (kind, interface) = if name.starts_with("QCOM2488") {
                        (MagicBayKind::DisplayBridge, "display")
                    } else if name.starts_with("QCOM24B7") {
                        (MagicBayKind::UsbRoleSwitch, "usb_role_switch")
                    } else {
                        continue;
                    };
                    inventory.acpi_devices.push(MagicBayDevice {
                        path: entry.path().display().to_string(),
                        bus: "acpi".into(),
                        vid: None,
                        pid: None,
                        kind,
                        interfaces: vec![interface.into()],
                        attached: true,
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(map_io_error(error)),
        }

        inventory
            .devices
            .sort_by(|left, right| left.path.cmp(&right.path));
        inventory
            .acpi_devices
            .sort_by(|left, right| left.path.cmp(&right.path));
        Ok(inventory)
    }
}
fn epp_raw_for_token(token: &str) -> Option<u8> {
    match token {
        "performance" => Some(0),
        "default" | "balance_performance" => Some(128),
        "balance_power" => Some(192),
        "power" => Some(255),
        _ => None,
    }
}
fn epp_token_for_raw(value: u8) -> Option<&'static str> {
    match value {
        0 => Some("performance"),
        128 => Some("balance_performance"),
        192 => Some("balance_power"),
        255 => Some("power"),
        _ => None,
    }
}

fn read_hex_u16(path: PathBuf) -> Result<Option<u16>> {
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io_error(error)),
    };
    u16::from_str_radix(value.trim(), 16)
        .map(Some)
        .map_err(|_| malformed("USB hexadecimal identifier", &value))
}

fn collect_hwmon_temperatures(hal: &LinuxHal, sensors: &mut Vec<TemperatureSensor>) -> Result<()> {
    let root = hal.path(HWMON_ROOT);
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(map_io_error(error)),
    };
    for entry in entries {
        let entry = entry.map_err(map_io_error)?;
        let chip = entry.file_name().to_string_lossy().into_owned();
        let name = hal
            .read_optional_path(entry.path().join("name"))?
            .and_then(|value| non_empty_trimmed(&value))
            .unwrap_or_else(|| chip.clone());
        for index in 1..=32 {
            let input = entry.path().join(format!("temp{index}_input"));
            if !hal.node_exists_path(&input)? {
                continue;
            }
            let raw = parse_signed(&hal.read_required_path(&input)?, "hwmon temperature")?;
            let label = hal
                .read_optional_path(entry.path().join(format!("temp{index}_label")))?
                .and_then(|value| non_empty_trimmed(&value))
                .unwrap_or_else(|| name.clone());
            let id = format!("hwmon/{chip}/temp{index}");
            sensors.push(TemperatureSensor {
                metadata: TemperatureSensorMetadata {
                    id,
                    name: label.clone(),
                    source: TemperatureSource::Sysfs,
                    location: temperature_location(&label),
                    availability: Availability::Available,
                },
                value_c: Some(raw as f32 / 1000.0),
            });
        }
    }
    Ok(())
}

fn collect_thermal_zone_temperatures(
    hal: &LinuxHal,
    sensors: &mut Vec<TemperatureSensor>,
) -> Result<()> {
    let root = hal.path("sys/class/thermal");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(map_io_error(error)),
    };
    for entry in entries {
        let entry = entry.map_err(map_io_error)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("thermal_zone")
        {
            continue;
        }
        let input = entry.path().join("temp");
        if !hal.node_exists_path(&input)? {
            continue;
        }
        let label = hal
            .read_optional_path(entry.path().join("type"))?
            .and_then(|value| non_empty_trimmed(&value))
            .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
        let raw = parse_signed(&hal.read_required_path(&input)?, "thermal zone temperature")?;
        let id = format!("thermal/{}", entry.file_name().to_string_lossy());
        sensors.push(TemperatureSensor {
            metadata: TemperatureSensorMetadata {
                id,
                name: label.clone(),
                source: TemperatureSource::Sysfs,
                location: temperature_location(&label),
                availability: Availability::Available,
            },
            value_c: Some(raw as f32 / 1000.0),
        });
    }
    Ok(())
}

fn temperature_location(label: &str) -> TemperatureLocation {
    let label = label.to_ascii_lowercase();
    if label.contains("cpu") || label.contains("core") || label.contains("package") {
        TemperatureLocation::Cpu
    } else if label.contains("gpu") || label.contains("graphics") {
        TemperatureLocation::Gpu
    } else if label.contains("battery") || label.contains("bat") {
        TemperatureLocation::Battery
    } else if label.contains("board") || label.contains("mother") {
        TemperatureLocation::Mainboard
    } else {
        TemperatureLocation::Unknown
    }
}

impl DiagnosticsControl for LinuxHal {
    fn diagnostic_items(&self) -> Result<Vec<DiagnosticKind>> {
        Ok(vec![
            DiagnosticKind::Battery,
            DiagnosticKind::Thermal,
            DiagnosticKind::Storage,
            DiagnosticKind::Memory,
            DiagnosticKind::Firmware,
            DiagnosticKind::Network,
        ])
    }

    fn run_diagnostics(&self, items: &[DiagnosticKind]) -> Result<Vec<DiagnosticResult>> {
        items
            .iter()
            .copied()
            .map(|kind| run_diagnostic(self, kind))
            .collect()
    }
}

fn run_diagnostic(hal: &LinuxHal, kind: DiagnosticKind) -> Result<DiagnosticResult> {
    match kind {
        DiagnosticKind::Battery => match hal.battery_telemetry(0) {
            Ok(telemetry) => Ok(DiagnosticResult {
                kind,
                outcome: DiagnosticOutcome::Warning,
                detail: format!(
                    "power_supply telemetry parsed (design={:?}, remaining={:?}); deep vendor-driver tests are excluded",
                    telemetry.design_capacity_mwh, telemetry.remaining_capacity_mwh
                ),
            }),
            Err(error) => Ok(DiagnosticResult {
                kind,
                outcome: DiagnosticOutcome::Unavailable,
                detail: error.to_string(),
            }),
        },
        DiagnosticKind::Thermal => match hal.temperature_sensors() {
            Ok(sensors) => {
                let valid = sensors.iter().all(|sensor| {
                    sensor
                        .value_c
                        .is_some_and(|value| value.is_finite() && (-40.0..=150.0).contains(&value))
                });
                Ok(DiagnosticResult {
                    kind,
                    outcome: if valid {
                        DiagnosticOutcome::Warning
                    } else {
                        DiagnosticOutcome::Unavailable
                    },
                    detail: format!(
                        "{} sysfs temperature sensor(s) parsed; vendor stress tests are excluded",
                        sensors.len()
                    ),
                })
            }
            Err(error) => Ok(DiagnosticResult {
                kind,
                outcome: DiagnosticOutcome::Unavailable,
                detail: error.to_string(),
            }),
        },
        DiagnosticKind::Storage => {
            let count = inventory_count(hal, "sys/block")?;
            Ok(inventory_result(
                kind,
                count,
                "block device",
                "SMART tests are not run automatically",
            ))
        }
        DiagnosticKind::Memory => {
            let meminfo = match hal.read_required("proc/meminfo") {
                Ok(meminfo) => meminfo,
                Err(error) => {
                    return Ok(DiagnosticResult {
                        kind,
                        outcome: DiagnosticOutcome::Unavailable,
                        detail: error.to_string(),
                    });
                }
            };
            let total = meminfo.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse::<u64>().ok())
            });
            Ok(if total.is_some_and(|value| value > 0) {
                DiagnosticResult {
                    kind,
                    outcome: DiagnosticOutcome::Warning,
                    detail: "MemTotal is readable; deep vendor-driver diagnostics are excluded; destructive memtest is not run automatically".into(),
                }
            } else {
                DiagnosticResult {
                    kind,
                    outcome: DiagnosticOutcome::Unavailable,
                    detail: "MemTotal is missing or zero".into(),
                }
            })
        }
        DiagnosticKind::Firmware => {
            let info = hal.hardware_info()?;
            let available =
                info.product_name.is_some() || info.family.is_some() || info.bios_version.is_some();
            Ok(DiagnosticResult {
                kind,
                outcome: if available {
                    DiagnosticOutcome::Warning
                } else {
                    DiagnosticOutcome::Unavailable
                },
                detail: if available {
                    "DMI firmware identity is readable; firmware flashing is excluded".into()
                } else {
                    "DMI firmware identity is unavailable".into()
                },
            })
        }
        DiagnosticKind::Network => {
            let count = inventory_count(hal, "sys/class/net")?;
            Ok(inventory_result(
                kind,
                count,
                "network",
                "connectivity changes are not performed",
            ))
        }
    }
}

fn inventory_count(hal: &LinuxHal, relative: impl AsRef<Path>) -> Result<usize> {
    let root = hal.path(relative);
    match fs::read_dir(root) {
        Ok(entries) => entries
            .map(|entry| entry.map(|_| 1usize).map_err(map_io_error))
            .try_fold(0usize, |total, entry| entry.map(|value| total + value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(map_io_error(error)),
    }
}

fn inventory_result(
    kind: DiagnosticKind,
    count: usize,
    label: &str,
    limitation: &str,
) -> DiagnosticResult {
    DiagnosticResult {
        kind,
        outcome: if count == 0 {
            DiagnosticOutcome::Unavailable
        } else {
            DiagnosticOutcome::Warning
        },
        detail: if count == 0 {
            format!("no {label} records were found")
        } else {
            format!("{count} {label} record(s) inventoried; {limitation}")
        },
    }
}

fn peripheral_write_with_rollback(
    error: LctrlError,
    path: &Path,
    previous: &str,
    channel: &str,
) -> LctrlError {
    match fs::write(path, previous).map_err(map_io_error) {
        Ok(()) => match fs::read_to_string(path).map_err(map_io_error) {
            Ok(actual) if actual == previous => error,
            Ok(actual) => rollback_failure(
                error,
                format!(
                    "{channel} write failed; rollback readback was {actual:?}, expected {previous:?}"
                ),
            ),
            Err(rollback) => rollback_failure(
                error,
                format!("{channel} write failed; rollback readback failed ({rollback})"),
            ),
        },
        Err(rollback) => rollback_failure(
            error,
            format!("{channel} write failed; rollback also failed ({rollback})"),
        ),
    }
}
fn power_write_with_rollback(error: LctrlError, previous: &[(PathBuf, String)]) -> LctrlError {
    for (path, value) in previous.iter().rev() {
        if let Err(rollback) = fs::write(path, value).map_err(map_io_error) {
            return rollback_failure(
                error,
                format!("Linux power-scheme write failed; rollback also failed ({rollback})"),
            );
        }
        match fs::read_to_string(path).map_err(map_io_error) {
            Ok(actual) if actual == *value => {}
            Ok(actual) => {
                return rollback_failure(
                    error,
                    format!(
                        "Linux power-scheme write failed; rollback readback was {actual:?}, expected {value:?}"
                    ),
                );
            }
            Err(rollback) => {
                return rollback_failure(
                    error,
                    format!(
                        "Linux power-scheme write failed; rollback readback failed ({rollback})"
                    ),
                );
            }
        }
    }
    error
}

fn rollback_failure(error: LctrlError, detail: String) -> LctrlError {
    if matches!(error, LctrlError::PermissionDenied { .. }) {
        error
    } else {
        LctrlError::FirmwareRejected { detail }
    }
}

impl UpdateControl for LinuxHal {
    fn update_capability(&self) -> Result<UpdateCapability> {
        Ok(UpdateCapability::Unavailable {
            reason: "no authenticated public update catalog/manifest contract is specified; firmware flashing and private MCP packages are excluded".into(),
        })
    }
}

fn epp_scheme_for_token(token: &str) -> Option<&'static str> {
    match token {
        "power" | "balance_power" => Some("power-saver"),
        "default" | "balance_performance" => Some("balanced"),
        "performance" => Some("performance"),
        _ => None,
    }
}

fn epp_token_for_scheme(scheme: &str) -> Option<&'static str> {
    match scheme {
        "power-saver" => Some("power"),
        "balanced" => Some("balance_performance"),
        "performance" => Some("performance"),
        _ => None,
    }
}

fn vendor_controller_name(identity: &str) -> bool {
    let identity = identity.to_ascii_lowercase();
    [
        "lenovovantage",
        "vantageservice",
        "imcontroller",
        "legionzone",
        "pcmanager",
        "magicenter",
    ]
    .iter()
    .any(|needle| identity.contains(needle))
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

fn linux_thermal_mode(mode: PerformanceMode) -> Result<u32> {
    match mode {
        PerformanceMode::Quiet => Ok(0),
        PerformanceMode::Balanced => Ok(1),
        PerformanceMode::Performance => Ok(2),
        PerformanceMode::SilentHighPerformance => Ok(3),
        PerformanceMode::Custom => Ok(4),
        PerformanceMode::Geek => Err(LctrlError::Unsupported {
            feature: "perf.mode.geek".into(),
        }),
    }
}

fn linux_fan_mode_token(mode: FanMode) -> Result<&'static str> {
    match mode {
        FanMode::Standard => Ok("balanced"),
        FanMode::Silent => Ok("quiet"),
        FanMode::Performance => Ok("max"),
        FanMode::Custom | FanMode::Unknown(_) => Err(LctrlError::Unsupported {
            feature: "perf.fan.mode.custom".into(),
        }),
    }
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

fn energy_or_charge(
    energy: Option<String>,
    charge: Option<String>,
    voltage_uv: Option<u128>,
    field: &str,
) -> Result<Option<u32>> {
    if let Some(energy) = energy {
        return parse_energy(&energy, field).map(Some);
    }
    let (Some(charge), Some(voltage_uv)) = (charge, voltage_uv) else {
        return Ok(None);
    };
    let charge_uah = parse_unsigned(&charge, field)?;
    let milliwatt_hours = charge_uah
        .checked_mul(voltage_uv)
        .ok_or_else(|| malformed(field, &charge))?
        / 1_000_000_000;
    u32::try_from(milliwatt_hours)
        .map(Some)
        .map_err(|_| malformed(field, &charge))
}

fn parse_power(value: &str, field: &str) -> Result<u16> {
    let watts = parse_unsigned(value, field)? / 1_000_000;
    u16::try_from(watts).map_err(|_| malformed(field, value))
}

fn battery_text(value: Option<String>) -> Option<String> {
    value.as_deref().and_then(non_empty_trimmed)
}

fn parse_battery_health(value: &str) -> Option<BatteryHealth> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("good") {
        Some(BatteryHealth::Green)
    } else if ["warm", "cold", "watch"]
        .iter()
        .any(|state| value.eq_ignore_ascii_case(state))
    {
        Some(BatteryHealth::Yellow)
    } else if [
        "overheat",
        "dead",
        "over voltage",
        "failure",
        "unspecified failure",
    ]
    .iter()
    .any(|state| value.eq_ignore_ascii_case(state))
    {
        Some(BatteryHealth::Red)
    } else {
        None
    }
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
