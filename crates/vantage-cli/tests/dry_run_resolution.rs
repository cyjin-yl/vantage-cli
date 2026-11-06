//! Regression guard for the `--dry-run` shadowing defect.
//!
//! `--dry-run` is declared exactly once, as the root `global = true` flag, and
//! every mutating handler consumes the single `ApplyMode` that
//! `execute_with_services` resolves from it. These tests pin all three legs of
//! that contract:
//!
//! 1. the *root* spelling (`vantage --dry-run <leaf>`) reaches each handler,
//! 2. the *trailing* spelling (`vantage <leaf> --dry-run`), which is what the
//!    removed per-subcommand fields used to serve, still reaches each handler,
//! 3. no subcommand declares an argument id that the root already declares
//!    `global` — the collision that made clap skip the global declaration and
//!    left five handlers dependent on an undocumented value-precedence rule.
//!
//! The services are fakes that record the `ApplyMode` they are handed, so a
//! `Commit`/`DryRun` mix-up fails here without touching hardware.

use std::sync::Mutex;

use clap::CommandFactory;
use lctrl_core::{
    ApplyMode, CapabilitySet, DriverIdentity, DriverRollbackReport, DriverSignature,
    DriverUpdateReport, DriverVersion, FixLedger, FixObservation, FixRepairKind, FixReport,
    FixRule, HardwareInfo, InstalledDriver, PerformanceMode, Platform, Result, SceneAction,
    ScenePolicyLedger, ScenePolicyMatch, ScenePolicyReport, ScenePolicyRule,
};
use lctrl_hal::{Hal, LocalUpdateControl, MachineFixControl, ScenePolicyControl};
use vantage_cli::{
    Cli, Command, CommandServices, ProfileCommand, TuneCommand, execute_with_services,
};

const HARDWARE_ID: &str = "PCI\\VEN_FFFF&DEV_0001";
const DEVICE_INSTANCE_ID: &str = "PCI\\VEN_FFFF&DEV_0001\\1";
const CLASS_GUID: &str = "{4d36e968-e325-11ce-bfc1-08002be10318}";
const SCENE_RULE: &str = "scene.test.rule";

struct FakeHal;

impl Hal for FakeHal {
    fn platform(&self) -> Platform {
        Platform::Windows
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: Some("21VG".into()),
            family: Some("21VG".into()),
            bios_version: Some("1.07".into()),
        })
    }

    fn capabilities(&self) -> Result<CapabilitySet> {
        Ok(CapabilitySet::new(Platform::Windows))
    }
}

/// What one execution observed: the parsed root flag, the mode the service was
/// handed, and the command's own JSON payload.
struct Observation {
    parsed_dry_run: bool,
    mode: ApplyMode,
    payload: serde_json::Value,
}

fn only_mode(modes: &Mutex<Vec<ApplyMode>>) -> ApplyMode {
    let recorded = modes.lock().expect("mode log").clone();
    assert_eq!(recorded.len(), 1, "the handler must call the service once");
    recorded[0]
}

#[derive(Default)]
struct FixRecorder {
    modes: Mutex<Vec<ApplyMode>>,
}

impl MachineFixControl for FixRecorder {
    fn rules(&self) -> Vec<FixRule> {
        Vec::new()
    }

    fn scan(&self) -> Result<Vec<FixObservation>> {
        Ok(Vec::new())
    }

    fn fix(&self, issue_id: &str, mode: ApplyMode) -> Result<FixReport> {
        self.modes.lock().expect("mode log").push(mode);
        Ok(FixReport {
            issue_id: issue_id.to_owned(),
            mode,
            previous: "service absent".into(),
            requested: FixRepairKind::StartService {
                name: "InertProbeService".into(),
            },
            actual: None,
            rollback: FixRepairKind::StopService {
                name: "InertProbeService".into(),
            },
        })
    }

    fn track(&self) -> FixLedger {
        FixLedger::default()
    }
}

#[derive(Default)]
struct SceneRecorder {
    modes: Mutex<Vec<ApplyMode>>,
}

impl ScenePolicyControl for SceneRecorder {
    fn rules(&self) -> Vec<ScenePolicyRule> {
        Vec::new()
    }

    fn match_event(&self, _event_class: &str) -> Vec<ScenePolicyMatch> {
        Vec::new()
    }

    fn apply(
        &self,
        rule_id: &str,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        self.modes.lock().expect("mode log").push(mode);
        Ok(ScenePolicyReport {
            rule_id: rule_id.to_owned(),
            event_class: event_class.to_owned(),
            mode,
            previous: "balanced".into(),
            requested: SceneAction::SetPerformanceMode {
                mode: PerformanceMode::Performance,
            },
            actual: None,
            rollback: Some(SceneAction::SetPerformanceMode {
                mode: PerformanceMode::Balanced,
            }),
        })
    }

