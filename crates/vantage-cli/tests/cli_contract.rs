use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, BiosInventory, CapabilitySet, ChangeReport, ChargeMode,
    ChargeModeActual, DeviceState, DolbyProfile, DolbyState, HardwareInfo, LctrlError,
    MagicBayDevice, MagicBayKind, MagicBayLteInterface, MagicBayLteMediaState,
    MagicBayLteOperational, MagicBayLteStatus, MotherboardInventory, Platform, ProductInventory,
    SenseDistance, SenseState, SystemIdentity,
};
use lctrl_hal::{
    AudioControl, BatteryControl, BiosControl, ControlConflictDetection, DbdcControl,
    GpuOcControl, Hal, MobileBroadbandControl, PanelBrightnessControl, PerformanceControl,
    PowerControl, PrivacyControl, SenseControl, SenseFeature, TemperatureControl, UsbControl,
};
use serde_json::json;
use vantage_cli::{
    Cli, Command, CommandResult, CommandServices, KbdCommand, PerfCommand, PrivacyCamAction,
    PrivacyCommand, StreamKind, StreamSpec, TempCommand, TuneCommand, execute,
    execute_with_services, render_error, sample_stream, stream_spec,
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

struct IdentityHal {
    identity: SystemIdentity,
}

impl Hal for IdentityHal {
    fn platform(&self) -> Platform {
        Platform::Windows
    }

    fn hardware_info(&self) -> lctrl_core::Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: Some("21VG".into()),
            family: Some("ThinkBook 14 G8+ IPH".into()),
            bios_version: Some("SWCN22WW".into()),
        })
    }

    fn capabilities(&self) -> lctrl_core::Result<CapabilitySet> {
        Ok(CapabilitySet::new(Platform::Windows))
    }

    fn system_identity(&self) -> lctrl_core::Result<SystemIdentity> {
        Ok(self.identity.clone())
    }
}

impl IdentityHal {
    fn target() -> Self {
        Self {
            identity: SystemIdentity {
                product: ProductInventory {
                    name: Some("21VG".into()),
                    version: Some("ThinkBook 14 G8+ IPH".into()),
                    identifying_number: Some("PRODUCT-SERIAL".into()),
                    model: Some("21VG".into()),
                    family: Some("ThinkBook 14 G8+ IPH".into()),
                    ..ProductInventory::default()
                },
                motherboard: MotherboardInventory {
                    product: Some("LNVNB161216".into()),
                    version: Some("SDK0T76577 WIN".into()),
                    ..MotherboardInventory::default()
                },
                bios: BiosInventory {
                    smbios_bios_version: Some("SWCN22WW".into()),
                    release_date: Some("20260715000000.000000+000".into()),
                    embedded_controller_major_version: Some(1),
                    embedded_controller_minor_version: Some(22),
                    ..BiosInventory::default()
                },
            },
        }
    }
}

#[test]
fn info_reports_identity_modules_next_to_unchanged_hardware_keys() {
    let cli = Cli::try_parse_from(["vantage", "--json", "info"]).expect("parse info");
    let output = execute(cli, &IdentityHal::target()).expect("info succeeds");

    // The pre-existing keys keep their values and shape.
    assert_eq!(output.json["platform"], "windows");
    assert_eq!(output.json["hardware"]["product_name"], "21VG");
    assert_eq!(output.json["hardware"]["family"], "ThinkBook 14 G8+ IPH");
    assert_eq!(output.json["hardware"]["bios_version"], "SWCN22WW");

    assert_eq!(output.json["identity"]["product"]["name"], "21VG");
    assert_eq!(output.json["identity"]["product"]["model"], "21VG");
    assert_eq!(
        output.json["identity"]["product"]["version"],
        "ThinkBook 14 G8+ IPH"
    );
    assert_eq!(
        output.json["identity"]["product"]["identifying_number"],
        "PRODUCT-SERIAL"
    );
    assert_eq!(output.json["identity"]["motherboard"]["product"], "LNVNB161216");
    assert_eq!(
        output.json["identity"]["bios"]["smbios_bios_version"],
        "SWCN22WW"
    );
    assert_eq!(
        output.json["identity"]["bios"]["embedded_controller_major_version"],
        1
    );
    assert_eq!(
        output.json["identity"]["bios"]["embedded_controller_minor_version"],
        22
    );
}

#[test]
fn info_human_output_lists_identity_values_and_omits_an_absent_identity() {
    let cli = Cli::try_parse_from(["vantage", "info"]).expect("parse info");
    let output = execute(cli, &IdentityHal::target()).expect("info succeeds");

    assert!(output.human.contains("  model (mtm): 21VG"));
    assert!(output.human.contains("  product version: ThinkBook 14 G8+ IPH"));
    assert!(output.human.contains("  serial: PRODUCT-SERIAL"));
    assert!(output.human.contains("  baseboard: LNVNB161216"));
    assert!(output.human.contains("  baseboard version: SDK0T76577 WIN"));
    assert!(
        output
            .human
            .contains("  bios release date: 20260715000000.000000+000")
    );
    assert!(output.human.contains("  ec firmware: 1.22"));
    assert!(output.human.contains("features:"));

    // A HAL without an identity source reports no identity block and no JSON key.
    let cli = Cli::try_parse_from(["vantage", "--json", "info"]).expect("parse info");
    let output = execute(cli, &FakeHal::new("ThinkBook 14+ 2026")).expect("info succeeds");
    assert!(!output.human.contains("identity:"));
    assert!(output.json.get("identity").is_none());
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
        &["vantage", "battery", "health"],
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
        &["vantage", "perf", "dbdc", "get"],
        &["vantage", "perf", "pl2", "25"],
        &["vantage", "perf", "top"],
        &["vantage", "tune", "profile", "list"],
        &["vantage", "kbd", "backlight", "2", "--effect", "breath"],
        &["vantage", "kbd", "backlight", "off"],
        &["vantage", "kbd", "backlight", "low"],
        &["vantage", "kbd", "backlight", "high"],
        &["vantage", "kbd", "backlight", "auto"],
        &["vantage", "kbd", "fnlock", "on"],
        &["vantage", "touchpad", "off"],
        &["vantage", "touchpad", "disable"],
        &["vantage", "touchpad", "enable"],
        &["vantage", "touchpad", "recover"],
        &["vantage", "panel", "rate", "120"],
        &["vantage", "privacy", "cam", "off", "--runtime"],
        &["vantage", "sense", "lock-on-leave", "on", "--distance", "2"],
        &["vantage", "audio", "dolby", "movie"],
        &["vantage", "bios", "get", "SecureBoot"],
        &["vantage", "magicbay", "lte", "status"],
        &["vantage", "osd", "enable"],
        &["vantage", "security", "status"],
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
fn backlight_levels_accept_state_names_and_raw_levels() {
    for (argument, level) in [("off", 0), ("low", 1), ("high", 2), ("auto", 3), ("2", 2)] {
        let cli = Cli::try_parse_from(["vantage", "kbd", "backlight", argument]).expect("parse");
        match cli.command {
            Command::Kbd {
                command: Some(KbdCommand::Backlight { level: parsed, .. }),
            } => assert_eq!(parsed, level, "argument {argument:?}"),
            other => panic!("unexpected command for {argument:?}: {other:?}"),
        }
    }
    for argument in ["bright", "300", "-1", ""] {
        assert!(
            Cli::try_parse_from(["vantage", "kbd", "backlight", argument]).is_err(),
            "argument {argument:?} must be rejected by the parser"
        );
    }
}

struct ChangingTemperatures {
    reads: AtomicUsize,
    fail: bool,
}

impl ChangingTemperatures {
    fn new(fail: bool) -> Self {
        Self {
            reads: AtomicUsize::new(0),
            fail,
        }
    }
}

impl TemperatureControl for ChangingTemperatures {
    fn temperature_sensors(&self) -> lctrl_core::Result<Vec<lctrl_core::TemperatureSensor>> {
        let sample = self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(LctrlError::ChannelUnavailable {
                channel: "fixture thermal source".into(),
            });
        }
        let value_c = match sample {
            0 => 42.5,
            1 => 43.5,
            2 => 44.5,
            _ => 45.5,
        };
        Ok(vec![
            lctrl_core::TemperatureSensor {
                metadata: lctrl_core::TemperatureSensorMetadata {
                    id: "cpu0".into(),
                    name: "CPU package".into(),
                    source: lctrl_core::TemperatureSource::Sysfs,
                    location: lctrl_core::TemperatureLocation::Cpu,
                    availability: Availability::Available,
                },
                value_c: Some(value_c),
            },
            lctrl_core::TemperatureSensor {
                metadata: lctrl_core::TemperatureSensorMetadata {
                    id: "gpu0".into(),
                    name: "GPU".into(),
                    source: lctrl_core::TemperatureSource::Sysfs,
                    location: lctrl_core::TemperatureLocation::Gpu,
                    availability: Availability::Unavailable,
                },
                value_c: None,
            },
        ])
    }

    fn temperature(&self, id: &str) -> lctrl_core::Result<lctrl_core::TemperatureSensor> {
        self.temperature_sensors()?
            .into_iter()
            .find(|sensor| sensor.metadata.id == id)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("unknown sensor {id}"),
            })
    }
}

struct FixedDbdc {
    profile: lctrl_core::DbdcProfile,
}

impl DbdcControl for FixedDbdc {
    fn dbdc_profile(&self) -> lctrl_core::Result<lctrl_core::DbdcProfile> {
        Ok(self.profile.clone())
    }
}

struct ToggleUsb {
    state: Mutex<lctrl_core::UsbChargingState>,
    writes: AtomicUsize,
}

impl ToggleUsb {
    fn on() -> Self {
        Self {
            state: Mutex::new(lctrl_core::UsbChargingState {
                ac_supported: true,
                ac_enabled: true,
                battery_supported: true,
                battery_enabled: false,
            }),
            writes: AtomicUsize::new(0),
        }
    }
}

impl UsbControl for ToggleUsb {
    fn usb_charging(&self) -> lctrl_core::Result<lctrl_core::UsbChargingState> {
        Ok(*self.state.lock())
    }

    fn set_usb_ac_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> lctrl_core::Result<lctrl_core::ChangeReport<lctrl_core::UsbChargingState>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let previous = *self.state.lock();
        let mut requested = previous;
        requested.ac_enabled = enabled;
        if apply == ApplyMode::Commit {
            self.state.lock().ac_enabled = enabled;
        }
        Ok(if apply == ApplyMode::Commit {
            lctrl_core::ChangeReport::committed(previous, requested, requested)
        } else {
            lctrl_core::ChangeReport::dry_run(previous, requested)
        })
    }

    fn set_usb_battery_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> lctrl_core::Result<lctrl_core::ChangeReport<lctrl_core::UsbChargingState>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let previous = *self.state.lock();
        let mut requested = previous;
        requested.battery_enabled = enabled;
        if apply == ApplyMode::Commit {
            self.state.lock().battery_enabled = enabled;
        }
        Ok(if apply == ApplyMode::Commit {
            lctrl_core::ChangeReport::committed(previous, requested, requested)
        } else {
            lctrl_core::ChangeReport::dry_run(previous, requested)
        })
    }
}

#[test]
fn perf_dbdc_get_reports_vendor_table() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "dbdc", "get"])
        .expect("parse perf dbdc get");
    let hal = FakeHal::new("unused");
    let dbdc = FixedDbdc {
        profile: lctrl_core::DbdcProfile {
            counts: 2,
            current_limit_ma: vec![7500, 4500],
            ros_power_mw: vec![65000, 35000],
            threshold_pct: vec![100, 20],
        },
    };

    let output = execute_with_services(cli, CommandServices::new(&hal).with_dbdc(&dbdc))
        .expect("dbdc get succeeds");

    assert_eq!(output.json["counts"], 2);
    assert_eq!(output.json["current_limit_ma"], json!([7500, 4500]));
    assert_eq!(output.json["threshold_pct"], json!([100, 20]));
    let text = &output.human;
    assert!(text.contains("tier 0"));
    assert!(text.contains("7500 mA"));
    assert!(text.contains("tier 1"));
}

#[test]
fn perf_dbdc_get_without_service_is_structured_unsupported() {
    let cli = Cli::try_parse_from(["vantage", "perf", "dbdc", "get"]).expect("parse");
    let hal = FakeHal::new("unused");

    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("missing dbdc service must not succeed");

    assert!(matches!(error, LctrlError::Unsupported { .. }));
}

