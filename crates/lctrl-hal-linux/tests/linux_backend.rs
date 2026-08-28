use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lctrl_core::{
    AdapterAuthentication, ApplyMode, Availability, BatteryHealth, ChargeMode, ChargeModeActual,
    ChargeStatus, DeviceState, DiagnosticKind, DiagnosticOutcome, FanMode, LctrlError,
    LightingEffect, MagicBayKind, PerformanceMode, Platform, PowerMutation, PowerSchemeId,
    UpdateCapability,
};
use lctrl_hal::{
    BatteryControl, ControlConflictDetection, DiagnosticsControl, FanControl, Hal, KeyboardControl,
    MagicBayControl, PerformanceControl, PowerControl, PowerLimitKind, PrivacyControl,
    TemperatureControl, TouchpadControl, TuningControl, UpdateControl,
};
use lctrl_hal_linux::LinuxHal;

static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!(
            "lctrl-hal-linux-{}-{}-{}",
            std::process::id(),
            NEXT_TREE.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after the epoch")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("create unique temporary tree");
        Self { root }
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: impl AsRef<Path>, value: &str) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, value).expect("write fixture");
    }

    fn hal(&self) -> LinuxHal {
        LinuxHal::with_root(self.root.clone())
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_capability(hal: &LinuxHal, feature: &str, expected: Availability) {
    let capabilities = hal.capabilities().expect("capability probe");
    assert_eq!(
        capabilities
            .get(feature)
            .unwrap_or_else(|| panic!("missing capability {feature}"))
            .availability,
        expected,
        "unexpected availability for {feature}"
    );
}

fn battery_fixture(tree: &TempTree) {
    tree.write("sys/class/power_supply/BAT0/type", "Battery\n");
    tree.write(
        "sys/class/power_supply/BAT0/energy_full_design",
        "51234000\n",
    );
    tree.write("sys/class/power_supply/BAT0/energy_full", "48001000\n");
    tree.write("sys/class/power_supply/BAT0/energy_now", "12345000\n");
    tree.write("sys/class/power_supply/BAT0/voltage_now", "11996000\n");
    tree.write(
        "sys/class/power_supply/BAT0/voltage_min_design",
        "11400000\n",
    );
    tree.write("sys/class/power_supply/BAT0/current_now", "-2345000\n");
    tree.write("sys/class/power_supply/BAT0/temp", "250\n");
    tree.write("sys/class/power_supply/BAT0/capacity", "77\n");
    tree.write("sys/class/power_supply/BAT0/cycle_count", "123\n");
    tree.write("sys/class/power_supply/BAT0/status", "Charging\n");
}

fn ideapad_mode_fixture(tree: &TempTree, conservation: &str, rapid: &str) {
    tree.write(
        "sys/devices/platform/ideapad/conservation_mode",
        conservation,
    );
    tree.write("sys/devices/platform/ideapad/fast_charge", rapid);
}

#[test]
fn linux_hal_reports_platform_and_preserves_absent_dmi_values() {
    let tree = TempTree::new();
    tree.write("sys/class/dmi/id/product_name", "ThinkBook 14 G8+\n");

    let hal = tree.hal();

    assert_eq!(hal.platform(), Platform::Linux);
    let info = hal.hardware_info().expect("hardware info");
    assert_eq!(info.product_name.as_deref(), Some("ThinkBook 14 G8+"));
    assert_eq!(info.family, None);
    assert_eq!(info.bios_version, None);
}

#[test]
fn capability_probe_reports_present_sysfs_channels_and_truthful_dead_ends() {
    let tree = TempTree::new();
    tree.write("sys/class/power_supply/BAT0/status", "Not charging\n");
    tree.write("sys/class/power_supply/AC/online", "1\n");
    tree.write("sys/devices/platform/ideapad/conservation_mode", "0\n");
    tree.write("sys/devices/platform/ideapad/fast_charge", "0\n");
    tree.write("sys/class/leds/laptop:kbd_backlight/brightness", "1\n");
    tree.write("sys/devices/platform/ideapad/touchpad", "1\n");
    tree.write("sys/devices/platform/ideapad/camera_power", "1\n");
    tree.write(
        "sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_max_power_uw",
        "12000000\n",
    );
    tree.write(
        "sys/class/powercap/intel-rapl/intel-rapl:0/constraint_1_max_power_uw",
        "30000000\n",
    );
    tree.write(
        "sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj",
        "987654321\n",
    );
    tree.write("sys/class/drm/card0-eDP-1/modes", "1920x1080\n");

    let hal = tree.hal();
    let capabilities = hal.capabilities().expect("capability probe");

    assert_eq!(capabilities.platform, Platform::Linux);
    for feature in [
        "channel.sysfs",
        "battery.info",
        "battery.status",
        "battery.adapter",
        "battery.conservation",
        "battery.fast_charge",
        "battery.charge_mode",
        "kbd.backlight",
        "touchpad",
        "privacy.camera",
    ] {
        assert_eq!(
            capabilities.get(feature).unwrap().availability,
            Availability::Available,
            "{feature} should be available"
        );
    }
    assert_eq!(
        capabilities.get("tune.rapl").unwrap().availability,
        Availability::Limited
    );
    for feature in ["tune.pl1", "tune.pl2", "panel.refresh"] {
        assert_eq!(
            capabilities.get(feature).unwrap().availability,
            Availability::Unavailable,
            "{feature} should be unavailable without a mutator"
        );
    }
    assert_eq!(
        capabilities.get("battery.thresholds").unwrap().availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("channel.acpi_call").unwrap().availability,
        Availability::Unavailable
    );
}

#[test]
fn capability_probe_marks_missing_channels_unavailable() {
    let tree = TempTree::new();
    let hal = tree.hal();

    for feature in [
        "channel.sysfs",
        "battery.info",
        "battery.status",
        "battery.adapter",
        "battery.conservation",
        "battery.fast_charge",
        "kbd.backlight",
        "touchpad",
        "privacy.camera",
        "tune.rapl",
        "tune.pl1",
        "tune.pl2",
        "panel.refresh",
    ] {
        assert_capability(&hal, feature, Availability::Unavailable);
    }
}

#[test]
fn readable_rapl_energy_without_constraints_is_limited_read_only() {
    let tree = TempTree::new();
    tree.write("sys/class/powercap/intel-rapl:0/energy_uj", "123456\n");

    let capabilities = tree.hal().capabilities().expect("capability probe");
    let rapl = capabilities.get("tune.rapl").unwrap();
    assert_eq!(rapl.availability, Availability::Limited);
    assert!(
        rapl.detail
            .as_deref()
            .unwrap()
            .contains("telemetry/read-only surface")
    );
    assert_eq!(
        capabilities.get("tune.pl1").unwrap().availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("tune.pl2").unwrap().availability,
        Availability::Unavailable
    );
}

#[test]
fn linux_epp_power_schemes_apply_with_readback() {
    let tree = TempTree::new();
    for policy in ["policy0", "policy1"] {
        tree.write(
            format!("sys/devices/system/cpu/cpufreq/{policy}/energy_performance_preference"),
            "balance_performance\n",
        );
    }
    let hal = tree.hal();
    let schemes = hal.power_schemes().unwrap();
    assert!(
        schemes
            .iter()
            .any(|scheme| scheme.name == "Balanced" && scheme.active)
    );

    let report = hal
        .apply_power_mutation(
            PowerMutation::Activate(PowerSchemeId::new("power-saver").unwrap()),
            ApplyMode::Commit,
        )
        .unwrap();

    assert!(matches!(
        report.actual(),
        Some(PowerMutation::Activate(id)) if id.as_str() == "power-saver"
    ));
    for policy in ["policy0", "policy1"] {
        assert_eq!(
            fs::read_to_string(tree.path(format!(
                "sys/devices/system/cpu/cpufreq/{policy}/energy_performance_preference"
            )))
            .unwrap(),
            "power"
        );
    }
}

#[test]
fn linux_epp_and_rapl_limits_validate_safety_and_read_back() {
    let tree = TempTree::new();
    for policy in ["policy0", "policy1"] {
        tree.write(
            format!("sys/devices/system/cpu/cpufreq/{policy}/energy_performance_preference"),
            "balance_performance\n",
        );
    }
    tree.write(
        "sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw",
        "12000000\n",
    );
    tree.write(
        "sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw",
        "30000000\n",
    );
    tree.write(
        "sys/class/powercap/intel-rapl:0/constraint_0_time_window_us",
        "28000000\n",
    );
    let hal = tree.hal();

    let epp = hal.set_epp(255, ApplyMode::Commit).unwrap();
    assert_eq!(epp.actual(), Some(&255));
    let pl1 = hal
        .set_power_limit(PowerLimitKind::Pl1, 15_000_000, ApplyMode::Commit)
        .unwrap();
    assert_eq!(pl1.actual(), Some(&15_000_000));
    assert!(
        hal.set_power_limit(PowerLimitKind::Pl1, 5_000_000, ApplyMode::DryRun)
            .is_err()
    );
    assert_eq!(hal.epp().unwrap(), 255);
}

#[test]
fn linux_thermal_mode_maps_to_performance_control_with_readback() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/thermal_mode", "1\n");
    let hal = tree.hal();

    assert_eq!(
        hal.performance_state().unwrap().active,
        Some(PerformanceMode::Balanced)
    );
    let report = hal
        .set_performance_mode(PerformanceMode::Quiet, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&PerformanceMode::Quiet));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/thermal_mode")).unwrap(),
        "0"
    );
}