    fn track(&self) -> ScenePolicyLedger {
        ScenePolicyLedger::default()
    }
}

#[derive(Default)]
struct DriverRecorder {
    modes: Mutex<Vec<ApplyMode>>,
}

fn probe_identity() -> DriverIdentity {
    DriverIdentity {
        provider: "Contoso".into(),
        description: "Inert Probe Device".into(),
        class_guid: CLASS_GUID.into(),
        driver_version: DriverVersion::parse("1.0.0.0").expect("four-part driver version"),
        driver_date: "2020-01-01".into(),
        matching_device_id: HARDWARE_ID.into(),
        inf_section: "Probe.NT".into(),
    }
}

fn installed_probe() -> InstalledDriver {
    InstalledDriver {
        identity: probe_identity(),
        published_inf: "oem42.inf".into(),
        driver_key: DEVICE_INSTANCE_ID.into(),
    }
}

impl LocalUpdateControl for DriverRecorder {
    fn apply_local_driver_update(
        &self,
        _request: &lctrl_core::LocalDriverUpdateRequest,
        mode: ApplyMode,
    ) -> Result<DriverUpdateReport> {
        self.modes.lock().expect("mode log").push(mode);
        Ok(DriverUpdateReport {
            mode,
            device_instance_id: DEVICE_INSTANCE_ID.into(),
            previous: None,
            requested: probe_identity(),
            actual: None,
            signature: DriverSignature {
                signer: "Contoso".into(),
                signer_version: "1.0".into(),
            },
            reboot_required: None,
        })
    }

    fn rollback_local_driver(
        &self,
        _request: &lctrl_core::LocalDriverRollbackRequest,
        mode: ApplyMode,
    ) -> Result<DriverRollbackReport> {
        self.modes.lock().expect("mode log").push(mode);
        Ok(DriverRollbackReport {
            mode,
            device_instance_id: DEVICE_INSTANCE_ID.into(),
            previous: installed_probe(),
            requested: probe_identity(),
            actual: None,
            backup_identity_verified: false,
            reboot_required: None,
        })
    }
}

/// An absolute `.inf` path with the right extension. `LocalDriverUpdateRequest`
/// only admits the *shape* of the path; it is never opened by these fakes.
fn inert_inf() -> String {
    let mut path = std::env::temp_dir();
    path.push("vantage-inert-dry-run.inf");
    path.to_string_lossy().into_owned()
}

fn execute_fix(argv: &[&str]) -> Observation {
    let cli = Cli::try_parse_from(argv.iter().copied()).expect("parse fix run");
    let parsed_dry_run = cli.dry_run;
    let hal = FakeHal;
    let machine_fix = FixRecorder::default();
    let services = CommandServices::new(&hal).with_machine_fix(&machine_fix);
    let output = execute_with_services(cli, services).expect("fix run executes");
    Observation {
        parsed_dry_run,
        mode: only_mode(&machine_fix.modes),
        payload: output.json,
    }
}

fn execute_scene(argv: &[&str]) -> Observation {
    let cli = Cli::try_parse_from(argv.iter().copied()).expect("parse scene test");
    let parsed_dry_run = cli.dry_run;
    let hal = FakeHal;
    let scene = SceneRecorder::default();
    let services = CommandServices::new(&hal).with_scene(&scene);
    let output = execute_with_services(cli, services).expect("scene test executes");
    Observation {
        parsed_dry_run,
        mode: only_mode(&scene.modes),
        payload: output.json,
    }
}

fn execute_driver(argv: &[&str]) -> Observation {
    let cli = Cli::try_parse_from(argv.iter().copied()).expect("parse update local-driver");
    let parsed_dry_run = cli.dry_run;
    let hal = FakeHal;
    let local_update = DriverRecorder::default();
    let services = CommandServices::new(&hal).with_local_update(&local_update);
    let output = execute_with_services(cli, services).expect("local-driver executes");
    Observation {
        parsed_dry_run,
        mode: only_mode(&local_update.modes),
        payload: output.json,
    }
}

fn fix_apply_argv(dry_run_at_root: bool) -> Vec<&'static str> {
    if dry_run_at_root {
        vec!["vantage", "--dry-run", "fix", "run", "fix.probe.issue"]
    } else {
        vec!["vantage", "fix", "run", "fix.probe.issue"]
    }
}

fn fix_trailing_argv() -> Vec<&'static str> {
    vec!["vantage", "fix", "run", "fix.probe.issue", "--dry-run"]
}