struct FixedGpuOc {
    status: lctrl_core::GpuOcStatus,
}

impl GpuOcControl for FixedGpuOc {
    fn gpu_oc_status(&self) -> lctrl_core::Result<lctrl_core::GpuOcStatus> {
        Ok(self.status.clone())
    }
}

#[test]
fn perf_gpu_status_reports_the_probe_and_the_missing_parameter_block() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "gpu", "status"])
        .expect("parse perf gpu status");
    let hal = FakeHal::new("unused");
    let gpu_oc = FixedGpuOc {
        status: lctrl_core::GpuOcStatus {
            supported: false,
            probe: lctrl_core::GpuOcSupportProbe {
                method: "LENOVO_GAMEZONE_DATA.IsSupportGpuOC".into(),
                return_value: true,
                data: 0,
            },
            parameter: None,
            parameter_error: Some(
                "feature is unsupported: ROOT\\WMI query: SELECT * FROM LENOVO_GAMEZONE_GPU_OC_DATA"
                    .into(),
            ),
        },
    };

    let output = execute_with_services(cli, CommandServices::new(&hal).with_gpu_oc(&gpu_oc))
        .expect("gpu status succeeds");

    assert!(!output.json["supported"].as_bool().unwrap());
    assert_eq!(output.json["probe"]["data"], 0);
    assert_eq!(
        output.json["probe"]["method"],
        "LENOVO_GAMEZONE_DATA.IsSupportGpuOC"
    );
    assert!(output.json["parameter"].is_null());
    assert!(
        output.json["parameter_error"]
            .as_str()
            .unwrap()
            .contains("LENOVO_GAMEZONE_GPU_OC_DATA")
    );
    let text = &output.human;
    assert!(text.contains("not supported by the firmware"));
    assert!(text.contains("OC parameter block: unavailable"));
    assert!(text.contains("write: unsupported"));
}

#[test]
fn perf_gpu_without_service_is_structured_unsupported() {
    let cli = Cli::try_parse_from(["vantage", "perf", "gpu"]).expect("parse perf gpu");
    let hal = FakeHal::new("unused");

    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("missing gpu OC service must not succeed");

    assert!(matches!(error, LctrlError::Unsupported { .. }));
}

#[test]
fn usb_charge_on_battery_dry_run_does_not_write() {
    let cli = Cli::try_parse_from(["vantage", "--dry-run", "usb", "charge-on-battery", "on"])
        .expect("parse");
    let hal = FakeHal::new("unused");
    let usb = ToggleUsb::on();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_usb(&usb))
        .expect("usb dry run succeeds");

    assert_eq!(usb.writes.load(Ordering::SeqCst), 1);
    assert!(!usb.usb_charging().unwrap().battery_enabled);
    assert_eq!(output.json["mode"], "dry_run");
}

#[test]
fn usb_without_service_is_structured_unsupported() {
    let cli = Cli::try_parse_from(["vantage", "usb", "always-on", "on"]).expect("parse");
    let hal = FakeHal::new("unused");

    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("missing usb service must not succeed");

    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert!(error.to_string().contains("usb.always-on"));
}

#[derive(Default)]
struct TelemetryBattery {
    writes: AtomicUsize,
}

impl BatteryControl for TelemetryBattery {
    fn battery_telemetry(&self, _: u32) -> lctrl_core::Result<lctrl_core::BatteryTelemetry> {
        Err(LctrlError::ChannelUnavailable {
            channel: "fixture battery".into(),
        })
    }

    fn adapter_info(&self) -> lctrl_core::Result<AdapterInfo> {
        Err(LctrlError::Unsupported {
            feature: "fixture adapter".into(),
        })
    }

    fn charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "fixture charge mode".into(),
        })
    }

    fn set_charge_mode(
        &self,
        _: ChargeMode,
        _: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(LctrlError::Unsupported {
            feature: "fixture charge mode".into(),
        })
    }
}

#[derive(Default)]
struct TelemetryPerformance {
    writes: AtomicUsize,
}

impl PerformanceControl for TelemetryPerformance {
    fn performance_state(&self) -> lctrl_core::Result<lctrl_core::PerformanceState> {
        Err(LctrlError::ChannelUnavailable {
            channel: "fixture performance".into(),
        })
    }

    fn set_performance_mode(
        &self,
        _: lctrl_core::PerformanceMode,
        _: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PerformanceMode>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(LctrlError::Unsupported {
            feature: "fixture performance".into(),
        })
    }
}

#[test]
fn stream_intervals_are_positive_and_specified_in_seconds() {
    let invalid_args: &[&[&str]] = &[
        &["vantage", "tune", "telemetry", "--interval", "0"],
        &["vantage", "perf", "temp", "watch", "--interval", "0"],
        &["vantage", "tune", "telemetry", "--interval", "1.5"],
        &["vantage", "perf", "temp", "watch", "--interval", "-1"],
    ];
    for args in invalid_args {
        assert!(Cli::try_parse_from(*args).is_err());
    }

    let valid: &[(&[&str], StreamKind, u64)] = &[
        (
            &["vantage", "tune", "telemetry", "--interval", "3"],
            StreamKind::Telemetry,
            3,
        ),
        (
            &["vantage", "perf", "temp", "watch"],
            StreamKind::Temperatures,
            1,
        ),
        (
            &["vantage", "perf", "temp", "watch", "--interval", "2"],
            StreamKind::Temperatures,
            2,
        ),
    ];
    for (args, kind, seconds) in valid {
        let cli = Cli::try_parse_from(*args).unwrap();
        assert_eq!(
            stream_spec(&cli).unwrap(),
            Some(StreamSpec {
                kind: *kind,
                interval: Duration::from_secs(*seconds),
            })
        );
    }

    let zero_commands = [
        Command::Tune {
            command: Some(TuneCommand::Telemetry { interval: Some(0) }),
        },
        Command::Perf {
            command: Some(PerfCommand::Temp {
                command: Some(TempCommand::Watch { interval: 0 }),
            }),
        },
    ];
    let hal = FakeHal::new("unused");
    let temperatures = ChangingTemperatures::new(false);
    for command in zero_commands {
        let cli = Cli {
            json: false,
            dry_run: false,
            yes: false,
            auto_restore: false,
            command,
        };
        assert!(matches!(
            stream_spec(&cli),
            Err(LctrlError::InvalidArgument { .. })
        ));
        assert!(matches!(
            execute_with_services(
                cli,
                CommandServices::new(&hal).with_temperature(&temperatures)
            ),
            Err(LctrlError::InvalidArgument { .. })
        ));
    }
    assert_eq!(temperatures.reads.load(Ordering::SeqCst), 0);
}

#[test]
fn telemetry_without_interval_is_one_read_only_sample() {
    let cli = Cli::try_parse_from(["vantage", "tune", "telemetry"]).unwrap();
    assert_eq!(stream_spec(&cli).unwrap(), None);
    let hal = FakeHal::new("unused");
    let temperatures = ChangingTemperatures::new(false);
    let tuning = FakeTune {
        epp_calls: Mutex::new(Vec::new()),
    };

    let output = execute_with_services(
        cli,
        CommandServices::new(&hal)
            .with_temperature(&temperatures)
            .with_tuning(&tuning),
    )
    .unwrap();

    assert_eq!(temperatures.reads.load(Ordering::SeqCst), 1);
    assert_eq!(output.json["temperatures"][0][0]["value_c"], 42.5);
    assert_eq!(
        output.json["temperatures"][0][0]["metadata"]["source"],
        "sysfs"
    );
    assert!(output.json["temperatures"][0][1]["value_c"].is_null());
    assert_eq!(output.json["epp"][0], 128);
    assert_eq!(output.json["pl1_uw"][1]["error"]["kind"], "unsupported");
    assert!(output.human.contains("42.5"));
    assert!(output.human.contains("sysfs"));
    assert!(output.human.contains("128"));
    assert!(output.human.contains("unavailable"));
    assert!(tuning.epp_calls.lock().is_empty());
}

#[test]
fn single_result_execution_rejects_streams_before_sampling() {
    let commands: &[&[&str]] = &[
        &["vantage", "tune", "telemetry", "--interval", "1"],
        &["vantage", "perf", "temp", "watch"],
    ];
    let hal = FakeHal::new("unused");
    let temperatures = ChangingTemperatures::new(false);
    let tuning = FakeTune {
        epp_calls: Mutex::new(Vec::new()),
    };
    for args in commands {
        let cli = Cli::try_parse_from(*args).unwrap();
        assert!(matches!(
            execute(cli.clone(), &hal),
            Err(LctrlError::Unsupported { .. })
        ));
        assert!(matches!(
            execute_with_services(
                cli,
                CommandServices::new(&hal)
                    .with_temperature(&temperatures)
                    .with_tuning(&tuning),
            ),
            Err(LctrlError::Unsupported { .. })
        ));
    }
    assert_eq!(temperatures.reads.load(Ordering::SeqCst), 0);
    assert!(tuning.epp_calls.lock().is_empty());
}

#[test]
fn successive_stream_samples_observe_changing_sensors_without_writes() {
    let hal = FakeHal::new("unused");
    let temperatures = ChangingTemperatures::new(false);
    let battery = TelemetryBattery::default();
    let performance = TelemetryPerformance::default();
    let tuning = FakeTune {
        epp_calls: Mutex::new(Vec::new()),
    };
    let services = CommandServices::new(&hal)
        .with_temperature(&temperatures)
        .with_battery(&battery)
        .with_performance(&performance)
        .with_tuning(&tuning);

    let first = sample_stream(StreamKind::Telemetry, &services).unwrap();
    let second = sample_stream(StreamKind::Telemetry, &services).unwrap();
    let third = sample_stream(StreamKind::Temperatures, &services).unwrap();
    let fourth = sample_stream(StreamKind::Temperatures, &services).unwrap();

    assert_eq!(first.json["temperatures"][0][0]["value_c"], 42.5);
    assert_eq!(second.json["temperatures"][0][0]["value_c"], 43.5);
    assert_eq!(first.json["epp"][0], 128);
    for field in ["pl1_uw", "pl2_uw", "tau_us"] {
        assert_eq!(first.json[field][1]["error"]["kind"], "unsupported");
    }
    assert_eq!(
        first.json["device_battery"][1]["error"]["channel"],
        "fixture battery"
    );
    assert_eq!(
        first.json["performance"][1]["error"]["channel"],
        "fixture performance"
    );
    assert_eq!(first.json["os_battery"][1]["error"]["kind"], "unsupported");
    assert_eq!(
        first.json["battery_electrical"][1]["error"]["kind"],
        "unsupported"
    );
    assert_eq!(
        first.json["system_power"][1]["error"]["kind"],
        "unsupported"
    );
    assert_eq!(third.json[0]["value_c"], 44.5);
    assert_eq!(fourth.json[0]["value_c"], 45.5);
    assert_eq!(fourth.json[0]["metadata"]["id"], "cpu0");
    assert_eq!(fourth.json[0]["metadata"]["source"], "sysfs");
    assert!(fourth.json[1]["value_c"].is_null());
    assert!(fourth.human.contains("45.5"));
    assert!(fourth.human.contains("Sysfs"));
    assert!(fourth.human.contains("unavailable"));
    assert_eq!(temperatures.reads.load(Ordering::SeqCst), 4);
    assert_eq!(battery.writes.load(Ordering::SeqCst), 0);
    assert_eq!(performance.writes.load(Ordering::SeqCst), 0);
    assert!(tuning.epp_calls.lock().is_empty());
}