#[test]
fn linux_fan_mode_maps_verified_sysfs_tokens_with_readback() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/fan_mode", "balanced\n");
    let hal = tree.hal();

    assert_eq!(hal.fan_mode().unwrap(), FanMode::Standard);
    let report = hal
        .set_fan_mode(FanMode::Silent, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&FanMode::Silent));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/fan_mode")).unwrap(),
        "quiet"
    );
}

#[test]
fn linux_temperature_control_reads_hwmon_and_thermal_zone_values() {
    let tree = TempTree::new();
    tree.write("sys/class/hwmon/hwmon0/name", "coretemp\n");
    tree.write("sys/class/hwmon/hwmon0/temp1_label", "Package id 0\n");
    tree.write("sys/class/hwmon/hwmon0/temp1_input", "47500\n");
    tree.write("sys/class/thermal/thermal_zone0/type", "acpitz\n");
    tree.write("sys/class/thermal/thermal_zone0/temp", "42000\n");

    let sensors = tree.hal().temperature_sensors().unwrap();
    assert_eq!(sensors.len(), 2);
    assert!(sensors.iter().any(|sensor| {
        sensor.metadata.location == lctrl_core::TemperatureLocation::Cpu
            && sensor.value_c == Some(47.5)
    }));
    let sensor = tree.hal().temperature("thermal/thermal_zone0").unwrap();
    assert_eq!(sensor.value_c, Some(42.0));
}