fn scene_argv(dry_run_at_root: bool) -> Vec<&'static str> {
    if dry_run_at_root {
        vec![
            "vantage",
            "--dry-run",
            "scene",
            "test",
            "lenovo.scene.event",
            "--rule",
            SCENE_RULE,
        ]
    } else {
        vec![
            "vantage",
            "scene",
            "test",
            "lenovo.scene.event",
            "--rule",
            SCENE_RULE,
        ]
    }
}

fn scene_trailing_argv() -> Vec<&'static str> {
    vec![
        "vantage",
        "scene",
        "test",
        "lenovo.scene.event",
        "--rule",
        SCENE_RULE,
        "--dry-run",
    ]
}

fn driver_apply_argv(dry_run_at_root: bool) -> Vec<String> {
    let inf = inert_inf();
    let mut argv: Vec<String> = Vec::new();
    argv.push("vantage".into());
    if dry_run_at_root {
        argv.push("--dry-run".into());
    }
    for part in ["update", "local-driver", "apply"] {
        argv.push(part.into());
    }
    argv.push(inf);
    argv.push(HARDWARE_ID.into());
    argv
}

fn driver_apply_trailing_argv() -> Vec<String> {
    let mut argv = driver_apply_argv(false);
    argv.push("--dry-run".into());
    argv
}

fn driver_rollback_argv(dry_run_at_root: bool) -> Vec<&'static str> {
    let mut argv = vec!["vantage"];
    if dry_run_at_root {
        argv.push("--dry-run");
    }
    argv.extend_from_slice(&[
        "update",
        "local-driver",
        "rollback",
        DEVICE_INSTANCE_ID,
        "--class-guid",
        CLASS_GUID,
        "--matching-device-id",
        HARDWARE_ID,
        "--expected-provider",
        "Contoso",
        "--expected-description",
        "Inert Probe Device",
        "--expected-version",
        "1.0.0.0",
        "--expected-date",
        "2020-01-01",
        "--expected-section",
        "Probe.NT",
        "--restore-provider",
        "Contoso",
        "--restore-description",
        "Inert Probe Device",
        "--restore-version",
        "1.0.0.0",
        "--restore-date",
        "2020-01-01",
        "--restore-section",
        "Probe.NT",
    ]);
    argv
}

fn driver_rollback_trailing_argv() -> Vec<&'static str> {
    let mut argv = driver_rollback_argv(false);
    argv.push("--dry-run");
    argv
}

fn assert_dry_run(observation: &Observation, context: &str) {
    assert!(
        observation.parsed_dry_run,
        "{context}: the parsed Cli must carry the dry-run request"
    );
    assert_eq!(
        observation.mode,
        ApplyMode::DryRun,
        "{context}: the handler must hand ApplyMode::DryRun to the service"
    );
    assert_eq!(
        observation.payload["mode"], "dry_run",
        "{context}: the report must say dry_run"
    );
}

fn assert_commit(observation: &Observation, context: &str) {
    assert!(
        !observation.parsed_dry_run,
        "{context}: no dry-run flag was given"
    );
    assert_eq!(
        observation.mode,
        ApplyMode::Commit,
        "{context}: a command without --dry-run must still commit"
    );
    assert_eq!(
        observation.payload["mode"], "commit",
        "{context}: the report must say commit"
    );
}

#[test]
fn root_dry_run_reaches_the_fix_run_handler() {
    let observation = execute_fix(&fix_apply_argv(true));
    assert_dry_run(&observation, "vantage --dry-run fix run");
}

#[test]
fn root_dry_run_reaches_the_scene_test_handler() {
    let observation = execute_scene(&scene_argv(true));
    assert_dry_run(&observation, "vantage --dry-run scene test");
}

#[test]
fn root_dry_run_reaches_local_driver_apply() {
    let argv = driver_apply_argv(true);
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();
    let observation = execute_driver(&borrowed);
    assert_dry_run(&observation, "vantage --dry-run update local-driver apply");
}

#[test]
fn root_dry_run_reaches_local_driver_rollback() {
    let observation = execute_driver(&driver_rollback_argv(true));
    assert_dry_run(&observation, "vantage --dry-run update local-driver rollback");
}