#[test]
fn temperature_observations_preserve_missing_and_read_error_provenance() {
    let hal = FakeHal::new("unused");
    let missing = CommandServices::new(&hal);
    let missing_output = execute(
        Cli::try_parse_from(["vantage", "tune", "telemetry"]).unwrap(),
        &hal,
    )
    .unwrap();
    assert!(missing_output.json["temperatures"][0].is_null());
    assert_eq!(
        missing_output.json["temperatures"][1]["error"]["kind"],
        "unsupported"
    );
    assert_eq!(
        missing_output.json["temperatures"][1]["error"]["feature"],
        "perf.temp"
    );
    assert!(missing_output.human.contains("unavailable"));
    assert!(matches!(
        sample_stream(StreamKind::Temperatures, &missing),
        Err(LctrlError::Unsupported { .. })
    ));

    let temperatures = ChangingTemperatures::new(true);
    let failing = CommandServices::new(&hal).with_temperature(&temperatures);
    let failed_output = execute_with_services(
        Cli::try_parse_from(["vantage", "tune", "telemetry"]).unwrap(),
        CommandServices::new(&hal).with_temperature(&temperatures),
    )
    .unwrap();
    assert!(failed_output.json["temperatures"][0].is_null());
    assert_eq!(
        failed_output.json["temperatures"][1]["error"]["kind"],
        "channel_unavailable"
    );
    assert_eq!(
        failed_output.json["temperatures"][1]["error"]["channel"],
        "fixture thermal source"
    );
    assert!(failed_output.human.contains("fixture thermal source"));
    let failed_sample = sample_stream(StreamKind::Telemetry, &failing).unwrap();
    assert_eq!(
        failed_sample.json["temperatures"],
        failed_output.json["temperatures"]
    );
    assert!(matches!(
        sample_stream(StreamKind::Temperatures, &failing),
        Err(LctrlError::ChannelUnavailable { channel }) if channel == "fixture thermal source"
    ));
}

