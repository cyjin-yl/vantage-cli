use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, CapabilitySet, ChangeReport, ChargeMode,
    ChargeModeActual, DeviceState, HardwareInfo, LctrlError, Platform,
};
use lctrl_hal::{
    BatteryControl, BiosControl, ControlConflictDetection, Hal, PerformanceControl, PowerControl,
    PrivacyControl,
};
use vantage_cli::{
    Cli, CommandResult, CommandServices, execute, execute_with_services, render_error,
};

struct FakeHal {
    info_calls: AtomicUsize,
    capability_calls: AtomicUsize,
    product_name: &'static str,
}

impl FakeHal {
    fn new(product_name: &'static str) -> Self {
        Self {
            info_calls: AtomicUsize::new(0),
            capability_calls: AtomicUsize::new(0),
            product_name,
        }
    }
}

impl Hal for FakeHal {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn hardware_info(&self) -> lctrl_core::Result<HardwareInfo> {
        self.info_calls.fetch_add(1, Ordering::SeqCst);
        Ok(HardwareInfo {
            product_name: Some(self.product_name.to_owned()),
            family: Some("21VG".into()),
            bios_version: Some("1.07".into()),
        })
    }

    fn capabilities(&self) -> lctrl_core::Result<CapabilitySet> {
        self.capability_calls.fetch_add(1, Ordering::SeqCst);
        let mut capabilities = CapabilitySet::new(Platform::Linux);
        capabilities
            .record("battery.status", Availability::Available, None)
            .expect("valid capability");
        capabilities
            .record(
                "tune.pl1",
                Availability::Limited,
                Some("Linux powercap only".into()),
            )
            .expect("valid capability");
        Ok(capabilities)
    }
}

#[test]
fn info_queries_hal_and_returns_machine_readable_payload() {
    let cli = Cli::try_parse_from(["vantage", "--json", "info"]).expect("parse info");
    let hal = FakeHal::new("ThinkBook 14+ 2026");

    let output = execute(cli, &hal).expect("info succeeds");

    assert_eq!(hal.info_calls.load(Ordering::SeqCst), 1);
    assert_eq!(hal.capability_calls.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["platform"], "linux");
    assert_eq!(
        output.json["hardware"]["product_name"],
        "ThinkBook 14+ 2026"
    );
    assert_eq!(output.json["hardware"]["family"], "21VG");
    assert_eq!(output.json["hardware"]["bios_version"], "1.07");
    assert_eq!(
        output.json["features"]["battery.status"]["availability"],
        "available"
    );
    assert_eq!(
        output.json["features"]["tune.pl1"]["detail"],
        "Linux powercap only"
    );
}

#[test]
fn info_human_output_contains_platform_and_hardware_values() {
    let cli = Cli::try_parse_from(["vantage", "info"]).expect("parse info");
    let output = execute(cli, &FakeHal::new("ThinkBook 14+ 2026")).expect("info succeeds");

    assert!(output.human.contains("platform: linux"));
    assert!(output.human.contains("ThinkBook 14+ 2026"));
    assert!(output.human.contains("21VG"));
    assert!(output.human.contains("1.07"));
    assert!(output.human.contains("battery.status"));
}

#[test]
fn json_error_is_exactly_the_core_error_report() {
    let error = LctrlError::Unsupported {
        feature: "battery.thresholds".into(),
    };

    let rendered = render_error(&error, true);
    let expected = serde_json::to_string(&error.report()).expect("serialize report");

    assert_eq!(rendered, expected);
}

