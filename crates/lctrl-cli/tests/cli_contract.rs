use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use lctrl_cli::{
    Cli, CommandResult, CommandServices, execute, execute_with_services, render_error,
};
use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, CapabilitySet, ChangeReport, ChargeMode,
    ChargeModeActual, HardwareInfo, LctrlError, Platform,
};
use lctrl_hal::{BatteryControl, Hal, PerformanceControl, PowerControl};

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
    let cli = Cli::try_parse_from(["lctrl", "--json", "info"]).expect("parse info");
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
    let cli = Cli::try_parse_from(["lctrl", "info"]).expect("parse info");
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
        &["lctrl", "info"],
        &["lctrl", "battery", "status"],
        &["lctrl", "battery", "adapter"],
        &["lctrl", "battery", "charge-mode", "rapid"],
        &["lctrl", "battery", "thresholds", "40", "80"],
        &["lctrl", "battery", "extreme-life", "on"],
        &["lctrl", "battery", "night-charge", "off"],
        &["lctrl", "battery", "temporary-mode"],
        &["lctrl", "battery", "watch"],
        &["lctrl", "usb", "always-on", "on", "--persistent"],
        &["lctrl", "usb", "charge-on-battery", "off"],
        &["lctrl", "power", "scheme", "list"],
        &["lctrl", "power", "scheme", "get", "balanced"],
        &["lctrl", "power", "scheme", "apply", "balanced"],
        &[
            "lctrl", "power", "scheme", "set", "subgroup", "setting", "ac", "42",
        ],
        &["lctrl", "power", "saver-once"],
        &["lctrl", "perf", "mode", "performance"],
        &["lctrl", "perf", "fan", "status"],
        &["lctrl", "perf", "temp"],
        &["lctrl", "perf", "pl1", "15"],
        &["lctrl", "perf", "pl2", "25"],
        &["lctrl", "perf", "top"],
        &["lctrl", "tune", "profile", "list"],
        &["lctrl", "kbd", "backlight", "2", "--effect", "breath"],
        &["lctrl", "kbd", "fnlock", "on"],
        &["lctrl", "touchpad", "off"],
        &["lctrl", "panel", "rate", "120"],
        &["lctrl", "privacy", "cam", "off", "--runtime"],
        &["lctrl", "sense", "lock-on-leave", "on", "--distance", "2"],
        &["lctrl", "audio", "dolby", "movie"],
        &["lctrl", "bios", "get", "SecureBoot"],
        &["lctrl", "magicbay", "lte", "status"],
        &["lctrl", "osd", "enable"],
        &["lctrl", "daemon", "status"],
        &["lctrl", "completions", "bash"],
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
    let cli = Cli::try_parse_from(["lctrl", "battery", "charge-mode", "rapid"])
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
    let cli = Cli::try_parse_from(["lctrl", "tune"]).expect("parse tune");
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

#[test]
fn battery_adapter_executes_through_explicit_service_only() {
    let cli = Cli::try_parse_from(["lctrl", "--json", "battery", "adapter"]).unwrap();
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
    let cli = Cli::try_parse_from(["lctrl", "battery", "adapter"]).unwrap();
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
    let cli = Cli::try_parse_from(["lctrl", "--dry-run", "perf", "mode", "performance"]).unwrap();
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
        "lctrl",
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