#[test]
fn capability_output_describes_temperature_streams_as_read_only_polling() {
    let hal = FakeHal::new("unused");
    let output = execute(
        Cli::try_parse_from(["vantage", "capability"]).unwrap(),
        &hal,
    )
    .unwrap();
    let rows = output.json.as_array().unwrap();
    let watch = rows
        .iter()
        .find(|row| row["command"] == "perf.temp.watch")
        .unwrap();
    assert_eq!(watch["axes"]["read"], true);
    assert_eq!(watch["axes"]["write"], false);
    assert_ne!(watch["status"], "unsupported");
    assert!(
        watch["feature_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "perf.temp")
    );
    assert_eq!(watch["service_roles"], serde_json::json!(["temperature"]));

    let telemetry = rows
        .iter()
        .find(|row| row["command"] == "tune.telemetry")
        .unwrap();
    assert_eq!(telemetry["axes"]["read"], true);
    assert_eq!(telemetry["axes"]["write"], false);
    assert!(
        telemetry["feature_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "perf.temp")
    );
    assert!(
        telemetry["service_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "temperature")
    );
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
    // A bare parent command is a usage error (InvalidArgument), so the core
    // mapping is pinned with a command that is still a genuine Unsupported
    // verdict rather than a missing subcommand.
    let cli = Cli::try_parse_from(["vantage", "tune", "turbo", "on"]).expect("parse tune turbo");
    let result = execute(cli, &FakeHal::new("unused"));
    let error = result.expect_err("tune turbo is not implemented");

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
    charge_writes: AtomicUsize,
    wear_calls: AtomicUsize,
}

impl FakeBattery {
    const fn new() -> Self {
        Self {
            adapter_calls: AtomicUsize::new(0),
            charge_writes: AtomicUsize::new(0),
            wear_calls: AtomicUsize::new(0),
        }
    }
}

impl BatteryControl for FakeBattery {
    fn battery_telemetry(&self, _index: u32) -> lctrl_core::Result<lctrl_core::BatteryTelemetry> {
        Ok(lctrl_core::BatteryTelemetry {
            design_capacity_mwh: Some(99_900),
            full_charge_capacity_mwh: Some(93_810),
            remaining_capacity_mwh: None,
            voltage_mv: None,
            current_ma: None,
            temperature_deci_kelvin: None,
            manufacture_date: None,
            first_used_date: None,
            design_voltage_mv: None,
            remaining_percent: None,
            life_percent: Some(93),
            charge_status: None,
            health: Some(lctrl_core::BatteryHealth::Green),
            remaining_time_min: None,
            charge_completion_time_min: None,
            wattage_w: None,
            cycle_count: None,
            manufacturer: None,
            model_name: None,
            firmware_version: None,
            serial_number: None,
            chemistry: None,
            source_issues: Vec::new(),
        })
    }

    fn battery_wear(&self) -> lctrl_core::Result<lctrl_core::BatteryWearReport> {
        self.wear_calls.fetch_add(1, Ordering::SeqCst);
        Ok(lctrl_core::BatteryWearReport::from_capacities(
            "fake_capacity_channel",
            lctrl_core::BatteryFieldProvenance {
                design_capacity: "fake:BatteryStaticData.DesignedCapacity",
                full_charge_capacity: "fake:BatteryFullChargedCapacity.FullChargedCapacity",
                cycle_count: "fake:BatteryCycleCount.CycleCount",
            },
            Some(99_900),
            Some(93_810),
            Some(233),
            Vec::new(),
        ))
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
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
        let previous = ChargeMode::Normal;
        if apply == ApplyMode::Commit {
            self.charge_writes.fetch_add(1, Ordering::SeqCst);
        }
        Ok(if apply == ApplyMode::Commit {
            lctrl_core::ChangeReport::committed(previous, mode, mode)
        } else {
            lctrl_core::ChangeReport::dry_run(previous, mode)
        })
    }
}

struct FakeConflicts;

impl ControlConflictDetection for FakeConflicts {
    fn active_vendor_controllers(&self) -> lctrl_core::Result<Vec<String>> {
        Ok(vec!["LenovoVantageService.exe".into()])
    }
}

#[derive(Default)]
struct FakeBrightness {
    reads: AtomicUsize,
    writes: Mutex<Vec<(u8, ApplyMode)>>,
}

impl PanelBrightnessControl for FakeBrightness {
    fn brightness(&self) -> lctrl_core::Result<lctrl_core::PanelBrightness> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(lctrl_core::PanelBrightness {
            current_percent: 80,
            supported_levels: vec![0, 50, 80, 100],
        })
    }

    fn set_brightness(
        &self,
        percent: u8,
        expected_current: Option<u8>,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<u8>> {
        if expected_current.is_some_and(|expected| expected != 80) {
            return Err(LctrlError::ChannelUnavailable {
                channel: "brightness precondition changed".into(),
            });
        }
        self.writes.lock().push((percent, apply));
        Ok(match apply {
            ApplyMode::DryRun => ChangeReport::dry_run(80, percent),
            ApplyMode::Commit => ChangeReport::committed(80, percent, percent),
        })
    }
}

#[test]
fn panel_brightness_reads_without_checking_conflicts() {
    struct FailOnConflict;
    impl ControlConflictDetection for FailOnConflict {
        fn active_vendor_controllers(&self) -> lctrl_core::Result<Vec<String>> {
            panic!("read must not query conflict detection")
        }
    }
    let hal = FakeHal::new("unused");
    let panel = FakeBrightness::default();
    let cli = Cli::try_parse_from(["vantage", "panel", "brightness"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal)
            .with_panel_brightness(&panel)
            .with_conflict_detection(&FailOnConflict),
    )
    .unwrap();
    assert_eq!(output.json["current_percent"], 80);
    assert_eq!(
        output.json["supported_levels"],
        serde_json::json!([0, 50, 80, 100])
    );
    assert_eq!(panel.reads.load(Ordering::SeqCst), 1);
    assert!(panel.writes.lock().is_empty());
}

#[test]
fn panel_brightness_missing_and_invalid_do_not_call_service() {
    let hal = FakeHal::new("unused");
    for args in [
        &["vantage", "panel", "brightness"][..],
        &["vantage", "panel", "brightness", "50"][..],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(execute_with_services(cli, CommandServices::new(&hal)),
            Err(LctrlError::Unsupported { feature }) if feature == "panel.brightness")
        );
    }
    let panel = FakeBrightness::default();
    assert!(Cli::try_parse_from(["vantage", "panel", "brightness", "101"]).is_err());
    assert!(Cli::try_parse_from(["vantage", "panel", "brightness", "256"]).is_err());
    let cli = vantage_cli::Cli {
        command: vantage_cli::Command::Panel {
            command: Some(vantage_cli::PanelCommand::Brightness {
                percent: Some(101),
                expect_current: None,
            }),
        },
        ..Cli::try_parse_from(["vantage", "--dry-run", "panel", "brightness"]).unwrap()
    };
    assert!(matches!(
        execute_with_services(
            cli,
            CommandServices::new(&hal).with_panel_brightness(&panel)
        ),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert_eq!(panel.reads.load(Ordering::SeqCst), 0);
    assert!(panel.writes.lock().is_empty());
}

#[test]
fn panel_brightness_expected_current_prevents_stale_writes() {
    let hal = FakeHal::new("unused");
    let panel = FakeBrightness::default();
    assert!(
        Cli::try_parse_from(["vantage", "panel", "brightness", "--expect-current", "80"]).is_err()
    );
    let cli = Cli::try_parse_from([
        "vantage",
        "panel",
        "brightness",
        "50",
        "--expect-current",
        "90",
    ])
    .unwrap();
    assert!(matches!(
        execute_with_services(
            cli,
            CommandServices::new(&hal).with_panel_brightness(&panel)
        ),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(panel.writes.lock().is_empty());
}

#[test]
fn panel_brightness_write_obeys_conflict_and_apply_mode() {
    let hal = FakeHal::new("unused");
    let panel = FakeBrightness::default();
    let services = || {
        CommandServices::new(&hal)
            .with_panel_brightness(&panel)
            .with_conflict_detection(&FakeConflicts)
    };
    let cli = Cli::try_parse_from(["vantage", "--json", "panel", "brightness", "50"]).unwrap();
    assert!(matches!(
        execute_with_services(cli, services()),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(panel.writes.lock().is_empty());
    let cli = Cli::try_parse_from(["vantage", "--dry-run", "panel", "brightness", "50"]).unwrap();
    let output = execute_with_services(cli, services()).unwrap();
    assert_eq!(output.json["mode"], "dry_run");
    assert_eq!(output.json["previous"], 80);
    assert_eq!(output.json["requested"], 50);
    assert!(output.json.get("actual").is_none());
    let cli = Cli::try_parse_from(["vantage", "--yes", "panel", "brightness", "100"]).unwrap();
    let output = execute_with_services(cli, services()).unwrap();
    assert_eq!(output.json["mode"], "commit");
    assert_eq!(
        *panel.writes.lock(),
        vec![(50, ApplyMode::DryRun), (100, ApplyMode::Commit)]
    );
}

#[test]
fn touchpad_status_is_read_only_even_with_a_vendor_controller_running() {
    struct StatusOnly;
    impl lctrl_hal::TouchpadControl for StatusOnly {
        fn touchpad_state(&self) -> lctrl_core::Result<lctrl_core::DeviceState> {
            Ok(lctrl_core::DeviceState::Enabled)
        }
        fn set_touchpad(
            &self,
            _: lctrl_core::DeviceState,
            _: ApplyMode,
        ) -> lctrl_core::Result<lctrl_core::ChangeReport<lctrl_core::DeviceState>> {
            panic!("status must not mutate the touchpad")
        }
    }
    let hal = FakeHal::new("unused");
    let cli = Cli::try_parse_from(["vantage", "--json", "touchpad", "status"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal)
            .with_touchpad(&StatusOnly)
            .with_conflict_detection(&FakeConflicts),
    )
    .unwrap();
    assert_eq!(output.json["actual"], "enabled");
}

#[test]
fn touchpad_enable_and_disable_aliases_drive_the_requested_state() {
    struct RecordingTouchpad {
        writes: Mutex<Vec<(DeviceState, ApplyMode)>>,
    }
    impl lctrl_hal::TouchpadControl for RecordingTouchpad {
        fn touchpad_state(&self) -> lctrl_core::Result<DeviceState> {
            Ok(DeviceState::Disabled)
        }
        fn set_touchpad(
            &self,
            state: DeviceState,
            apply: ApplyMode,
        ) -> lctrl_core::Result<ChangeReport<DeviceState>> {
            self.writes.lock().push((state, apply));
            Ok(ChangeReport::committed(DeviceState::Disabled, state, state))
        }
    }
    let hal = FakeHal::new("unused");
    let touchpad = RecordingTouchpad {
        writes: Mutex::new(Vec::new()),
    };
    for (verb, expected) in [("enable", DeviceState::Enabled), ("disable", DeviceState::Disabled)] {
        let cli = Cli::try_parse_from(["vantage", "--json", "--yes", "touchpad", verb]).unwrap();
        let output = execute_with_services(
            cli,
            CommandServices::new(&hal)
                .with_touchpad(&touchpad)
                .with_conflict_detection(&FakeConflicts),
        )
        .unwrap();
        assert_eq!(output.json["requested"], json!(expected), "{verb}");
        assert_eq!(output.json["mode"], json!("commit"), "{verb}");
    }
    assert_eq!(
        *touchpad.writes.lock(),
        vec![
            (DeviceState::Enabled, ApplyMode::Commit),
            (DeviceState::Disabled, ApplyMode::Commit),
        ]
    );
}

#[test]
fn touchpad_recover_reports_the_chosen_strategy() {
    struct CyclingTouchpad;
    impl lctrl_hal::TouchpadControl for CyclingTouchpad {
        fn touchpad_state(&self) -> lctrl_core::Result<DeviceState> {
            Ok(DeviceState::Enabled)
        }
        fn set_touchpad(
            &self,
            _: DeviceState,
            _: ApplyMode,
        ) -> lctrl_core::Result<ChangeReport<DeviceState>> {
            panic!("recover must not take the plain touchpad switch path")
        }
        fn recover_touchpad(
            &self,
            apply: ApplyMode,
        ) -> lctrl_core::Result<lctrl_core::TouchpadRecoveryReport> {
            assert_eq!(apply, ApplyMode::Commit);
            Ok(lctrl_core::TouchpadRecoveryReport {
                mode: apply,
                strategy: lctrl_core::TouchpadRecoveryStrategy::CycleInputLeaves,
                leaves: vec![
                    lctrl_core::TouchpadRecoveryLeaf {
                        role: "digitizer",
                        previous: DeviceState::Enabled,
                        requested: DeviceState::Enabled,
                        actual: Some(DeviceState::Enabled),
                    },
                    lctrl_core::TouchpadRecoveryLeaf {
                        role: "mouse",
                        previous: DeviceState::Enabled,
                        requested: DeviceState::Enabled,
                        actual: Some(DeviceState::Enabled),
                    },
                ],
            })
        }
    }
    let hal = FakeHal::new("unused");
    let cli = Cli::try_parse_from(["vantage", "--json", "--yes", "touchpad", "recover"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal)
            .with_touchpad(&CyclingTouchpad)
            .with_conflict_detection(&FakeConflicts),
    )
    .unwrap();
    assert_eq!(output.json["strategy"], json!("cycle_input_leaves"));
    assert_eq!(output.json["mode"], json!("commit"));
    assert!(output.human.contains("restarted"), "{}", output.human);
    assert!(
        output.human.contains("not press or haptic function"),
        "PnP readback must not be reported as functional proof: {}",
        output.human
    );
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
    assert_eq!(output.json["vendor_adapter"]["authentication"], "inbox");
    assert_eq!(output.json["vendor_adapter"]["has_detail"], false);
    assert_eq!(output.json["source_association"], "unverified");
    assert!(output.json["system_power"].is_null());
    assert!(!output.json["system_power_error"].is_null());
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

#[test]
fn battery_health_reports_wear_with_per_field_provenance() {
    let cli = Cli::try_parse_from(["vantage", "--json", "battery", "health"]).unwrap();
    let hal = FakeHal::new("unused");
    let battery = FakeBattery::new();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_battery(&battery))
        .expect("health succeeds with service");

    assert_eq!(battery.wear_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        output.json["capacity_wear"]["design_capacity_mwh"],
        99_900
    );
    assert_eq!(
        output.json["capacity_wear"]["full_charge_capacity_mwh"],
        93_810
    );
    assert_eq!(output.json["capacity_wear"]["cycle_count"], 233);
    assert_eq!(output.json["capacity_wear"]["capacity_ratio_percent"], 93);
    assert_eq!(output.json["capacity_wear"]["wear_percent"], 6);
    assert_eq!(
        output.json["capacity_wear"]["fields"]["design_capacity"],
        "fake:BatteryStaticData.DesignedCapacity"
    );
    assert_eq!(
        output.json["capacity_wear"]["fields"]["cycle_count"],
        "fake:BatteryCycleCount.CycleCount"
    );
    assert_eq!(output.json["source_association"], "unverified");
    assert_eq!(hal.info_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn battery_health_stays_unsupported_without_service() {
    let cli = Cli::try_parse_from(["vantage", "battery", "health"]).unwrap();
    let hal = FakeHal::new("unused");

    assert!(matches!(
        execute(cli, &hal),
        Err(LctrlError::Unsupported { feature }) if feature == "battery.health"
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

    fn read_power_setting(
        &self,
        _scheme: &lctrl_core::PowerSchemeId,
        _key: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerSettingObservation> {
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
    isolated: bool,
}

impl FakeBios {
    const fn new() -> Self {
        Self {
            staged: Mutex::new(Vec::new()),
            saves: AtomicUsize::new(0),
            discards: AtomicUsize::new(0),
            fail_save: false,
            isolated: true,
        }
    }

    const fn failing_save() -> Self {
        Self {
            staged: Mutex::new(Vec::new()),
            saves: AtomicUsize::new(0),
            discards: AtomicUsize::new(0),
            fail_save: true,
            isolated: true,
        }
    }
}

impl BiosControl for FakeBios {
    fn require_staging_contract(&self) -> lctrl_core::Result<()> {
        if self.isolated {
            Ok(())
        } else {
            Err(LctrlError::ChannelUnavailable {
                channel: "unverified staging ownership".into(),
            })
        }
    }

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
fn unverified_bios_staging_blocks_set_and_discard_before_mutation() {
    let hal = FakeHal::new("unused");
    let mut bios = FakeBios::new();
    bios.isolated = false;
    let set = Cli::try_parse_from([
        "vantage", "--yes", "bios", "set", "Camera", "Disable", "--save",
    ])
    .unwrap();
    assert!(matches!(
        execute_with_services(set, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    let discard = Cli::try_parse_from(["vantage", "--yes", "bios", "discard"]).unwrap();
    assert!(matches!(
        execute_with_services(discard, CommandServices::new(&hal).with_bios(&bios)),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(bios.staged.lock().is_empty());
    assert_eq!(bios.saves.load(Ordering::SeqCst), 0);
    assert_eq!(bios.discards.load(Ordering::SeqCst), 0);
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

    fn microphone_state(&self) -> lctrl_core::Result<DeviceState> {
        Ok(DeviceState::Enabled)
    }

    fn set_microphone(
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
fn camera_status_reports_both_layers_without_mutating() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "status"]).unwrap();
    let hal = FakeHal::new("unused");
    let bios = FakeBios::new();

    let output = execute_with_services(
        cli,
        CommandServices::new(&hal)
            .with_privacy(&FakePrivacy)
            .with_bios(&bios),
    )
    .expect("camera status");

    assert_eq!(output.json["runtime"], "enabled");
    assert_eq!(output.json["persistent"]["value"], "Enable");
    // This fake implements no per-devnode read, and the status command reports
    // that instead of inventing devnodes.
    assert_eq!(output.json["devices_error"]["kind"], "unsupported");
    assert!(
        output.json["devices"]
            .as_array()
            .is_some_and(|devices| devices.is_empty())
    );
    // The occlusion channel stays closed: no WinRT session is opened here.
    assert_eq!(output.json["occlusion"]["supported"], false);
    assert!(output.json["occlusion"]["reason"].is_string());
    assert!(bios.staged.lock().is_empty());
    assert_eq!(bios.saves.load(Ordering::SeqCst), 0);
}

#[test]
fn camera_alias_accepts_enable_and_disable_spellings() {
    let cli =
        Cli::try_parse_from(["vantage", "privacy", "camera", "disable", "--runtime"]).unwrap();

    assert!(matches!(
        cli.command,
        Command::Privacy {
            command: Some(PrivacyCommand::Cam {
                action: PrivacyCamAction::Off,
                runtime: true,
                persistent: false,
            }),
        }
    ));
}

/// A privacy service that answers the consent-store read and refuses every
/// device question: `privacy cam inuse` must not read the PnP group, open a
/// device, or write one.
struct FakeCameraConsent;

impl PrivacyControl for FakeCameraConsent {
    fn camera_state(&self) -> lctrl_core::Result<DeviceState> {
        unreachable!("camera inuse must not read the PnP camera group")
    }

    fn set_camera(
        &self,
        _state: DeviceState,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<DeviceState>> {
        unreachable!("camera inuse must not write a camera devnode")
    }

    fn microphone_state(&self) -> lctrl_core::Result<DeviceState> {
        unreachable!("camera inuse must not read the microphone")
    }

    fn set_microphone(
        &self,
        _state: DeviceState,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<DeviceState>> {
        unreachable!("camera inuse must not write the microphone")
    }

    fn camera_consent(&self) -> lctrl_core::Result<lctrl_core::CameraConsentReport> {
        use lctrl_core::{
            CameraConsent, CameraConsentEntry, CameraConsentReport, CameraConsentScope,
            CameraConsentScopeReport, CameraPrivacyPolicy,
        };

        Ok(CameraConsentReport::new(
            vec![
                CameraConsentScopeReport {
                    scope: CameraConsentScope::CurrentUser,
                    present: true,
                    consent: Some(CameraConsent::Allow),
                    consent_raw: None,
                    last_set: None,
                    entries: Vec::new(),
                    notes: Vec::new(),
                },
                CameraConsentScopeReport {
                    scope: CameraConsentScope::CurrentUserNonPackaged,
                    present: true,
                    // A record may carry use stamps and no consent text; the
                    // container's `Allow` is not inherited into it.
                    consent: None,
                    consent_raw: None,
                    last_set: None,
                    entries: vec![CameraConsentEntry::new(
                        CameraConsentScope::CurrentUserNonPackaged,
                        "C:#Program Files#Google#Chrome#Application#chrome.exe",
                        None,
                        None,
                        Some(20),
                        Some(10),
                    )],
                    notes: Vec::new(),
                },
                CameraConsentScopeReport {
                    scope: CameraConsentScope::Machine,
                    present: true,
                    consent: Some(CameraConsent::Deny),
                    consent_raw: None,
                    last_set: None,
                    entries: Vec::new(),
                    notes: Vec::new(),
                },
            ],
            CameraPrivacyPolicy::default(),
        ))
    }
}

#[test]
fn camera_inuse_reports_each_consent_store_separately_without_mutation() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "camera", "inuse"]).unwrap();
    assert!(matches!(
        cli.command,
        Command::Privacy {
            command: Some(PrivacyCommand::Cam {
                action: PrivacyCamAction::InUse,
                runtime: false,
                persistent: false,
            }),
        }
    ));
    let hal = FakeHal::new("unused");

    let output = execute_with_services(
        cli,
        CommandServices::new(&hal).with_privacy(&FakeCameraConsent),
    )
    .expect("camera inuse");

    // The caveat is part of the answer, in JSON and in the terminal text.
    assert_eq!(
        output.json["as_recorded"],
        "as recorded by CapabilityAccessManager; last-use stamps are not proof that a camera is streaming"
    );
    assert_eq!(
        output.json["in_use_rule"],
        "LastUsedTimeStart > LastUsedTimeStop, both non-zero"
    );

    let scopes = output.json["scopes"].as_array().expect("scopes array");
    assert_eq!(scopes.len(), 3);
    // A machine `Deny` and a user `Allow` stay separate answers.
    assert_eq!(scopes[0]["scope"], "current_user");
    assert_eq!(scopes[0]["consent"], "allow");
    assert_eq!(scopes[1]["scope"], "current_user_non_packaged");
    assert!(scopes[1]["consent"].is_null());
    assert_eq!(scopes[2]["scope"], "machine");
    assert_eq!(scopes[2]["consent"], "deny");

    // The encoded `#` name decodes to the executable path it stores.
    assert_eq!(
        scopes[1]["entries"][0]["key"],
        "C:#Program Files#Google#Chrome#Application#chrome.exe"
    );
    assert_eq!(
        scopes[1]["entries"][0]["path"],
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
    );
    assert_eq!(scopes[1]["entries"][0]["in_use"], true);

    // The policy key is absent here, and absence is not reported as consent.
    assert_eq!(output.json["policy"]["present"], false);
    assert!(output.json["policy"]["let_apps_access_camera"].is_null());

    assert!(
        output
            .human
            .contains("as recorded by CapabilityAccessManager")
    );
    assert!(output.human.contains("in-use rule: LastUsedTimeStart"));
    assert!(output.human.contains("not a live stream"));
    assert!(output.human.contains("this is not an allow"));
}

#[test]
fn camera_inuse_accepts_the_hyphenated_spelling() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "in-use"]).unwrap();

    assert!(matches!(
        cli.command,
        Command::Privacy {
            command: Some(PrivacyCommand::Cam {
                action: PrivacyCamAction::InUse,
                ..
            }),
        }
    ));
}

#[test]
fn camera_inuse_rejects_a_layer_flag_without_reading_anything() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "inuse", "--runtime"]).unwrap();
    let hal = FakeHal::new("unused");

    // `FakeCameraConsent` panics if the service is asked anything at all, so a
    // rejected flag proves the read never started.
    let error = execute_with_services(
        cli,
        CommandServices::new(&hal).with_privacy(&FakeCameraConsent),
    )
    .expect_err("inuse must reject a layer flag");

    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
}

#[test]
fn camera_inuse_without_the_channel_is_unsupported_not_empty_success() {
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "inuse"]).unwrap();
    let hal = FakeHal::new("unused");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_privacy(&FakePrivacy))
        .expect_err("a backend without the consent store must not report an empty success");

    assert!(matches!(
        error,
        LctrlError::Unsupported { feature } if feature == "privacy.cam.inuse"
    ));
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

struct FakeSchemes {
    mutations: Mutex<Vec<lctrl_core::PowerMutation>>,
}

impl PowerControl for FakeSchemes {
    fn power_schemes(&self) -> lctrl_core::Result<Vec<lctrl_core::PowerScheme>> {
        Ok(vec![lctrl_core::PowerScheme {
            id: lctrl_core::PowerSchemeId::new("scheme-guid")?,
            name: "Balanced".into(),
            active: false,
        }])
    }

    fn active_power_scheme(&self) -> lctrl_core::Result<lctrl_core::PowerScheme> {
        unreachable!()
    }

    fn read_power_setting(
        &self,
        _scheme: &lctrl_core::PowerSchemeId,
        _key: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerSettingObservation> {
        unreachable!()
    }

    fn power_value_range(
        &self,
        _key: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerValueRange> {
        unreachable!()
    }

    fn apply_power_mutation(
        &self,
        mutation: lctrl_core::PowerMutation,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PowerMutation>> {
        self.mutations.lock().push(mutation.clone());
        Ok(ChangeReport::dry_run(mutation.clone(), mutation))
    }
}

#[test]
fn power_scheme_get_and_apply_resolve_the_same_friendly_name() {
    let hal = FakeHal::new("unused");
    let power = FakeSchemes {
        mutations: Mutex::new(Vec::new()),
    };
    let get = Cli::try_parse_from(["vantage", "power", "scheme", "get", "balanced"]).unwrap();
    let read = execute_with_services(get, CommandServices::new(&hal).with_power(&power)).unwrap();
    assert_eq!(
        read.json,
        serde_json::json!({"id": "scheme-guid", "name": "Balanced", "active": false})
    );
    assert_eq!(read.human, "power scheme: Balanced\n");
    assert!(power.mutations.lock().is_empty());

    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "power",
        "scheme",
        "apply",
        "balanced",
    ])
    .unwrap();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_power(&power)).unwrap();

    assert_eq!(
        &*power.mutations.lock(),
        &[lctrl_core::PowerMutation::Activate(
            lctrl_core::PowerSchemeId::new("scheme-guid").unwrap()
        )]
    );
    assert_eq!(output.json["mode"], "dry_run");
}

#[test]
fn unknown_power_scheme_name_is_rejected_without_activation() {
    let hal = FakeHal::new("unused");
    let power = FakeSchemes {
        mutations: Mutex::new(Vec::new()),
    };
    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "power",
        "scheme",
        "apply",
        "missing",
    ])
    .unwrap();

    let result = execute_with_services(cli, CommandServices::new(&hal).with_power(&power));

    assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    assert!(power.mutations.lock().is_empty());
}

struct AmbiguousSchemes {
    schemes: Vec<lctrl_core::PowerScheme>,
    mutations: Mutex<Vec<lctrl_core::PowerMutation>>,
}

impl PowerControl for AmbiguousSchemes {
    fn power_schemes(&self) -> lctrl_core::Result<Vec<lctrl_core::PowerScheme>> {
        Ok(self.schemes.clone())
    }
    fn active_power_scheme(&self) -> lctrl_core::Result<lctrl_core::PowerScheme> {
        unreachable!()
    }
    fn read_power_setting(
        &self,
        _: &lctrl_core::PowerSchemeId,
        _: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerSettingObservation> {
        unreachable!()
    }
    fn power_value_range(
        &self,
        _: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerValueRange> {
        unreachable!()
    }
    fn apply_power_mutation(
        &self,
        mutation: lctrl_core::PowerMutation,
        _: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PowerMutation>> {
        self.mutations.lock().push(mutation.clone());
        Ok(ChangeReport::dry_run(mutation.clone(), mutation))
    }
}

#[test]
fn power_scheme_ambiguous_name_is_rejected_before_read_or_activation() {
    let hal = FakeHal::new("unused");
    let power = AmbiguousSchemes {
        schemes: vec![
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new("one").unwrap(),
                "Balanced",
                true,
            ),
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new("two").unwrap(),
                "BALANCED",
                false,
            ),
        ],
        mutations: Mutex::new(Vec::new()),
    };
    for verb in ["get", "apply"] {
        let cli =
            Cli::try_parse_from(["vantage", "--dry-run", "power", "scheme", verb, "balanced"])
                .unwrap();
        let result = execute_with_services(cli, CommandServices::new(&hal).with_power(&power));
        assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    }
    let cli = Cli::try_parse_from([
        "vantage",
        "power",
        "scheme",
        "get",
        "balanced",
        "--subgroup",
        SETTING_SUBGROUP,
        "--setting",
        FIRST_SETTING,
    ])
    .unwrap();
    let result = execute_with_services(cli, CommandServices::new(&hal).with_power(&power));
    assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    assert!(power.mutations.lock().is_empty());
}

#[test]
fn power_scheme_exact_id_takes_precedence_over_another_scheme_name() {
    let hal = FakeHal::new("unused");
    let power = AmbiguousSchemes {
        schemes: vec![
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new("one").unwrap(),
                "two",
                true,
            ),
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new("two").unwrap(),
                "Other",
                false,
            ),
        ],
        mutations: Mutex::new(Vec::new()),
    };
    let cli =
        Cli::try_parse_from(["vantage", "--dry-run", "power", "scheme", "apply", "two"]).unwrap();
    execute_with_services(cli, CommandServices::new(&hal).with_power(&power)).unwrap();
    assert_eq!(
        &*power.mutations.lock(),
        &[lctrl_core::PowerMutation::Activate(
            lctrl_core::PowerSchemeId::new("two").unwrap()
        )]
    );
}

const ACTIVE_SCHEME_ID: &str = "11111111-1111-4111-8111-111111111111";
const SELECTED_SCHEME_ID: &str = "22222222-2222-4222-8222-222222222222";
const SETTING_SUBGROUP: &str = "33333333-3333-4333-8333-333333333333";
const FIRST_SETTING: &str = "44444444-4444-4444-8444-444444444444";
const SECOND_SETTING: &str = "55555555-5555-4555-8555-555555555555";
const UNAVAILABLE_RANGE_DETAIL: &str = "fixture minimum is unavailable";

struct ObservedSchemes {
    schemes: Vec<lctrl_core::PowerScheme>,
    observations: Vec<lctrl_core::PowerSettingObservation>,
    reads: Mutex<Vec<(lctrl_core::PowerSchemeId, lctrl_core::PowerSettingKey)>>,
    fail_list: bool,
    fail_read: bool,
}

impl ObservedSchemes {
    fn new() -> Self {
        let schemes = vec![
            // An exact ID must win even when it is also another scheme's name.
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new(ACTIVE_SCHEME_ID).unwrap(),
                SELECTED_SCHEME_ID,
                true,
            ),
            lctrl_core::PowerScheme::new(
                lctrl_core::PowerSchemeId::new(SELECTED_SCHEME_ID).unwrap(),
                "Office",
                false,
            ),
        ];
        let observations = [
            (ACTIVE_SCHEME_ID, FIRST_SETTING, 9, 31, true, false, Some(1)),
            (SELECTED_SCHEME_ID, FIRST_SETTING, 26, 64, false, true, None),
            (
                SELECTED_SCHEME_ID,
                SECOND_SETTING,
                35,
                55,
                true,
                false,
                Some(5),
            ),
        ]
        .into_iter()
        .map(
            |(scheme, setting, ac, dc, ac_policy, dc_policy, increment)| {
                lctrl_core::PowerSettingObservation {
                    scheme: lctrl_core::PowerSchemeId::new(scheme).unwrap(),
                    key: lctrl_core::PowerSettingKey {
                        subgroup: lctrl_core::PowerGuid::new(SETTING_SUBGROUP).unwrap(),
                        setting: lctrl_core::PowerGuid::new(setting).unwrap(),
                    },
                    values: lctrl_core::PowerSourceValues::new(ac, dc),
                    range: match increment {
                        Some(increment) => lctrl_core::PowerRangeObservation::Available {
                            range: lctrl_core::PowerValueRange::new(0, 100, increment).unwrap(),
                        },
                        None => lctrl_core::PowerRangeObservation::Unavailable {
                            detail: UNAVAILABLE_RANGE_DETAIL.into(),
                        },
                    },
                    policy_overrides: lctrl_core::PowerSourceValues::new(ac_policy, dc_policy),
                }
            },
        )
        .collect();
        Self {
            schemes,
            observations,
            reads: Mutex::new(Vec::new()),
            fail_list: false,
            fail_read: false,
        }
    }
}

impl PowerControl for ObservedSchemes {
    fn power_schemes(&self) -> lctrl_core::Result<Vec<lctrl_core::PowerScheme>> {
        if self.fail_list {
            return Err(std::io::Error::from_raw_os_error(1234).into());
        }
        Ok(self.schemes.clone())
    }

    fn active_power_scheme(&self) -> lctrl_core::Result<lctrl_core::PowerScheme> {
        panic!("a selected-scheme observation must not resolve the active scheme")
    }

    fn read_power_setting(
        &self,
        scheme: &lctrl_core::PowerSchemeId,
        key: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerSettingObservation> {
        self.reads.lock().push((scheme.clone(), key.clone()));
        if self.fail_read {
            return Err(std::io::Error::from_raw_os_error(2345).into());
        }
        self.observations
            .iter()
            .find(|observation| &observation.scheme == scheme && &observation.key == key)
            .cloned()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "fixture setting".into(),
            })
    }

    fn power_value_range(
        &self,
        _: &lctrl_core::PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerValueRange> {
        panic!("the observation already includes range availability")
    }

    fn apply_power_mutation(
        &self,
        _: lctrl_core::PowerMutation,
        _: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<lctrl_core::PowerMutation>> {
        panic!("a setting observation must not mutate power configuration")
    }
}

fn get_power_setting(name: &str, setting: &str) -> Cli {
    Cli::try_parse_from([
        "vantage",
        "--json",
        "power",
        "scheme",
        "get",
        name,
        "--subgroup",
        SETTING_SUBGROUP,
        "--setting",
        setting,
    ])
    .unwrap()
}

#[test]
fn power_setting_get_requires_both_flags() {
    for args in [
        [
            "vantage",
            "power",
            "scheme",
            "get",
            "Office",
            "--subgroup",
            SETTING_SUBGROUP,
        ],
        [
            "vantage",
            "power",
            "scheme",
            "get",
            "Office",
            "--setting",
            FIRST_SETTING,
        ],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }

    // Publicly constructed commands must enforce the same pair invariant.
    let hal = FakeHal::new("unused");
    let power = ObservedSchemes::new();
    for (subgroup, setting) in [
        (Some(SETTING_SUBGROUP.to_owned()), None),
        (None, Some(FIRST_SETTING.to_owned())),
    ] {
        let cli = Cli {
            command: vantage_cli::Command::Power {
                command: Some(vantage_cli::PowerCommand::Scheme {
                    command: Some(vantage_cli::SchemeCommand::Get {
                        name: "Office".into(),
                        subgroup,
                        setting,
                    }),
                }),
            },
            ..Cli::try_parse_from(["vantage", "power", "scheme", "get", "Office"]).unwrap()
        };
        let result = execute_with_services(cli, CommandServices::new(&hal).with_power(&power));
        assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    }
    assert!(power.reads.lock().is_empty());
}

#[test]
fn power_setting_get_reports_selected_values_even_when_range_unavailable() {
    let hal = FakeHal::new("unused");
    let power = ObservedSchemes::new();
    let output = execute_with_services(
        get_power_setting("office", FIRST_SETTING),
        CommandServices::new(&hal)
            .with_power(&power)
            .with_conflict_detection(&FakeConflicts),
    )
    .unwrap();

    assert_eq!(
        output.json,
        serde_json::json!({
            "scheme": SELECTED_SCHEME_ID,
            "key": {"subgroup": SETTING_SUBGROUP, "setting": FIRST_SETTING},
            "values": {"ac": 26, "dc": 64},
            "range": {"status": "unavailable", "detail": UNAVAILABLE_RANGE_DETAIL},
            "policy_overrides": {"ac": false, "dc": true}
        })
    );
    let reads = power.reads.lock();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].0.as_str(), SELECTED_SCHEME_ID);
    assert_eq!(reads[0].1.subgroup.as_str(), SETTING_SUBGROUP);
    assert_eq!(reads[0].1.setting.as_str(), FIRST_SETTING);
}

#[test]
fn power_setting_get_resolves_exact_id_and_each_setting_independently() {
    let hal = FakeHal::new("unused");
    let power = ObservedSchemes::new();
    // The selected ID is also the active scheme's name; exact ID takes precedence.
    let cli = Cli::try_parse_from([
        "vantage",
        "--json",
        "power",
        "scheme",
        "get",
        "--setting",
        FIRST_SETTING,
        SELECTED_SCHEME_ID,
        "--subgroup",
        SETTING_SUBGROUP,
    ])
    .unwrap();
    let selected =
        execute_with_services(cli, CommandServices::new(&hal).with_power(&power)).unwrap();
    assert_eq!(selected.json["scheme"], SELECTED_SCHEME_ID);
    assert_eq!(
        selected.json["values"],
        serde_json::json!({"ac": 26, "dc": 64})
    );
    assert_eq!(
        selected.json["policy_overrides"],
        serde_json::json!({"ac": false, "dc": true})
    );

    let active = execute_with_services(
        get_power_setting(ACTIVE_SCHEME_ID, FIRST_SETTING),
        CommandServices::new(&hal).with_power(&power),
    )
    .unwrap();
    assert_eq!(active.json["scheme"], ACTIVE_SCHEME_ID);
    assert_eq!(
        active.json["values"],
        serde_json::json!({"ac": 9, "dc": 31})
    );
    assert_eq!(
        active.json["policy_overrides"],
        serde_json::json!({"ac": true, "dc": false})
    );

    let other_setting = execute_with_services(
        get_power_setting("Office", SECOND_SETTING),
        CommandServices::new(&hal).with_power(&power),
    )
    .unwrap();
    assert_eq!(
        other_setting.json,
        serde_json::json!({
            "scheme": SELECTED_SCHEME_ID,
            "key": {"subgroup": SETTING_SUBGROUP, "setting": SECOND_SETTING},
            "values": {"ac": 35, "dc": 55},
            "range": {"status": "available", "range": {"min": 0, "max": 100, "increment": 5}},
            "policy_overrides": {"ac": true, "dc": false}
        })
    );
    assert!(other_setting.human.contains("35"));
    assert!(other_setting.human.contains("55"));
    assert!(other_setting.human.contains("100"));
}

#[test]
fn power_setting_get_rejects_unresolvable_schemes_before_reading() {
    let hal = FakeHal::new("unused");
    let mut power = ObservedSchemes::new();
    let result = execute_with_services(
        get_power_setting("unknown", FIRST_SETTING),
        CommandServices::new(&hal).with_power(&power),
    );
    assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    assert!(power.reads.lock().is_empty());

    power.schemes.push(lctrl_core::PowerScheme::new(
        lctrl_core::PowerSchemeId::new(SELECTED_SCHEME_ID).unwrap(),
        "Duplicated ID",
        false,
    ));
    let result = execute_with_services(
        get_power_setting(SELECTED_SCHEME_ID, FIRST_SETTING),
        CommandServices::new(&hal).with_power(&power),
    );
    assert!(matches!(result, Err(LctrlError::InvalidArgument { .. })));
    assert!(power.reads.lock().is_empty());
}

#[test]
fn power_setting_get_propagates_enumeration_and_read_errors() {
    let hal = FakeHal::new("unused");
    for (fail_list, expected_code, expected_reads) in [(true, 1234, 0), (false, 2345, 1)] {
        let mut power = ObservedSchemes::new();
        power.fail_list = fail_list;
        power.fail_read = !fail_list;
        let result = execute_with_services(
            get_power_setting("Office", FIRST_SETTING),
            CommandServices::new(&hal).with_power(&power),
        );
        let Err(LctrlError::Io(error)) = result else {
            panic!("expected original I/O error");
        };
        assert_eq!(error.raw_os_error(), Some(expected_code));
        assert_eq!(power.reads.lock().len(), expected_reads);
    }
}

struct FakeTune {
    epp_calls: Mutex<Vec<(u8, ApplyMode)>>,
}

impl lctrl_hal::TuningControl for FakeTune {
    fn epp(&self) -> lctrl_core::Result<u8> {
        Ok(128)
    }

    fn set_epp(&self, value: u8, apply: ApplyMode) -> lctrl_core::Result<ChangeReport<u8>> {
        self.epp_calls.lock().push((value, apply));
        Ok(ChangeReport::dry_run(128, value))
    }

    fn power_limit(&self, _kind: lctrl_hal::PowerLimitKind) -> lctrl_core::Result<u64> {
        Err(LctrlError::Unsupported {
            feature: "test.power_limit".into(),
        })
    }

    fn set_power_limit(
        &self,
        _kind: lctrl_hal::PowerLimitKind,
        _value: u64,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<u64>> {
        unreachable!()
    }
}

#[test]
fn tune_restore_dry_run_neither_writes_nor_consumes_the_recovery_snapshot() {
    let _lock = SNAPSHOT_ENV_LOCK.lock().unwrap();
    let path = std::env::temp_dir().join(format!(
        "vantage-tune-restore-test-{}.json",
        std::process::id()
    ));
    let state = serde_json::json!({
        "version": 1,
        "platform": "linux",
        "hardware": {
            "product_name": "21VG",
            "family": "21VG",
            "bios_version": "1.07"
        },
        "active_profile": "balanced",
        "status": "active",
        "saved_before_apply": [{"target": "epp", "value": 64}]
    });
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    unsafe { std::env::set_var("VANTAGE_STATE_PATH", &path) };
    let hal = FakeHal::new("21VG");
    let tuning = FakeTune {
        epp_calls: Mutex::new(Vec::new()),
    };

    let dry_run = Cli::try_parse_from(["vantage", "--dry-run", "tune", "restore"]).unwrap();
    let preview =
        execute_with_services(dry_run, CommandServices::new(&hal).with_tuning(&tuning)).unwrap();

    assert_eq!(preview.json["changes"][0]["mode"], "dry_run");
    assert_eq!(&*tuning.epp_calls.lock(), &[(64, ApplyMode::DryRun)]);
    assert!(path.is_file());

    let mut invalid = state.clone();
    invalid["saved_before_apply"] = serde_json::json!([
        {"target": "power_limit", "kind": "invalid", "value_uw": 1},
        {"target": "epp", "value": 64}
    ]);
    std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let dry_run = Cli::try_parse_from(["vantage", "--dry-run", "tune", "restore"]).unwrap();
    assert!(
        execute_with_services(dry_run, CommandServices::new(&hal).with_tuning(&tuning)).is_err()
    );
    assert!(
        tuning
            .epp_calls
            .lock()
            .iter()
            .all(|(_, mode)| *mode == ApplyMode::DryRun)
    );
    assert!(path.is_file());
    std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

    let commit = Cli::try_parse_from(["vantage", "tune", "restore"]).unwrap();
    execute_with_services(commit, CommandServices::new(&hal).with_tuning(&tuning)).unwrap();

    assert_eq!(
        tuning.epp_calls.lock().last(),
        Some(&(64, ApplyMode::Commit))
    );
    assert!(!path.exists());
    unsafe { std::env::remove_var("VANTAGE_STATE_PATH") };
}

/// A Dolby endpoint that mirrors the vendor behaviour: switching Dolby off
/// keeps the last selected profile.
struct FakeAudio {
    state: Mutex<DolbyState>,
    writes: AtomicUsize,
}

impl FakeAudio {
    fn new(enabled: bool, profile: DolbyProfile) -> Self {
        Self {
            state: Mutex::new(DolbyState {
                enabled,
                active_profile: profile,
                sku: None,
                spatial_supported: None,
            }),
            writes: AtomicUsize::new(0),
        }
    }
}

impl AudioControl for FakeAudio {
    fn dolby_state(&self) -> lctrl_core::Result<DolbyState> {
        Ok(*self.state.lock())
    }

    fn set_dolby_profile(
        &self,
        profile: DolbyProfile,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<DolbyState>> {
        if apply == ApplyMode::Commit {
            self.writes.fetch_add(1, Ordering::SeqCst);
        }
        let previous = *self.state.lock();
        let mut requested = previous;
        match profile {
            DolbyProfile::Off => requested.enabled = false,
            selected => {
                requested.enabled = true;
                requested.active_profile = selected;
            }
        }
        if apply == ApplyMode::Commit {
            let mut state = self.state.lock();
            state.enabled = requested.enabled;
            if profile != DolbyProfile::Off {
                state.active_profile = requested.active_profile;
            }
        }
        Ok(if apply == ApplyMode::Commit {
            ChangeReport::committed(previous, requested, requested)
        } else {
            ChangeReport::dry_run(previous, requested)
        })
    }
}

/// An endpoint whose readback never agrees with the requested profile, so a
/// restore cannot be proven.
struct StubbornAudio {
    inner: FakeAudio,
}

impl AudioControl for StubbornAudio {
    fn dolby_state(&self) -> lctrl_core::Result<DolbyState> {
        let mut state = *self.inner.state.lock();
        state.active_profile = DolbyProfile::Music;
        Ok(state)
    }

    fn set_dolby_profile(
        &self,
        profile: DolbyProfile,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<DolbyState>> {
        self.inner.set_dolby_profile(profile, apply)
    }
}

#[test]
fn auto_restore_puts_the_previous_dolby_state_back() {
    let hal = FakeHal::new("21VG");
    let audio = FakeAudio::new(false, DolbyProfile::Dynamic);
    let cli = Cli::try_parse_from([
        "vantage",
        "--yes",
        "--auto-restore",
        "audio",
        "dolby",
        "movie",
    ])
    .expect("parse dolby command");

    let output = execute_with_services(cli, CommandServices::new(&hal).with_audio(&audio))
        .expect("dolby command succeeds");

    let state = *audio.state.lock();
    assert!(!state.enabled, "Dolby stays off after the restore");
    assert_eq!(
        state.active_profile,
        DolbyProfile::Dynamic,
        "the previous profile is selected again"
    );
    // One write plus the two restore steps: select the previous profile,
    // then switch Dolby off again.
    assert_eq!(audio.writes.load(Ordering::SeqCst), 3);
    assert!(output.human.contains("restored dolby"));
    assert!(output.json["auto_restore"].is_string());
}

#[test]
fn auto_restore_reports_a_state_that_cannot_be_proven() {
    let hal = FakeHal::new("21VG");
    let audio = StubbornAudio {
        inner: FakeAudio::new(false, DolbyProfile::Dynamic),
    };
    let cli = Cli::try_parse_from([
        "vantage",
        "--yes",
        "--auto-restore",
        "audio",
        "dolby",
        "movie",
    ])
    .expect("parse dolby command");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_audio(&audio))
        .expect_err("an unprovable restore is reported");
    assert!(
        matches!(error, LctrlError::FirmwareRejected { .. }),
        "unexpected error: {error:?}"
    );
}

#[test]
fn auto_restore_stays_dry_in_a_dry_run() {
    let hal = FakeHal::new("21VG");
    let audio = FakeAudio::new(false, DolbyProfile::Dynamic);
    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "--auto-restore",
        "audio",
        "dolby",
        "movie",
    ])
    .expect("parse dolby command");

    execute_with_services(cli, CommandServices::new(&hal).with_audio(&audio))
        .expect("dry run succeeds");
    assert_eq!(audio.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn auto_restore_puts_the_previous_usb_charging_state_back() {
    let hal = FakeHal::new("21VG");
    let usb = ToggleUsb::on();
    let cli = Cli::try_parse_from([
        "vantage",
        "--yes",
        "--auto-restore",
        "usb",
        "always-on",
        "off",
    ])
    .expect("parse usb command");

    let output = execute_with_services(cli, CommandServices::new(&hal).with_usb(&usb))
        .expect("usb command succeeds");

    assert!(usb.state.lock().ac_enabled, "AC charging is back on");
    assert_eq!(usb.writes.load(Ordering::SeqCst), 2);
    assert!(output.human.contains("restored USB AC charging"));
}

/// In-memory SmartSense endpoint with scripted committed-write failures.
/// Failure indexes count committed writes only (the forward sequence and
/// the restore share the same counter), and each scripted failure fires at
/// most once.
struct FakeSense {
    state: Mutex<SenseState>,
    writes: AtomicUsize,
    /// One scripted failure per write index; `bool` = whether the write
    /// takes effect in the state before the error is returned (models a
    /// real readback failure after a committed driver write).
    scripted_failures: Mutex<Vec<(usize, bool, LctrlError)>>,
}

impl FakeSense {
    fn new(state: SenseState) -> Self {
        Self {
            state: Mutex::new(state),
            writes: AtomicUsize::new(0),
            scripted_failures: Mutex::new(Vec::new()),
        }
    }

    /// Fail the `index`-th commit write (1-based) before it applies any state.
    fn fail_write(self, index: usize, error: LctrlError) -> Self {
        self.scripted_failures.lock().push((index, false, error));
        self
    }

    /// Fail the `index`-th commit write after applying its requested state,
    /// modelling a write that committed but whose independent readback
    /// rejected the result.
    fn fail_write_after_apply(self, index: usize, error: LctrlError) -> Self {
        self.scripted_failures.lock().push((index, true, error));
        self
    }
}

fn apply_sense_feature_to(state: &mut SenseState, feature: SenseFeature) {
    match feature {
        SenseFeature::LockOnLeave(enabled) => state.lock_on_leave = Some(enabled),
        SenseFeature::LeaveDistance(distance) => state.leave_distance = Some(distance),
        SenseFeature::LeaveWait(wait) => state.leave_wait = Some(wait),
        SenseFeature::WakeOnApproach(enabled) => state.wake_on_approach = Some(enabled),
        SenseFeature::ApproachDistance(distance) => state.approach_distance = Some(distance),
        SenseFeature::PauseVideo(enabled) => state.pause_video = Some(enabled),
        SenseFeature::CameraAssist(enabled) => state.camera_assist = Some(enabled),
        SenseFeature::ApproachAutoAdjust(enabled) => state.approach_auto_adjust = Some(enabled),
        SenseFeature::LeaveAutoAdjust(enabled) => state.leave_auto_adjust = Some(enabled),
        SenseFeature::PrivacyAlert(enabled) => state.privacy_alert = Some(enabled),
        SenseFeature::AttentionTracking(enabled) => state.attention_tracking = Some(enabled),
        SenseFeature::ResetAll => {}
    }
}

impl SenseControl for FakeSense {
    fn sense_state(&self) -> lctrl_core::Result<SenseState> {
        Ok(self.state.lock().clone())
    }

    fn set_sense_feature(
        &self,
        feature: SenseFeature,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<SenseState>> {
        let previous = self.state.lock().clone();
        let mut requested = previous.clone();
        apply_sense_feature_to(&mut requested, feature);
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let write_index = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        let scripted = {
            let mut failures = self.scripted_failures.lock();
            let position = failures
                .iter()
                .position(|(index, _, _)| *index == write_index);
            position.map(|position| failures.remove(position))
        };
        if let Some((_, applied, error)) = scripted {
            if applied {
                *self.state.lock() = requested.clone();
            }
            return Err(error);
        }
        *self.state.lock() = requested.clone();
        Ok(ChangeReport::committed(
            previous,
            requested.clone(),
            requested,
        ))
    }
}

#[test]
fn sense_auto_restore_rejects_an_unknown_attention_tracking_baseline_before_any_write() {
    let hal = FakeHal::new("21VG");
    // Attention tracking has no persisted key on this model, so the
    // pre-write value is unknown and an auto-restore could never put it
    // back; the command must abort without mutating anything.
    let sense = FakeSense::new(SenseState {
        source: None,
        advertised: None,
        lock_on_leave: Some(false),
        ..SenseState::default()
    });
    let cli = Cli::try_parse_from([
        "vantage",
        "--auto-restore",
        "sense",
        "attention-tracking",
        "on",
    ])
    .expect("parse attention-tracking command");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect_err("an unrecoverable feature must reject auto-restore");

    assert!(
        matches!(error, LctrlError::InvalidArgument { .. }),
        "unexpected error: {error:?}"
    );
    assert!(
        error.to_string().contains("refused before write"),
        "the rejection says no write happened: {error}"
    );
    assert_eq!(
        sense.writes.load(Ordering::SeqCst),
        0,
        "preflight rejects before any mutation"
    );
    assert_eq!(sense.state.lock().attention_tracking, None);
}

#[test]
fn sense_auto_restore_rejects_a_missing_persisted_value_before_any_write() {
    let hal = FakeHal::new("21VG");
    // The settings store has never persisted lock-on-leave (None
    // baseline), the same refusal must apply to the plain features.
    let sense = FakeSense::new(SenseState::default());
    let cli = Cli::try_parse_from(["vantage", "--auto-restore", "sense", "lock-on-leave", "on"])
        .expect("parse lock-on-leave command");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect_err("an unknown baseline must reject auto-restore");

    assert!(
        matches!(error, LctrlError::InvalidArgument { .. }),
        "unexpected error: {error:?}"
    );
    assert_eq!(
        sense.writes.load(Ordering::SeqCst),
        0,
        "preflight rejects before any mutation"
    );
}

#[test]
fn sense_auto_restore_uses_the_multi_feature_initial_baseline() {
    let hal = FakeHal::new("21VG");
    let sense = FakeSense::new(SenseState {
        source: None,
        advertised: None,
        lock_on_leave: Some(false),
        leave_distance: Some(SenseDistance::Near),
        ..SenseState::default()
    });
    let cli = Cli::try_parse_from([
        "vantage",
        "--auto-restore",
        "sense",
        "lock-on-leave",
        "on",
        "--distance",
        "2",
    ])
    .expect("parse lock-on-leave command");

    let output = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect("lock-on-leave command succeeds");

    let state = sense.state.lock().clone();
    assert_eq!(
        state.lock_on_leave,
        Some(false),
        "the initial baseline is restored, not the intermediate report.previous"
    );
    assert_eq!(state.leave_distance, Some(SenseDistance::Near));
    assert_eq!(
        sense.writes.load(Ordering::SeqCst),
        4,
        "two forward writes plus two restore writes"
    );
    assert!(output.human.contains("auto-restore"));
    assert!(output.json["auto_restore"].is_string());
}

#[test]
fn sense_auto_restore_runs_after_a_readback_failure_and_preserves_the_error() {
    let hal = FakeHal::new("21VG");
    // The second committed write applies its change but fails its
    // readback; the committed prefix and the failed feature must both be
    // returned to their initial values, and the original error is
    // reported unchanged.
    let sense = FakeSense::new(SenseState {
        source: None,
        advertised: None,
        lock_on_leave: Some(false),
        leave_distance: Some(SenseDistance::Near),
        ..SenseState::default()
    })
    .fail_write_after_apply(
        2,
        LctrlError::VerifyMismatch {
            requested: "leave distance far".into(),
            actual: "leave distance near".into(),
        },
    );
    let cli = Cli::try_parse_from([
        "vantage",
        "--auto-restore",
        "sense",
        "lock-on-leave",
        "on",
        "--distance",
        "2",
    ])
    .expect("parse lock-on-leave command");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect_err("the readback failure is reported");

    assert!(
        matches!(error, LctrlError::VerifyMismatch { .. }),
        "the primary failure is preserved, got {error:?}"
    );
    let state = sense.state.lock().clone();
    assert_eq!(
        state.lock_on_leave,
        Some(false),
        "the committed prefix is restored"
    );
    assert_eq!(
        state.leave_distance,
        Some(SenseDistance::Near),
        "the failed feature keeps its initial value"
    );
    assert_eq!(
        sense.writes.load(Ordering::SeqCst),
        4,
        "two forward attempts plus one restore write per touched feature"
    );
}

#[test]
fn sense_auto_restore_reports_a_cleanup_failure_next_to_the_primary_error() {
    let hal = FakeHal::new("21VG");
    // The forward write fails and the first restore write fails too; the
    // combined error must carry both failures.
    let sense = FakeSense::new(SenseState {
        source: None,
        advertised: None,
        lock_on_leave: Some(false),
        leave_distance: Some(SenseDistance::Near),
        ..SenseState::default()
    })
    .fail_write(
        2,
        LctrlError::FirmwareRejected {
            detail: "pipe write failed".into(),
        },
    )
    .fail_write(
        3,
        LctrlError::ChannelUnavailable {
            channel: "settings pipe closed".into(),
        },
    );
    let cli = Cli::try_parse_from([
        "vantage",
        "--auto-restore",
        "sense",
        "lock-on-leave",
        "on",
        "--distance",
        "2",
    ])
    .expect("parse lock-on-leave command");

    let error = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect_err("the combined failure is reported");

    let detail = match &error {
        LctrlError::FirmwareRejected { detail } => detail,
        other => panic!("unexpected error: {other:?}"),
    };
    assert!(
        detail.contains("pipe write failed"),
        "the primary failure is preserved: {detail}"
    );
    assert!(
        detail.contains("auto-restore also failed"),
        "the cleanup failure is included: {detail}"
    );
    assert!(
        detail.contains("settings pipe closed"),
        "the cleanup failure is included: {detail}"
    );
    assert_eq!(
        sense.state.lock().lock_on_leave,
        Some(true),
        "the restore could not run, so the forward value stays"
    );
    assert_eq!(sense.writes.load(Ordering::SeqCst), 3);
}

#[test]
fn sense_output_keeps_the_plain_report_shape_without_auto_restore() {
    let hal = FakeHal::new("21VG");
    let sense = FakeSense::new(SenseState {
        source: None,
        advertised: None,
        pause_video: Some(false),
        ..SenseState::default()
    });
    let cli = Cli::try_parse_from(["vantage", "sense", "pause-video", "on"])
        .expect("parse pause-video command");

    let output = execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect("pause-video command succeeds");

    assert!(
        output.json.get("auto_restore").is_none(),
        "no auto_restore key is added without the flag"
    );
    assert!(
        output.json.get("change").is_none(),
        "the report stays the top-level payload"
    );
    assert_eq!(output.json["mode"], "commit");
    assert_eq!(sense.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn sense_auto_restore_stays_dry_in_a_dry_run() {
    let hal = FakeHal::new("21VG");
    // Even a feature with no persisted baseline is only simulated.
    let sense = FakeSense::new(SenseState::default());
    let cli = Cli::try_parse_from([
        "vantage",
        "--dry-run",
        "--auto-restore",
        "sense",
        "attention-tracking",
        "on",
    ])
    .expect("parse dry-run command");

    execute_with_services(cli, CommandServices::new(&hal).with_sense(&sense))
        .expect("dry run succeeds");
    assert_eq!(sense.writes.load(Ordering::SeqCst), 0);
}

struct FixedBroadband(MagicBayLteStatus);

impl MobileBroadbandControl for FixedBroadband {
    fn lte_status(&self) -> lctrl_core::Result<MagicBayLteStatus> {
        Ok(self.0.clone())
    }
}

fn lte_status_fixture() -> MagicBayLteStatus {
    MagicBayLteStatus {
        modules: vec![MagicBayDevice {
            path: r"USB\VID_17EF&PID_7005\SERIAL".into(),
            bus: "usb".into(),
            vid: Some(0x17ef),
            pid: Some(0x7005),
            kind: MagicBayKind::Lte2,
            interfaces: vec!["mbim".into()],
            attached: true,
        }],
        interfaces: vec![MagicBayLteInterface {
            interface_index: 57,
            if_type: 243,
            description: "Generic Mobile Broadband Adapter".into(),
            media_state: MagicBayLteMediaState::Connected,
            operational: MagicBayLteOperational::Up,
            ipv4: vec!["10.42.7.9".into()],
            ipv6: Vec::new(),
        }],
        data_session: true,
    }
}

#[test]
fn magicbay_lte_status_reports_module_interface_and_session() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "--json", "magicbay", "lte", "status"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal).with_mobile_broadband(&FixedBroadband(lte_status_fixture())),
    )
    .expect("lte status succeeds");

    assert_eq!(output.json["data_session"], json!(true));
    assert_eq!(output.json["modules"][0]["kind"], json!("lte2"));
    assert_eq!(output.json["interfaces"][0]["interface_index"], json!(57));
    assert_eq!(output.json["interfaces"][0]["media_state"], json!("connected"));
    assert_eq!(output.json["interfaces"][0]["operational"], json!("up"));
    assert_eq!(
        output.human,
        "1 MagicBay LTE module(s), 1 WWAN interface(s), data session up\n"
    );
}

#[test]
fn magicbay_lte_status_without_a_service_is_unsupported() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "magicbay", "lte", "status"]).unwrap();
    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("no Mobile Broadband service is attached");
    assert!(
        matches!(error, LctrlError::Unsupported { ref feature } if feature.contains("magicbay.lte")),
        "unexpected error {error}"
    );
}

#[test]
fn magicbay_lte_status_keeps_the_accessory_absence_error() {
    struct Absent;

    impl MobileBroadbandControl for Absent {
        fn lte_status(&self) -> lctrl_core::Result<MagicBayLteStatus> {
            Err(LctrlError::ChannelUnavailable {
                channel: "magicbay.lte: no MagicBay LTE accessory is enumerated".into(),
            })
        }
    }

    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "magicbay", "lte", "status"]).unwrap();
    let error =
        execute_with_services(cli, CommandServices::new(&hal).with_mobile_broadband(&Absent))
            .expect_err("absent accessory is an error, never an empty success");
    assert!(
        matches!(error, LctrlError::ChannelUnavailable { ref channel } if channel.contains("no MagicBay LTE accessory")),
        "unexpected error {error}"
    );
}

#[test]
fn magicbay_modem_session_control_stays_unsupported_with_the_recovered_reason() {
    let hal = FakeHal::new("21VG");
    for args in [
        &["vantage", "magicbay", "lte", "connect"][..],
        &["vantage", "magicbay", "lte", "disconnect"][..],
        &["vantage", "magicbay", "lte", "apn", "ctnet"][..],
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        let error = execute_with_services(
            cli,
            CommandServices::new(&hal).with_mobile_broadband(&FixedBroadband(lte_status_fixture())),
        )
        .expect_err("modem session control has no bindable API");
        let text = error.to_string();
        assert!(text.contains("mbnapi.dll"), "{args:?}: {text}");
        assert!(text.contains("re-magicbay-20261105.md"), "{args:?}: {text}");
    }
}

struct FakeFirmware {
    status: lctrl_core::FirmwareStatus,
}

impl lctrl_hal::FirmwareControl for FakeFirmware {
    fn firmware_status(&self) -> lctrl_core::Result<lctrl_core::FirmwareStatus> {
        Ok(self.status.clone())
    }
}

fn firmware_status_fixture() -> lctrl_core::FirmwareStatus {
    lctrl_core::FirmwareStatus {
        identity: lctrl_core::FirmwareIdentity {
            firmware_type: lctrl_core::FirmwareType::Uefi,
            vendor: Some("LENOVO".into()),
            version: Some("SWCN22WW".into()),
            release_date: Some("07/15/2026 08:00:00".into()),
            bios_release_major: Some(1),
            bios_release_minor: Some(22),
            ec_firmware_major: Some(1),
            ec_firmware_minor: Some(22),
            smbios_major: Some(3),
            smbios_minor: Some(8),
            version_strings: vec!["LENOVO - 1".into(), "INSYDE Corp. - 71250022".into()],
        },
        resources: vec![
            lctrl_core::FirmwareResource {
                instance_id: "UEFI\\RES_{8163650E-C940-3926-0703-FCF1F743D4E2}\\0".into(),
                firmware_class: "{8163650E-C940-3926-0703-FCF1F743D4E2}".into(),
                current_version: Some(0x7125_0022),
                stored_version: Some(0x7125_0022),
                description: Some("System Firmware".into()),
                status: Some("OK".into()),
                hardware_ids: Vec::new(),
                compatible_ids: Vec::new(),
                issues: Vec::new(),
            },
            lctrl_core::FirmwareResource {
                instance_id: "UEFI\\RES_{64EC2CA6-4AE3-4A6E-B542-27A7E0AA0BA6}\\0".into(),
                firmware_class: "{64EC2CA6-4AE3-4A6E-B542-27A7E0AA0BA6}".into(),
                current_version: None,
                stored_version: None,
                description: Some("Device Firmware".into()),
                status: Some("OK".into()),
                hardware_ids: Vec::new(),
                compatible_ids: Vec::new(),
                issues: Vec::new(),
            },
        ],
        attempts: vec![
            lctrl_core::FirmwareAttempt {
                firmware_class: "{499371BF-8950-4B3C-81B6-2C97525241D7}".into(),
                version: Some(0x0D03_3710),
                last_attempt_version: Some(0x0D03_3710),
                last_attempt_status: Some(0),
                last_attempt_outcome: Some("success"),
                phase: Some(2),
                filename: Some("TB14G8+_TFforcepad_FW0D033710_260720.bin".into()),
                catalog: Some("oem57.cat".into()),
            },
            lctrl_core::FirmwareAttempt {
                firmware_class: "{01F69777-46DB-46D0-9947-83BAC67B7EF1}".into(),
                version: Some(0x1011),
                last_attempt_version: None,
                last_attempt_status: Some(0x1000),
                last_attempt_outcome: None,
                phase: None,
                filename: None,
                catalog: None,
            },
        ],
        issues: Vec::new(),
    }
}

#[test]
fn firmware_status_reports_identity_resources_and_attempts() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "firmware", "status"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal).with_firmware(&FakeFirmware {
            status: firmware_status_fixture(),
        }),
    )
    .expect("firmware status");
    assert!(output.human.contains("UEFI firmware: LENOVO SWCN22WW"), "{}", output.human);
    assert!(output.human.contains("BIOS release: 1.22"), "{}", output.human);
    assert!(output.human.contains("ESRT entries: 2"), "{}", output.human);
    assert!(
        output.human.contains("{8163650E-C940-3926-0703-FCF1F743D4E2}"),
        "{}",
        output.human
    );
    assert!(
        output.human.contains("1898250274 (0x71250022)"),
        "{}",
        output.human
    );
    // An entry with no readable revision keeps a visible absence.
    assert!(output.human.contains("version absent"), "{}", output.human);
    assert!(output.human.contains("status 0 (success)"), "{}", output.human);
    assert!(output.human.contains("catalog: oem57.cat"), "{}", output.human);

    assert_eq!(output.json["resources"][0]["current_version"], json!(0x7125_0022));
    assert!(output.json["resources"][1]["current_version"].is_null());
    assert_eq!(output.json["attempts"][0]["last_attempt_outcome"], json!("success"));
}