#[test]
fn battery_telemetry_converts_sysfs_units_and_status() {
    let tree = TempTree::new();
    battery_fixture(&tree);

    let telemetry = tree.hal().battery_telemetry(0).expect("battery telemetry");

    assert_eq!(telemetry.design_capacity_mwh, Some(51234));
    assert_eq!(telemetry.full_charge_capacity_mwh, Some(48001));
    assert_eq!(telemetry.remaining_capacity_mwh, Some(12345));
    assert_eq!(telemetry.voltage_mv, Some(11996));
    assert_eq!(telemetry.design_voltage_mv, Some(11400));
    assert_eq!(telemetry.current_ma, Some(-2345));
    // power_supply `temp` is deci-C; BatteryTelemetry stores deci-K.
    assert_eq!(telemetry.temperature_deci_kelvin, Some(2982));
    assert_eq!(telemetry.remaining_percent, Some(77));
    assert_eq!(telemetry.cycle_count, Some(123));
    assert_eq!(telemetry.charge_status, Some(ChargeStatus::Charging));
}

#[test]
fn battery_telemetry_supports_charge_based_supplies_and_identity_fields() {
    let tree = TempTree::new();
    tree.write(
        "sys/class/power_supply/BAT0/voltage_min_design",
        "10000000\n",
    );
    tree.write("sys/class/power_supply/BAT0/voltage_now", "10000000\n");
    tree.write(
        "sys/class/power_supply/BAT0/charge_full_design",
        "5000000\n",
    );
    tree.write("sys/class/power_supply/BAT0/charge_full", "4500000\n");
    tree.write("sys/class/power_supply/BAT0/charge_now", "2250000\n");
    tree.write("sys/class/power_supply/BAT0/health", "Good\n");
    tree.write("sys/class/power_supply/BAT0/manufacturer", "SMP\n");
    tree.write("sys/class/power_supply/BAT0/model_name", "L24M4PF2\n");
    tree.write("sys/class/power_supply/BAT0/serial_number", "ABC123\n");
    tree.write("sys/class/power_supply/BAT0/technology", "Li-poly\n");

    let telemetry = tree.hal().battery_telemetry(0).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, Some(50_000));
    assert_eq!(telemetry.full_charge_capacity_mwh, Some(45_000));
    assert_eq!(telemetry.remaining_capacity_mwh, Some(22_500));
    assert_eq!(telemetry.life_percent, Some(90));
    assert_eq!(telemetry.health, Some(BatteryHealth::Green));
    assert_eq!(telemetry.manufacturer.as_deref(), Some("SMP"));
    assert_eq!(telemetry.model_name.as_deref(), Some("L24M4PF2"));
    assert_eq!(telemetry.serial_number.as_deref(), Some("ABC123"));
    assert_eq!(telemetry.chemistry.as_deref(), Some("Li-poly"));
}