#[test]
fn representative_command_paths_parse() {
    let cases: &[&[&str]] = &[
        &["vantage", "info"],
        &["vantage", "battery", "status"],
        &["vantage", "battery", "adapter"],
        &["vantage", "battery", "charge-mode", "rapid"],
        &["vantage", "battery", "thresholds", "40", "80"],
        &["vantage", "battery", "extreme-life", "on"],
        &["vantage", "battery", "night-charge", "off"],
        &["vantage", "battery", "temporary-mode"],
        &["vantage", "battery", "watch"],
        &["vantage", "usb", "always-on", "on", "--persistent"],
        &["vantage", "usb", "charge-on-battery", "off"],
        &["vantage", "power", "scheme", "list"],
        &["vantage", "power", "scheme", "get", "balanced"],
        &["vantage", "power", "scheme", "apply", "balanced"],
        &[
            "vantage", "power", "scheme", "set", "subgroup", "setting", "ac", "42",
        ],
        &["vantage", "power", "saver-once"],
        &["vantage", "perf", "mode", "performance"],
        &["vantage", "perf", "fan", "status"],
        &["vantage", "perf", "temp"],
        &["vantage", "perf", "pl1", "15"],
        &["vantage", "perf", "pl2", "25"],
        &["vantage", "perf", "top"],
        &["vantage", "tune", "profile", "list"],
        &["vantage", "kbd", "backlight", "2", "--effect", "breath"],
        &["vantage", "kbd", "fnlock", "on"],
        &["vantage", "touchpad", "off"],
        &["vantage", "panel", "rate", "120"],
        &["vantage", "privacy", "cam", "off", "--runtime"],
        &["vantage", "sense", "lock-on-leave", "on", "--distance", "2"],
        &["vantage", "audio", "dolby", "movie"],
        &["vantage", "bios", "get", "SecureBoot"],
        &["vantage", "magicbay", "lte", "status"],
        &["vantage", "osd", "enable"],
        &["vantage", "daemon", "status"],
        &["vantage", "completions", "bash"],
    ];

    for args in cases {
        Cli::try_parse_from(*args).unwrap_or_else(|error| {
            panic!("failed to parse {args:?}: {error}");
        });
    }
}

#[test]
fn unsupported_commands_do_not_call_or_mutate_hal() {
    let hal = FakeHal::new("unused");
    let cli = Cli::try_parse_from(["vantage", "battery", "charge-mode", "rapid"])
        .expect("parse unsupported command");

    let result: CommandResult = execute(cli, &hal);

    assert!(matches!(
        result,
        Err(LctrlError::Unsupported { ref feature }) if feature == "battery.charge-mode"
    ));
    assert_eq!(hal.info_calls.load(Ordering::SeqCst), 0);
    assert_eq!(hal.capability_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn unsupported_error_uses_core_exit_code_mapping() {
    let cli = Cli::try_parse_from(["vantage", "tune"]).expect("parse tune");
    let result = execute(cli, &FakeHal::new("unused"));
    let error = result.expect_err("tune is not implemented");

    assert_eq!(
        error.exit_code(),
        LctrlError::Unsupported {
            feature: "tune".into()
        }
        .exit_code()
    );
}

struct FakeBattery {
    adapter_calls: AtomicUsize,
}

impl FakeBattery {
    const fn new() -> Self {
        Self {
            adapter_calls: AtomicUsize::new(0),
        }
    }
}

impl BatteryControl for FakeBattery {
    fn battery_telemetry(&self, _index: u32) -> lctrl_core::Result<lctrl_core::BatteryTelemetry> {
        unreachable!()
    }

    fn adapter_info(&self) -> lctrl_core::Result<AdapterInfo> {
        self.adapter_calls.fetch_add(1, Ordering::SeqCst);
        Ok(AdapterInfo::from_gbmd(0x0086_0004, None))
    }

    fn charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
        Ok(ChargeModeActual::Normal)
    }

    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
        unreachable!()
    }
}

struct FakeConflicts;

impl ControlConflictDetection for FakeConflicts {
    fn active_vendor_controllers(&self) -> lctrl_core::Result<Vec<String>> {
        Ok(vec!["LenovoVantageService.exe".into()])
    }
}

#[test]
fn conflicting_vendor_controller_blocks_write_without_explicit_override() {
    let cli = Cli::try_parse_from(["vantage", "battery", "charge-mode", "rapid"]).unwrap();
    let hal = FakeHal::new("unused");
    let battery = FakeBattery::new();

    assert!(matches!(
        execute_with_services(
            cli,
            CommandServices::new(&hal)
                .with_battery(&battery)
                .with_conflict_detection(&FakeConflicts),
        ),
        Err(LctrlError::InvalidArgument { detail }) if detail.contains("LenovoVantageService.exe")
    ));
}