#[test]
fn firmware_status_prints_an_unknown_attempt_status_as_its_raw_number() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "firmware", "status"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal).with_firmware(&FakeFirmware {
            status: firmware_status_fixture(),
        }),
    )
    .expect("firmware status");
    assert!(output.human.contains("status 4096 (unnamed)"), "{}", output.human);
    assert!(!output.human.contains("status 4096 (success)"));
    assert!(output.json["attempts"][1]["last_attempt_outcome"].is_null());
    assert_eq!(output.json["attempts"][1]["last_attempt_status"], json!(0x1000));
}

#[test]
fn firmware_status_without_a_channel_is_unsupported() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "firmware", "status"]).unwrap();
    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("no firmware inventory service is attached");
    assert!(
        matches!(
            error,
            LctrlError::Unsupported { ref feature } if feature.starts_with("firmware.status")
        ),
        "unexpected error {error}"
    );
}

struct FakeUsbPorts {
    inventory: lctrl_core::UsbPortInventory,
}

impl lctrl_hal::UsbPortInventoryControl for FakeUsbPorts {
    fn usb_port_inventory(&self) -> lctrl_core::Result<lctrl_core::UsbPortInventory> {
        Ok(self.inventory.clone())
    }
}

#[test]
fn usb_ports_prints_the_port_tree_and_keeps_unknown_speeds_raw() {
    let hal = FakeHal::new("21VG");
    let inventory = lctrl_core::UsbPortInventory {
        controllers: vec![lctrl_core::UsbController {
            instance_id: "PCI\\VEN_8086&DEV_E47D&REV_01".into(),
            description: "Intel(R) USB 3.20 eXtensible Host Controller".into(),
            driver: Some("USBXHCI".into()),
            kind: lctrl_core::UsbControllerKind::Xhci,
            hubs: vec![lctrl_core::UsbHub {
                instance_id: "USB\\ROOT_HUB30\\4&196F3462&0&0".into(),
                port_count: 12,
                bus_powered: false,
                ports: vec![
                    lctrl_core::UsbPort {
                        number: 4,
                        connection: lctrl_core::UsbPortConnection::Connected,
                        speed: Some(lctrl_core::UsbPortSpeed::Unknown(9)),
                        device: Some(lctrl_core::UsbPortDevice {
                            instance_id: Some("USB\\VID_27C6&PID_659A\\UID".into()),
                            description: Some("Goodix USB2.0 MISC".into()),
                            vendor_id: 0x27c6,
                            product_id: 0x659a,
                            usb_revision: "2.00".into(),
                            is_hub: false,
                            max_power_ma: Some(100),
                        }),
                        power_delivery: None,
                    },
                    lctrl_core::UsbPort {
                        number: 6,
                        connection: lctrl_core::UsbPortConnection::Empty,
                        speed: None,
                        device: None,
                        power_delivery: None,
                    },
                ],
            }],
            ports_unavailable: None,
        }],
        power_delivery: lctrl_core::PowerDeliveryReport {
            pd_controller: Some(lctrl_core::UsbPdController {
                instance_id: "ACPI\\USBC000\\0".into(),
                driver: Some("UcmUcsiAcpiClient".into()),
                firmware_version: Some("1.2".into()),
            }),
            usb4_host_routers: vec!["PCI\\VEN_8086&DEV_E434".into()],
            per_port_unavailable: Some("no user-mode connector interface".into()),
        },
    };
    let cli = Cli::try_parse_from(["vantage", "usb", "ports"]).unwrap();
    let output = execute_with_services(
        cli,
        CommandServices::new(&hal).with_usb_ports(&FakeUsbPorts { inventory }),
    )
    .expect("usb ports");
    assert!(
        output
            .human
            .contains("port 4: connected, speed unknown (raw 9)"),
        "{}",
        output.human
    );
    assert!(output.human.contains("declares 100 mA"), "{}", output.human);
    assert!(output.human.contains("    port 6: empty"), "{}", output.human);
    assert!(output.human.contains("firmware 1.2"), "{}", output.human);
    assert!(
        output.human.contains("per-port power: unavailable"),
        "{}",
        output.human
    );
    assert_eq!(
        output.json["controllers"][0]["hubs"][0]["port_count"],
        json!(12)
    );
    assert_eq!(
        output.json["controllers"][0]["hubs"][0]["ports"][0]["speed"]["unknown"],
        json!(9)
    );
    assert!(
        output.json["controllers"][0]["hubs"][0]["ports"][0]["power_delivery"].is_null(),
        "a port with no reported PD contract must serialise as null"
    );
    assert_eq!(
        output.json["power_delivery"]["usb4_host_routers"][0],
        json!("PCI\\VEN_8086&DEV_E434")
    );
}

#[test]
fn usb_ports_without_a_service_is_structured_unsupported() {
    let hal = FakeHal::new("21VG");
    let cli = Cli::try_parse_from(["vantage", "usb", "ports"]).unwrap();
    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("no USB port inventory service is attached");
    assert!(
        matches!(error, LctrlError::Unsupported { ref feature } if feature == "usb.ports"),
        "unexpected error {error}"
    );
}