#[test]
fn battery_status_parser_maps_standard_power_supply_states() {
    for (status, expected) in [
        ("Charging", Some(ChargeStatus::Charging)),
        ("Discharging", Some(ChargeStatus::Discharging)),
        ("Not charging", Some(ChargeStatus::NoActivity)),
        ("Full", Some(ChargeStatus::NoActivity)),
        ("Unknown", None),
    ] {
        let tree = TempTree::new();
        tree.write("sys/class/power_supply/BAT0/status", status);
        let telemetry = tree.hal().battery_telemetry(0).expect("battery telemetry");
        assert_eq!(telemetry.charge_status, expected, "status {status}");
    }
}

#[test]
fn battery_numeric_overflow_is_rejected_instead_of_wrapping() {
    let tree = TempTree::new();
    tree.write("sys/class/power_supply/BAT0/energy_now", "4294967296000\n");

    let error = tree.hal().battery_telemetry(0).unwrap_err();
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
}

#[test]
fn adapter_info_requires_a_reliable_ac_online_node() {
    let tree = TempTree::new();
    let error = tree.hal().adapter_info().unwrap_err();
    assert!(matches!(
        error,
        LctrlError::Unsupported { feature } if feature == "battery.adapter"
    ));

    tree.write("sys/class/power_supply/AC/online", "0\n");
    let adapter = tree.hal().adapter_info().expect("AC adapter channel");
    assert_eq!(adapter.authentication, AdapterAuthentication::Unknown);
    assert!(!adapter.has_detail);
    assert_eq!(adapter.detail, None);
}

#[test]
fn touchpad_and_runtime_camera_mutations_read_back() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/touchpad", "1\n");
    tree.write("sys/devices/platform/ideapad/camera_power", "1\n");

    let touchpad = tree
        .hal()
        .set_touchpad(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();
    let camera = tree
        .hal()
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();

    assert_eq!(touchpad.actual(), Some(&DeviceState::Disabled));
    assert_eq!(camera.actual(), Some(&DeviceState::Disabled));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/touchpad")).unwrap(),
        "0"
    );
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/camera_power")).unwrap(),
        "0"
    );
}

#[test]
fn charge_mode_reads_normal_conservation_rapid_and_conflict() {
    for (conservation, rapid, expected) in [
        ("0", "0", ChargeModeActual::Normal),
        ("1", "0", ChargeModeActual::Conservation),
        ("0", "1", ChargeModeActual::Rapid),
        ("1", "1", ChargeModeActual::Conflict),
    ] {
        let tree = TempTree::new();
        ideapad_mode_fixture(&tree, conservation, rapid);
        assert_eq!(
            tree.hal().charge_mode().expect("charge mode"),
            expected,
            "conservation={conservation}, fast_charge={rapid}"
        );
    }
}

#[test]
fn dry_run_reads_charge_mode_but_never_writes() {
    let tree = TempTree::new();
    ideapad_mode_fixture(&tree, "0\n", "0\n");

    let report = tree
        .hal()
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::DryRun)
        .expect("dry-run charge mode");

    assert_eq!(report.mode(), ApplyMode::DryRun);
    assert_eq!(report.previous(), &ChargeMode::Normal);
    assert_eq!(report.requested(), &ChargeMode::Conservation);
    assert_eq!(report.actual(), None);
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode")).unwrap(),
        "0\n"
    );
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/fast_charge")).unwrap(),
        "0\n"
    );
}