#[test]
fn battery_adapter_executes_through_explicit_service_only() {
    let cli = Cli::try_parse_from(["vantage", "--json", "battery", "adapter"]).unwrap();
    let hal = FakeHal::new("unused");
    let battery = FakeBattery::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_battery(&battery))
        .expect("adapter succeeds with service");

    assert_eq!(battery.adapter_calls.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["authentication"], "inbox");
    assert_eq!(output.json["has_detail"], false);
    assert_eq!(hal.info_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn battery_adapter_stays_unsupported_without_service() {
    let cli = Cli::try_parse_from(["vantage", "battery", "adapter"]).unwrap();
    let hal = FakeHal::new("unused");

    assert!(matches!(
        execute(cli, &hal),
        Err(LctrlError::Unsupported { feature }) if feature == "battery.adapter"
    ));
    assert_eq!(hal.info_calls.load(Ordering::SeqCst), 0);
}

struct FakePerformance {
    modes: Mutex<Vec<(lctrl_core::PerformanceMode, ApplyMode)>>,
}

impl FakePerformance {
    const fn new() -> Self {
        Self {
            modes: Mutex::new(Vec::new()),
        }
    }
}

impl PerformanceControl for FakePerformance {
    fn performance_state(&self) -> lctrl_core::Result<lctrl_core::PerformanceState> {
        unreachable!()
    }

    fn set_performance_mode(
        &self,
        mode: lctrl_core::PerformanceMode,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PerformanceMode>> {
        self.modes.lock().push((mode, apply));
        Ok(ChangeReport::dry_run(mode, mode))
    }
}

#[test]
fn global_dry_run_reaches_performance_service_without_commit() {
    let cli = Cli::try_parse_from(["vantage", "--dry-run", "perf", "mode", "performance"]).unwrap();
    let hal = FakeHal::new("unused");
    let performance = FakePerformance::new();

    let result = execute_with_services(
        cli,
        CommandServices::new(&hal).with_performance(&performance),
    );

    assert!(result.is_ok());
    assert_eq!(
        &*performance.modes.lock(),
        &[(lctrl_core::PerformanceMode::Performance, ApplyMode::DryRun)]
    );
}

struct FakePower {
    range_calls: AtomicUsize,
    mutation_calls: AtomicUsize,
}

impl FakePower {
    const fn new() -> Self {
        Self {
            range_calls: AtomicUsize::new(0),
            mutation_calls: AtomicUsize::new(0),
        }
    }
}

impl PowerControl for FakePower {
    fn power_schemes(&self) -> lctrl_core::Result<Vec<lctrl_core::PowerScheme>> {
        Ok(Vec::new())
    }

    fn active_power_scheme(&self) -> lctrl_core::Result<lctrl_core::PowerScheme> {
        unreachable!()
    }

    fn power_value_range(
        &self,
        _key: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerValueRange> {
        self.range_calls.fetch_add(1, Ordering::SeqCst);
        lctrl_core::PowerValueRange::new(0, 100, 10)
    }

    fn apply_power_mutation(
        &self,
        mutation: lctrl_core::PowerMutation,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PowerMutation>> {
        self.mutation_calls.fetch_add(1, Ordering::SeqCst);
        Ok(ChangeReport::dry_run(mutation.clone(), mutation))
    }
}

#[test]
fn power_set_validates_service_range_before_dispatch() {
    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "power",
        "scheme",
        "set",
        "subgroup",
        "setting",
        "dc",
        "50",
    ])
    .unwrap();
    let hal = FakeHal::new("unused");
    let power = FakePower::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_power(&power))
        .expect("validated power dry-run");

    assert_eq!(power.range_calls.load(Ordering::SeqCst), 1);
    assert_eq!(power.mutation_calls.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["mode"], "dry_run");
}

struct FakeBios {
    staged: Mutex<Vec<lctrl_core::BiosChange>>,
    saves: AtomicUsize,
    discards: AtomicUsize,
    fail_save: bool,
}

impl FakeBios {
    const fn new() -> Self {
        Self {
            staged: Mutex::new(Vec::new()),
            saves: AtomicUsize::new(0),
            discards: AtomicUsize::new(0),
            fail_save: false,
        }
    }

    const fn failing_save() -> Self {
        Self {
            staged: Mutex::new(Vec::new()),
            saves: AtomicUsize::new(0),
            discards: AtomicUsize::new(0),
            fail_save: true,
        }
    }
}

impl BiosControl for FakeBios {
    fn list(&self) -> lctrl_core::Result<Vec<lctrl_core::BiosItem>> {
        Ok(vec![lctrl_core::BiosItem {
            name: "Camera".into(),
            value: "Enable".into(),
            selections: vec!["Enable".into(), "Disable".into()],
        }])
    }

    fn get(&self, name: &str) -> lctrl_core::Result<Option<lctrl_core::BiosItem>> {
        let value = self
            .staged
            .lock()
            .last()
            .filter(|change| change.name.as_str().eq_ignore_ascii_case(name))
            .map_or("Enable", |change| change.value.as_str())
            .to_string();
        Ok(Some(lctrl_core::BiosItem {
            name: "Camera".into(),
            value,
            selections: vec!["Enable".into(), "Disable".into()],
        }))
    }

    fn selections(&self, _name: &str) -> lctrl_core::Result<Vec<String>> {
        Ok(vec!["Enable".into(), "Disable".into()])
    }

    fn stage(&self, change: lctrl_core::BiosChange) -> lctrl_core::Result<()> {
        self.staged.lock().push(change);
        Ok(())
    }

    fn save(&self) -> lctrl_core::Result<()> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        if self.fail_save {
            Err(LctrlError::FirmwareRejected {
                detail: "test save failure".into(),
            })
        } else {
            Ok(())
        }
    }

    fn discard(&self) -> lctrl_core::Result<()> {
        self.discards.fetch_add(1, Ordering::SeqCst);
        self.staged.lock().clear();
        Ok(())
    }

    fn password_status(&self) -> lctrl_core::Result<lctrl_core::BiosPasswordStatus> {
        Ok(lctrl_core::BiosPasswordStatus::from_raw(1, 128, 0))
    }
}

