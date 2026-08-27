use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lctrl_core::{
    AdapterAuthentication, ApplyMode, Availability, ChargeMode, ChargeModeActual, ChargeStatus,
    LctrlError, Platform,
};
use lctrl_hal::{BatteryControl, Hal};
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
    for feature in ["tune.rapl", "tune.pl1", "tune.pl2", "panel.refresh"] {
        assert_eq!(
            capabilities.get(feature).unwrap().availability,
            Availability::Limited,
            "{feature} should be limited"
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