#[test]
fn conservation_disables_rapid_before_enabling_conservation() {
    let tree = TempTree::new();
    ideapad_mode_fixture(&tree, "0\n", "1\n");

    let report = tree
        .hal()
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit)
        .expect("conservation mode");

    assert_eq!(report.mode(), ApplyMode::Commit);
    assert_eq!(report.previous(), &ChargeMode::Rapid);
    assert_eq!(report.actual(), Some(&ChargeMode::Conservation));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/fast_charge")).unwrap(),
        "0"
    );
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode")).unwrap(),
        "1"
    );
}

#[test]
fn rapid_disables_conservation_before_enabling_rapid() {
    let tree = TempTree::new();
    ideapad_mode_fixture(&tree, "1\n", "0\n");
    tree.write(
        "sys/class/power_supply/BAT0/energy_full_design",
        "50000000\n",
    );

    let report = tree
        .hal()
        .set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit)
        .expect("rapid mode");

    assert_eq!(report.previous(), &ChargeMode::Conservation);
    assert_eq!(report.actual(), Some(&ChargeMode::Rapid));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode")).unwrap(),
        "0"
    );
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/fast_charge")).unwrap(),
        "1"
    );
}

#[test]
fn rapid_charge_is_blocked_for_39wh_or_missing_design_capacity() {
    for design_capacity in [Some("39000000\n"), None] {
        let tree = TempTree::new();
        ideapad_mode_fixture(&tree, "1\n", "0\n");
        if let Some(capacity) = design_capacity {
            tree.write("sys/class/power_supply/BAT0/energy_full_design", capacity);
        } else {
            fs::create_dir_all(tree.path("sys/class/power_supply/BAT0"))
                .expect("create battery fixture");
        }

        assert!(
            tree.hal()
                .set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit)
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode"))
                .unwrap(),
            "1\n"
        );
        assert_eq!(
            fs::read_to_string(tree.path("sys/devices/platform/ideapad/fast_charge")).unwrap(),
            "0\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn unreadable_rapl_telemetry_marks_capability_unavailable_without_aborting_info() {
    let tree = TempTree::new();
    let path = tree.path("sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj");
    tree.write(
        "sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj",
        "123\n",
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("make unreadable");

    let capabilities = tree.hal().capabilities().expect("probe must not abort");
    assert_eq!(
        capabilities.get("tune.rapl").unwrap().availability,
        Availability::Unavailable
    );

    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restore cleanup mode");
}

#[test]
fn missing_fast_charge_is_unsupported_before_any_write() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/conservation_mode", "0\n");

    let error = tree
        .hal()
        .set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(
        error,
        LctrlError::Unsupported { feature } if feature == "battery.fast_charge"
    ));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode")).unwrap(),
        "0\n"
    );
}

#[test]
fn no_arbitrary_threshold_mutator_is_advertised() {
    let tree = TempTree::new();
    let capabilities = tree.hal().capabilities().expect("capability probe");
    let threshold = capabilities.get("battery.thresholds").unwrap();
    assert_eq!(threshold.availability, Availability::Unavailable);
    assert!(threshold.detail.as_deref().unwrap().contains("arbitrary"));
}

#[cfg(unix)]
#[test]
fn permission_errors_map_to_the_documented_privilege_need() {
    // The repository test runner may itself be root (as in a container); in
    // that environment mode 000 is intentionally still readable by root.
    let effective_uid = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("Uid:"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|uid| uid.parse::<u32>().ok())
        });
    if effective_uid == Some(0) {
        return;
    }

    use std::os::unix::fs::PermissionsExt;

    let tree = TempTree::new();
    tree.write("sys/class/power_supply/BAT0/energy_now", "12000000\n");
    let energy_now = tree.path("sys/class/power_supply/BAT0/energy_now");
    fs::set_permissions(&energy_now, fs::Permissions::from_mode(0o000))
        .expect("remove read permission");

    let error = tree.hal().battery_telemetry(0).unwrap_err();
    assert!(matches!(
        error,
        LctrlError::PermissionDenied { need } if need == "root or configured udev rule"
    ));
}

#[test]
fn charge_mode_preserves_unknown_sysfs_combinations_without_writing() {
    let tree = TempTree::new();
    ideapad_mode_fixture(&tree, "2\n", "0\n");

    assert_eq!(
        tree.hal().charge_mode().expect("charge mode"),
        ChargeModeActual::Unknown(2)
    );
    let error = tree
        .hal()
        .set_charge_mode(ChargeMode::Normal, ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/conservation_mode")).unwrap(),
        "2\n"
    );
}