#[test]
fn bios_set_requires_confirmation_before_staging() {
    let cli = Cli::try_parse_from(["vantage", "bios", "set", "Camera", "Disable"]).unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    assert!(matches!(
        execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(bios.staged.lock().is_empty());
}

#[test]
fn bios_set_rejects_unrecoverable_staged_only_commit() {
    let cli =
        Cli::try_parse_from(["vantage", "bios", "set", "Camera", "Disable", "--yes"]).unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    assert!(matches!(
        execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::InvalidArgument { detail }) if detail.contains("require --save")
    ));
    assert!(bios.staged.lock().is_empty());
}

#[test]
fn bios_set_save_validates_exact_selection_and_reads_back() {
    let cli = Cli::try_parse_from([
        "vantage", "bios", "set", "Camera", "Disable", "--yes", "--save",
    ])
    .unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios))
        .expect("safe BIOS transaction");

    assert_eq!(bios.staged.lock().len(), 1);
    assert_eq!(bios.saves.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["requested"]["name"], "Camera");
    assert_eq!(output.json["requested"]["value"], "Disable");
    assert_eq!(output.json["actual"]["value"], "Disable");
}

#[test]
fn bios_save_failure_discards_staged_transaction() {
    let cli = Cli::try_parse_from([
        "vantage", "bios", "set", "Camera", "Disable", "--yes", "--save",
    ])
    .unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::failing_save();

    assert!(matches!(
        execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::FirmwareRejected { .. })
    ));
    assert_eq!(bios.discards.load(Ordering::SeqCst), 1);
    assert!(bios.staged.lock().is_empty());
}

#[test]
fn persistent_privacy_requires_confirmation_and_reads_back() {
    let cli =
        Cli::try_parse_from(["vantage", "privacy", "cam", "off", "--persistent", "--yes"]).unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios))
        .expect("persistent camera change");

    assert_eq!(bios.staged.lock()[0].name.as_str(), "IntegratedCamera");
    assert_eq!(bios.staged.lock()[0].value.as_str(), "Disable");
    assert_eq!(bios.saves.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["mode"], "commit");
}