#[test]
fn trailing_and_middle_dry_run_still_reach_every_shadowed_handler() {
    // The trailing spelling is the one the deleted per-subcommand fields used to
    // serve; the middle spelling pins the deeper up-propagation path. Both now
    // resolve through the single root global, so both must behave identically.
    assert_dry_run(
        &execute_fix(&fix_trailing_argv()),
        "vantage fix run --dry-run",
    );
    assert_dry_run(
        &execute_scene(&scene_trailing_argv()),
        "vantage scene test --dry-run",
    );
    let argv = driver_apply_trailing_argv();
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();
    assert_dry_run(
        &execute_driver(&borrowed),
        "vantage update local-driver apply --dry-run",
    );
    assert_dry_run(
        &execute_driver(&driver_rollback_trailing_argv()),
        "vantage update local-driver rollback --dry-run",
    );

    // Middle positions: the flag sits on an intermediate group command.
    assert_dry_run(
        &execute_fix(&["vantage", "fix", "--dry-run", "run", "fix.probe.issue"]),
        "vantage fix --dry-run run",
    );
    assert_dry_run(
        &execute_scene(&[
            "vantage",
            "scene",
            "--dry-run",
            "test",
            "lenovo.scene.event",
            "--rule",
            SCENE_RULE,
        ]),
        "vantage scene --dry-run test",
    );

    let mut middle = driver_apply_argv(false);
    middle.insert(2, "--dry-run".into());
    let borrowed = middle.iter().map(String::as_str).collect::<Vec<_>>();
    assert_dry_run(
        &execute_driver(&borrowed),
        "vantage update --dry-run local-driver apply",
    );

    let mut deep = driver_apply_argv(false);
    deep.insert(3, "--dry-run".into());
    let borrowed = deep.iter().map(String::as_str).collect::<Vec<_>>();
    assert_dry_run(
        &execute_driver(&borrowed),
        "vantage update local-driver --dry-run apply",
    );

    let mut rollback_middle = driver_rollback_argv(false);
    rollback_middle.insert(2, "--dry-run");
    assert_dry_run(
        &execute_driver(&rollback_middle),
        "vantage update --dry-run local-driver rollback",
    );
}

#[test]
fn commit_without_either_spelling_still_commits() {
    // Negative control: the recorder observes a real resolution, not a constant.
    assert_commit(&execute_fix(&fix_apply_argv(false)), "fix run");
    assert_commit(&execute_scene(&scene_argv(false)), "scene test");
    let argv = driver_apply_argv(false);
    let borrowed = argv.iter().map(String::as_str).collect::<Vec<_>>();
    assert_commit(&execute_driver(&borrowed), "local-driver apply");
    assert_commit(
        &execute_driver(&driver_rollback_argv(false)),
        "local-driver rollback",
    );
}

#[test]
fn tune_profile_apply_reads_the_single_global_flag() {
    let cli = Cli::try_parse_from(["vantage", "--dry-run", "tune", "profile", "apply", "balanced"])
        .expect("parse tune profile apply");
    assert!(cli.dry_run);
    assert!(matches!(
        cli.command,
        Command::Tune {
            command: Some(TuneCommand::Profile {
                command: Some(ProfileCommand::Apply { .. }),
            }),
        }
    ));

    let trailing =
        Cli::try_parse_from(["vantage", "tune", "profile", "apply", "balanced", "--dry-run"])
            .expect("parse tune profile apply with the trailing flag");
    assert!(
        trailing.dry_run,
        "the trailing spelling must still reach the global flag"
    );
}

fn collect_global_id_collisions(
    command: &clap::Command,
    globals: &[clap::Id],
    path: &str,
    collisions: &mut Vec<String>,
) {
    for arg in command.get_arguments() {
        if globals.iter().any(|global| global == arg.get_id()) {
            collisions.push(format!("{path}: {}", arg.get_id().as_str()));
        }
    }
    for subcommand in command.get_subcommands() {
        collect_global_id_collisions(
            subcommand,
            globals,
            &format!("{path} {}", subcommand.get_name()),
            collisions,
        );
    }
}

#[test]
fn no_subcommand_redeclares_a_global_id() {
    // This is the invariant that removes the whole defect class: a subcommand
    // that declares an id the root already declares `global` makes clap skip
    // propagating that global into the subcommand (`Command::_propagate_global_args`),
    // which is how five handlers ended up with a second, drift-prone source of
    // truth for the dry-run decision.
    let root = Cli::command();
    let globals = root
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .map(|arg| arg.get_id().clone())
        .collect::<Vec<_>>();

    assert_eq!(
        globals.len(),
        4,
        "the root declares exactly --json, --dry-run, --yes and --auto-restore as globals: {globals:?}"
    );
    assert!(
        globals.iter().any(|id| id.as_str() == "dry_run"),
        "--dry-run must be a root global"
    );

    let mut collisions = Vec::new();
    for subcommand in root.get_subcommands() {
        collect_global_id_collisions(
            subcommand,
            &globals,
            &format!("vantage {}", subcommand.get_name()),
            &mut collisions,
        );
    }
    assert!(
        collisions.is_empty(),
        "no subcommand may re-declare a global argument id: {collisions:?}"
    );
}