#[test]
fn keyboard_backlight_dry_run_and_commit_use_discovered_max() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/kbd_backlight", "1\n");
    tree.write("sys/devices/platform/ideapad/kbd_backlight_max", "3\n");
    let hal = tree.hal();

    let dry_run = hal
        .set_backlight(2, LightingEffect::Static, ApplyMode::DryRun)
        .unwrap();
    assert_eq!(dry_run.previous().level, 1);
    assert_eq!(dry_run.requested().max_level, 3);
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/kbd_backlight")).unwrap(),
        "1\n"
    );

    let committed = hal
        .set_backlight(2, LightingEffect::Static, ApplyMode::Commit)
        .unwrap();
    assert_eq!(committed.actual().unwrap().level, 2);
}

#[test]
fn keyboard_backlight_rejects_unsupported_effect_before_write() {
    let tree = TempTree::new();
    tree.write("sys/devices/platform/ideapad/kbd_backlight", "1\n");
    tree.write("sys/devices/platform/ideapad/kbd_backlight_max", "3\n");

    assert!(matches!(
        tree.hal()
            .set_backlight(2, LightingEffect::Breathing, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
    assert_eq!(
        fs::read_to_string(tree.path("sys/devices/platform/ideapad/kbd_backlight")).unwrap(),
        "1\n"
    );
}

#[test]
fn magicbay_detection_uses_verified_usb_and_acpi_ids() {
    let tree = TempTree::new();
    tree.write("sys/bus/usb/devices/1-2/idVendor", "17ef\n");
    tree.write("sys/bus/usb/devices/1-2/idProduct", "7005\n");
    fs::create_dir_all(tree.path("sys/bus/usb/devices/1-2:1.0"))
        .expect("create MBIM interface fixture");
    tree.write("sys/bus/usb/devices/2-1/idVendor", "1234\n");
    tree.write("sys/bus/usb/devices/2-1/idProduct", "7005\n");
    fs::create_dir_all(tree.path("sys/bus/acpi/devices/QCOM2488:00"))
        .expect("create ACPI display fixture");
    fs::create_dir_all(tree.path("sys/bus/acpi/devices/QCOM24B7:00"))
        .expect("create role-switch fixture");

    let inventory = tree.hal().detect_magicbay().unwrap();

    assert_eq!(inventory.devices.len(), 1);
    let lte = &inventory.devices[0];
    assert_eq!(lte.vid, Some(0x17ef));
    assert_eq!(lte.pid, Some(0x7005));
    assert_eq!(lte.kind, MagicBayKind::Lte2);
    assert_eq!(lte.interfaces, vec!["mbim"]);
    assert_eq!(inventory.acpi_devices.len(), 2);
    assert!(inventory.acpi_devices.iter().any(|device| {
        device.kind == MagicBayKind::DisplayBridge && device.interfaces == vec!["display"]
    }));
    assert!(inventory.acpi_devices.iter().any(|device| {
        device.kind == MagicBayKind::UsbRoleSwitch && device.interfaces == vec!["usb_role_switch"]
    }));
}

#[test]
fn standard_diagnostics_report_inventory_only_without_fake_deep_pass() {
    let tree = TempTree::new();
    tree.write("proc/meminfo", "MemTotal: 1024 kB\n");

    let results = tree
        .hal()
        .run_diagnostics(&[DiagnosticKind::Memory, DiagnosticKind::Battery])
        .unwrap();

    assert_eq!(results[0].outcome, DiagnosticOutcome::Warning);
    assert!(
        results[0]
            .detail
            .contains("deep vendor-driver diagnostics are excluded")
    );
    assert_eq!(results[1].outcome, DiagnosticOutcome::Unavailable);
}

#[test]
fn update_capability_fails_closed_without_catalog_trust_contract() {
    let capability = tree_update_capability();
    assert!(matches!(
        capability,
        UpdateCapability::Unavailable { reason } if reason.contains("authenticated public update catalog")
    ));
}

#[test]
fn conflict_detection_reports_vendor_controller_processes() {
    let tree = TempTree::new();
    tree.write("proc/123/comm", "LenovoVantageService\n");
    tree.write("proc/123/cmdline", "LenovoVantageService\0--service\0");
    tree.write("proc/456/comm", "unrelated\n");

    assert_eq!(
        tree.hal().active_vendor_controllers().unwrap(),
        vec!["LenovoVantageService"]
    );
}

fn tree_update_capability() -> UpdateCapability {
    TempTree::new().hal().update_capability().unwrap()
}