#[test]
fn privacy_runtime_is_unavailable_without_verified_feature_id() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "off", "--runtime"]).unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    assert!(matches!(
        execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::Unsupported { feature }) if feature == "privacy.cam.runtime"
    ));
    assert!(bios.staged.lock().is_empty());
}

#[test]
fn persistent_privacy_dry_run_never_stages_or_saves() {
    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "privacy",
        "mic",
        "off",
        "--persistent",
    ])
    .unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_bios(&bios))
        .expect("privacy dry run");

    assert!(bios.staged.lock().is_empty());
    assert_eq!(bios.saves.load(Ordering::SeqCst), 0);
    assert_eq!(output.json["mode"], "dry_run");
}

struct FakePrivacy;

impl PrivacyControl for FakePrivacy {
    fn camera_state(&self) -> lctrl_core::Result<DeviceState> {
        Ok(DeviceState::Enabled)
    }

    fn set_camera(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<DeviceState>> {
        Ok(match apply {
            ApplyMode::DryRun => ChangeReport::dry_run(DeviceState::Enabled, state),
            ApplyMode::Commit => ChangeReport::committed(DeviceState::Enabled, state, state),
        })
    }
}

#[test]
fn privacy_runtime_dispatches_to_runtime_service() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "off", "--runtime"]).unwrap();
    let hal = FakeHal::new("unused");

    let output =
        execute_with_services(cli, CommandServices::new(&hal).with_privacy(&FakePrivacy)).unwrap();

    assert_eq!(output.json["actual"], "disabled");
}

#[test]
fn tuning_profiles_are_listed_and_planned_without_writes() {
    let hal = FakeHal::new("unused");
    let list = Cli::try_parse_from(["vantage", "tune", "profile", "list"]).unwrap();
    let output = execute(list, &hal).expect("built-in profiles list");
    assert!(
        output
            .json
            .as_array()
            .is_some_and(|profiles| !profiles.is_empty())
    );

    let apply = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "tune",
        "profile",
        "apply",
        "balanced",
    ])
    .unwrap();
    let plan = execute(apply, &hal).expect("balanced dry-run plan");
    assert_eq!(plan.json["profile"], "balanced");
    assert_eq!(plan.json["mode"], "dry_run");
}

static SNAPSHOT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SnapshotPathGuard;

impl Drop for SnapshotPathGuard {
    fn drop(&mut self) {
        // This integration test serializes every access to this process-local test variable.
        unsafe { std::env::remove_var("VANTAGE_SNAPSHOT_PATH") };
    }
}

#[test]
fn managed_snapshot_capture_diff_and_restore_use_persistent_baseline() {
    let _lock = SNAPSHOT_ENV_LOCK.lock().unwrap();
    let path: PathBuf =
        std::env::temp_dir().join(format!("vantage-snapshot-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    // This test holds SNAPSHOT_ENV_LOCK until the guard removes the variable.
    unsafe { std::env::set_var("VANTAGE_SNAPSHOT_PATH", &path) };
    let _path_guard = SnapshotPathGuard;

    let baseline_hal = FakeHal::new("baseline");
    let capture = Cli::try_parse_from(["vantage", "snapshot", "capture"]).unwrap();
    let captured = execute(capture, &baseline_hal).unwrap();
    assert_eq!(captured.json["version"], 1);
    assert!(path.is_file());

    let diff = Cli::try_parse_from(["vantage", "snapshot", "diff"]).unwrap();
    let unchanged = execute(diff, &baseline_hal).unwrap();
    assert_eq!(unchanged.json["equal"], true);

    let changed_hal = FakeHal::new("changed");
    let diff = Cli::try_parse_from(["vantage", "snapshot", "diff"]).unwrap();
    let changed = execute(diff, &changed_hal).unwrap();
    assert_eq!(changed.json["equal"], false);
    assert!(
        changed.json["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "hardware")
    );

    let restore = Cli::try_parse_from(["vantage", "snapshot", "restore", "--dry-run"]).unwrap();
    let restored = execute(restore, &baseline_hal).unwrap();
    assert_eq!(restored.json, serde_json::json!([]));

    std::fs::remove_file(path).unwrap();
}
