//! Command-line surface for `vantage`.
//!
//! The command tree deliberately lives separately from host backends. Concrete
//! platform composition roots opt into only the hardware services with safe,
//! verified implementations; missing services return structured unsupported
//! errors rather than synthetic success.

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand, ValueEnum};
use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, BacklightState, BatteryTelemetry, BiosChange, Capability,
    CapabilitySet, ChargeMode, ChargeModeActual, DeviceState, DiagnosticKind, FanMode,
    HardwareInfo, LctrlError, PerformanceMode, Platform, PowerGuid, PowerMutation, PowerSchemeId,
    PowerSettingKey, PowerSettingValue, PowerSource, UpdateCapability,
};
use lctrl_hal::{
    BatteryControl, BiosControl, ControlConflictDetection, DiagnosticsControl, FanControl, Hal,
    KeyboardControl, MagicBayControl, PanelControl, PerformanceControl, PowerControl,
    PowerLimitKind, PrivacyControl, TemperatureControl, TouchpadControl, TuningControl,
    UpdateControl,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Result returned by [`execute`].
pub type CommandResult = lctrl_core::Result<CommandOutput>;

/// The explicit set of hardware services available to one CLI invocation.
///
/// A service is absent when the selected platform has no safe implementation
/// for it. Dispatch then returns `Unsupported`; it never synthesizes success.
pub struct CommandServices<'a> {
    hal: &'a dyn Hal,
    battery: Option<&'a dyn BatteryControl>,
    bios: Option<&'a dyn BiosControl>,
    conflicts: Option<&'a dyn ControlConflictDetection>,
    diagnostics: Option<&'a dyn DiagnosticsControl>,
    fan: Option<&'a dyn FanControl>,
    keyboard: Option<&'a dyn KeyboardControl>,
    magicbay: Option<&'a dyn MagicBayControl>,
    performance: Option<&'a dyn PerformanceControl>,
    power: Option<&'a dyn PowerControl>,
    privacy: Option<&'a dyn PrivacyControl>,
    touchpad: Option<&'a dyn TouchpadControl>,
    panel: Option<&'a dyn PanelControl>,
    temperature: Option<&'a dyn TemperatureControl>,
    tuning: Option<&'a dyn TuningControl>,
    update: Option<&'a dyn UpdateControl>,
}

impl<'a> CommandServices<'a> {
    pub const fn new(hal: &'a dyn Hal) -> Self {
        Self {
            hal,
            battery: None,
            bios: None,
            conflicts: None,
            diagnostics: None,
            fan: None,
            keyboard: None,
            magicbay: None,
            performance: None,
            power: None,
            privacy: None,
            touchpad: None,
            panel: None,
            temperature: None,
            tuning: None,
            update: None,
        }
    }

    #[must_use]
    pub fn with_battery(mut self, battery: &'a dyn BatteryControl) -> Self {
        self.battery = Some(battery);
        self
    }

    #[must_use]
    pub fn with_bios(mut self, bios: &'a dyn BiosControl) -> Self {
        self.bios = Some(bios);
        self
    }

    #[must_use]
    pub fn with_conflict_detection(mut self, conflicts: &'a dyn ControlConflictDetection) -> Self {
        self.conflicts = Some(conflicts);
        self
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: &'a dyn DiagnosticsControl) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    #[must_use]
    pub fn with_fan(mut self, fan: &'a dyn FanControl) -> Self {
        self.fan = Some(fan);
        self
    }

    #[must_use]
    pub fn with_keyboard(mut self, keyboard: &'a dyn KeyboardControl) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    #[must_use]
    pub fn with_magicbay(mut self, magicbay: &'a dyn MagicBayControl) -> Self {
        self.magicbay = Some(magicbay);
        self
    }

    #[must_use]
    pub fn with_performance(mut self, performance: &'a dyn PerformanceControl) -> Self {
        self.performance = Some(performance);
        self
    }

    #[must_use]
    pub fn with_power(mut self, power: &'a dyn PowerControl) -> Self {
        self.power = Some(power);
        self
    }

    #[must_use]
    pub fn with_privacy(mut self, privacy: &'a dyn PrivacyControl) -> Self {
        self.privacy = Some(privacy);
        self
    }

    #[must_use]
    pub fn with_touchpad(mut self, touchpad: &'a dyn TouchpadControl) -> Self {
        self.touchpad = Some(touchpad);
        self
    }

    #[must_use]
    pub fn with_panel(mut self, panel: &'a dyn PanelControl) -> Self {
        self.panel = Some(panel);
        self
    }

    #[must_use]
    pub fn with_temperature(mut self, temperature: &'a dyn TemperatureControl) -> Self {
        self.temperature = Some(temperature);
        self
    }
    #[must_use]
    pub fn with_tuning(mut self, tuning: &'a dyn TuningControl) -> Self {
        self.tuning = Some(tuning);
        self
    }
    #[must_use]
    pub fn with_update(mut self, update: &'a dyn UpdateControl) -> Self {
        self.update = Some(update);
        self
    }
}

/// The two renderings prepared for a successfully executed command.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandOutput {
    /// Human-readable output suitable for a terminal.
    pub human: String,
    /// Machine-readable output for `--json`.
    pub json: Value,
}

impl CommandOutput {
    /// Render this successful result in the requested format.
    #[must_use]
    pub fn render(&self, json: bool) -> String {
        if json {
            // A serde_json::Value cannot contain values that fail serialization.
            serde_json::to_string(&self.json).expect("JSON values are serializable")
        } else {
            self.human.clone()
        }
    }
}

/// Render a successful command result.
#[must_use]
pub fn render_success(output: &CommandOutput, json: bool) -> String {
    output.render(json)
}

/// Render an error.  JSON mode is intentionally exactly the core error report
/// (with no CLI-specific wrapper or additional fields).
#[must_use]
pub fn render_error(error: &LctrlError, json: bool) -> String {
    if json {
        serde_json::to_string(&error.report()).expect("error reports are serializable")
    } else {
        error.to_string()
    }
}

/// Render either branch of an execution result.
#[must_use]
pub fn render_result(result: &CommandResult, json: bool) -> String {
    match result {
        Ok(output) => render_success(output, json),
        Err(error) => render_error(error, json),
    }
}

/// Root parser for the `vantage` command.
#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "vantage", version, about = "Cross-platform hardware control")]
pub struct Cli {
    /// Emit machine-readable JSON instead of terminal-oriented text.
    #[arg(long, global = true)]
    pub json: bool,
    /// Validate a mutating command and report the intended change without writing hardware.
    #[arg(long, global = true)]
    pub dry_run: bool,
    /// Confirm a risky operation after reviewing its impact and recovery path.
    #[arg(long, global = true)]
    pub yes: bool,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Parse arguments without terminating the process on an error.
    ///
    /// This inherent forwarding method keeps parsing convenient for library
    /// callers and tests that do not otherwise need to import clap's trait.
    pub fn try_parse_from<I, T>(itr: I) -> std::result::Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as clap::Parser>::try_parse_from(itr)
    }
}

/// Top-level `vantage` commands.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Report platform, hardware identity, and discovered capabilities.
    Info,
    /// Probe read-only platform channels and report actionable readiness failures.
    Doctor,
    Battery {
        #[command(subcommand)]
        command: Option<BatteryCommand>,
    },
    Usb {
        #[command(subcommand)]
        command: Option<UsbCommand>,
    },
    Power {
        #[command(subcommand)]
        command: Option<PowerCommand>,
    },
    Perf {
        #[command(subcommand)]
        command: Option<PerfCommand>,
    },
    Tune {
        #[command(subcommand)]
        command: Option<TuneCommand>,
    },
    Kbd {
        #[command(subcommand)]
        command: Option<KbdCommand>,
    },
    Touchpad {
        #[arg(value_enum)]
        state: Toggle,
    },
    Panel {
        #[command(subcommand)]
        command: Option<PanelCommand>,
    },
    Privacy {
        #[command(subcommand)]
        command: Option<PrivacyCommand>,
    },
    Sense {
        #[command(subcommand)]
        command: Option<SenseCommand>,
    },
    Audio {
        #[command(subcommand)]
        command: Option<AudioCommand>,
    },
    Bios {
        #[command(subcommand)]
        command: Option<BiosCommand>,
    },
    Magicbay {
        #[command(subcommand)]
        command: Option<MagicbayCommand>,
    },
    Osd {
        #[command(subcommand)]
        command: Option<OsdCommand>,
    },
    Update {
        #[command(subcommand)]
        command: Option<UpdateCommand>,
    },
    Scan {
        #[command(subcommand)]
        command: Option<ScanCommand>,
    },
    Snapshot {
        #[command(subcommand)]
        command: Option<SnapshotCommand>,
    },
    Daemon {
        #[command(subcommand)]
        command: Option<DaemonCommand>,
    },
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Battery command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum BatteryCommand {
    Status,
    Adapter,
    ChargeMode {
        #[arg(value_enum)]
        mode: ChargeModeArg,
    },
    Thresholds {
        start: String,
        stop: String,
    },
    ExtremeLife {
        #[arg(value_enum)]
        state: Toggle,
    },
    NightCharge {
        #[arg(value_enum)]
        state: Toggle,
    },
    TemporaryMode,
    Watch,
}

/// USB command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum UsbCommand {
    AlwaysOn {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        persistent: bool,
    },
    ChargeOnBattery {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        persistent: bool,
    },
}

/// Power command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PowerCommand {
    Scheme {
        #[command(subcommand)]
        command: Option<SchemeCommand>,
    },
    SaverOnce,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SchemeCommand {
    List,
    Get {
        name: String,
    },
    Apply {
        name: String,
    },
    Set {
        subgroup: String,
        setting: String,
        #[arg(value_enum)]
        source: PowerSourceArg,
        value: u32,
    },
}

/// Performance and thermal command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PerfCommand {
    Mode {
        #[arg(value_enum)]
        mode: PerfModeArg,
    },
    Fan {
        #[command(subcommand)]
        command: Option<FanCommand>,
    },
    Temp {
        #[command(subcommand)]
        command: Option<TempCommand>,
    },
    Pl1 {
        watts: u32,
    },
    Pl2 {
        watts: u32,
    },
    Top,
    Boost {
        pids: Vec<u32>,
    },
    Throttle {
        pids: Vec<u32>,
    },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum FanCommand {
    Status,
    Auto,
    Manual,
    Fullspeed,
    Smart,
    Curve { source: String },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum TempCommand {
    Read { id: String },
    Watch,
}

/// Tuning command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum TuneCommand {
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommand>,
    },
    Pl1 {
        watts: u32,
        #[arg(long)]
        pl2: Option<u32>,
        #[arg(long)]
        tau: Option<u32>,
    },
    Epp {
        value: String,
    },
    Turbo {
        #[arg(value_enum)]
        state: Toggle,
    },
    Restore,
    Telemetry {
        #[arg(long)]
        interval: Option<u64>,
    },
    Watch,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum ProfileCommand {
    List,
    Show {
        name: String,
    },
    Apply {
        name: String,
        #[arg(long)]
        dry_run: bool,
    },
}

/// Keyboard command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum KbdCommand {
    Backlight {
        level: u8,
        #[arg(long, value_enum)]
        effect: Option<BacklightEffect>,
    },
    Fnlock {
        #[arg(value_enum)]
        state: Toggle,
    },
    FnCtrlSwap {
        #[arg(value_enum)]
        state: Toggle,
    },
    Winlock {
        #[arg(value_enum)]
        state: Toggle,
    },
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum BacklightEffect {
    Static,
    Breath,
}

/// Panel command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PanelCommand {
    Rate {
        rate: PanelRate,
    },
    Color {
        gamut: PanelGamut,
    },
    SuperResolution {
        #[arg(value_enum)]
        state: Toggle,
    },
    Overdrive {
        #[arg(value_enum)]
        state: Toggle,
    },
    EyeCare {
        level: EyeCareLevel,
    },
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum PanelRate {
    #[value(name = "60")]
    Fps60,
    #[value(name = "120")]
    Fps120,
    Auto,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum PanelGamut {
    Srgb,
    DciP3,
    Adobe,
    Custom,
    Movie,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum EyeCareLevel {
    Off,
    Mid,
    High,
}

/// Privacy command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PrivacyCommand {
    Cam {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long, conflicts_with = "persistent")]
        runtime: bool,
        #[arg(long, conflicts_with = "runtime")]
        persistent: bool,
    },
    Mic {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long, conflicts_with = "persistent")]
        runtime: bool,
        #[arg(long, conflicts_with = "runtime")]
        persistent: bool,
    },
    Fingerprint {
        #[arg(value_enum)]
        state: Toggle,
    },
    Status,
}

/// Smart-sensing command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SenseCommand {
    Status,
    LockOnLeave {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        distance: Option<u32>,
        #[arg(long)]
        wait: Option<u32>,
    },
    WakeOnApproach {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        distance: Option<u32>,
    },
    PauseVideo {
        #[arg(value_enum)]
        state: Toggle,
    },
    AttentionTracking {
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        dim: bool,
        #[arg(long)]
        ac_only: bool,
    },
    KbdLightAuto {
        #[arg(value_enum)]
        state: Toggle,
    },
}

/// Audio command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum AudioCommand {
    Dolby { profile: DolbyProfileArg },
    NoiseCancel { mode: NoiseCancelArg },
}

/// BIOS command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum BiosCommand {
    List,
    Get {
        name: String,
    },
    Set {
        name: String,
        value: String,
        #[arg(long)]
        save: bool,
    },
    Save,
    Discard,
    Defaults,
    Password {
        #[command(subcommand)]
        command: Option<PasswordCommand>,
    },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PasswordCommand {
    Status,
    Set { value: Option<String> },
    Clear,
    Verify { value: Option<String> },
}

/// Magic Bay command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum MagicbayCommand {
    Detect,
    Lte {
        #[command(subcommand)]
        command: Option<LteCommand>,
    },
    Cam,
    Display,
    Watch,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum LteCommand {
    Status,
    Connect,
    Disconnect,
    Apn { values: Vec<String> },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum UpdateCommand {
    Check {
        #[arg(long, value_enum)]
        severity: Option<UpdateSeverity>,
    },
    Download {
        #[arg(required = true, num_args = 1..)]
        packages: Vec<String>,
    },
    Install {
        #[arg(required = true, num_args = 1..)]
        packages: Vec<String>,
        #[arg(long)]
        reboot: bool,
    },
    History,
    Ignore {
        id: String,
    },
    Rollback {
        id: String,
    },
    Schedule {
        #[arg(value_enum)]
        frequency: UpdateSchedule,
    },
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum UpdateSeverity {
    Critical,
    Recommended,
    Driver,
    Optional,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum UpdateSchedule {
    Daily,
    Weekly,
    Off,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum ScanCommand {
    List,
    Run { items: Vec<String> },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SnapshotCommand {
    Capture,
    Diff,
    Restore,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum OsdCommand {
    Enable,
    Disable,
    Test,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum DaemonCommand {
    Start,
    Stop,
    Status,
    Install,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum Toggle {
    On,
    Off,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum ChargeModeArg {
    Normal,
    Conservation,
    Rapid,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum PowerSourceArg {
    Ac,
    Dc,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum PerfModeArg {
    #[value(alias = "balanced")]
    Auto,
    #[value(alias = "quiet")]
    Cool,
    Performance,
    Geek,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum DolbyProfileArg {
    Off,
    Movie,
    Music,
    Voice,
    Game,
    Personalize,
    Dynamic,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum NoiseCancelArg {
    Off,
    Single,
    Shared,
    Spatial,
    VoiceId,
    #[value(alias = "far-field")]
    Farfield,
}

#[derive(Clone, Debug, ValueEnum, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Debug, Serialize)]
struct InfoPayload {
    platform: Platform,
    hardware: HardwareInfo,
    features: BTreeMap<String, Capability>,
}

#[derive(Debug, Serialize)]
struct BatteryStatusPayload {
    telemetry: BatteryTelemetry,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter: Option<AdapterInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adapter_error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    charge_mode: Option<ChargeModeActual>,
    #[serde(skip_serializing_if = "Option::is_none")]
    charge_mode_error: Option<Value>,
}

#[derive(Debug, Serialize)]
struct PrivacyStatusPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_camera: Option<DeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_camera_error: Option<Value>,
    bios: Vec<lctrl_core::BiosItem>,
}

#[derive(Debug, Serialize)]
struct AppliedTunePayload {
    plan: lctrl_tune::TunePlan,
    changes: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TuneState {
    version: u32,
    platform: String,
    hardware: Value,
    active_profile: String,
    status: String,
    saved_before_apply: Vec<SavedTuneSetting>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
enum SavedTuneSetting {
    Battery {
        mode: String,
    },
    Backlight {
        level: u8,
        max_level: u8,
        effect_raw: u8,
    },
    Fan {
        mode: String,
    },
    Performance {
        mode: String,
    },
    Epp {
        value: u8,
    },
    PowerLimit {
        kind: String,
        value_uw: u64,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ManagedSnapshot {
    version: u32,
    platform: String,
    hardware: Value,
    charge_mode: Option<String>,
    performance_mode: Option<String>,
    fan_mode: Option<String>,
    epp: Option<u8>,
    pl1_uw: Option<u64>,
    pl2_uw: Option<u64>,
    tau_us: Option<u64>,
    backlight: Option<ManagedBacklight>,
    touchpad: Option<String>,
    runtime_camera: Option<String>,
    power_scheme: Option<String>,
    errors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ManagedBacklight {
    level: u8,
    max_level: u8,
    effect_raw: u8,
}

#[derive(Debug, Serialize)]
struct ManagedSnapshotDiff {
    equal: bool,
    changed: Vec<String>,
    baseline: ManagedSnapshot,
    current: ManagedSnapshot,
}

#[derive(Debug, Serialize)]
struct TuneRestorePayload {
    restored_profile: String,
    changes: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct DoctorPayload {
    platform: Platform,
    operational: usize,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    channel: &'static str,
    operational: bool,
    detail: String,
}

/// Execute a parsed CLI command using only the root HAL.
///
/// This compatibility entry point intentionally exposes no optional services.
/// Call [`execute_with_services`] from a concrete platform composition root.
pub fn execute(cli: Cli, hal: &dyn Hal) -> CommandResult {
    execute_with_services(cli, CommandServices::new(hal))
}

/// Execute a parsed CLI command without terminating the process.
pub fn execute_with_services(cli: Cli, services: CommandServices<'_>) -> CommandResult {
    let local_dry_run = matches!(
        &cli.command,
        Command::Tune {
            command: Some(TuneCommand::Profile {
                command: Some(ProfileCommand::Apply { dry_run: true, .. }),
            }),
        }
    );
    let apply = if cli.dry_run || local_dry_run {
        ApplyMode::DryRun
    } else {
        ApplyMode::Commit
    };
    let confirmed = cli.yes;
    let interactive = !cli.json;
    if apply == ApplyMode::Commit && command_is_mutating(&cli.command) {
        preflight_control_conflicts(services.conflicts, confirmed, interactive)?;
    }
    match cli.command {
        Command::Info => execute_info(services.hal),
        Command::Doctor => execute_doctor(&services),
        Command::Battery {
            command: Some(BatteryCommand::Status),
        } => execute_battery_status(services.battery),
        Command::Battery { command: None } => execute_battery_status(services.battery),
        Command::Battery {
            command: Some(BatteryCommand::Adapter),
        } => execute_battery_adapter(services.battery),
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { mode }),
        } => execute_charge_mode(services.battery, mode, apply),
        Command::Power {
            command:
                Some(PowerCommand::Scheme {
                    command: Some(scheme),
                }),
        } => execute_power_scheme(services.power, scheme, apply),
        Command::Power {
            command: Some(PowerCommand::Scheme { command: None }),
        } => execute_active_power_scheme(services.power),
        Command::Perf {
            command: Some(PerfCommand::Mode { mode }),
        } => execute_performance_mode(services.performance, mode, apply),
        Command::Perf {
            command: Some(PerfCommand::Fan {
                command: Some(command),
            }),
        } => execute_fan(services.fan, command, apply),
        Command::Perf {
            command: Some(PerfCommand::Fan { command: None }),
        } => execute_fan_status(services.fan),
        Command::Perf {
            command: Some(PerfCommand::Temp { command: None }),
        } => execute_temperature_list(services.temperature),
        Command::Perf {
            command: Some(PerfCommand::Temp {
                command: Some(command),
            }),
        } => execute_temperature(services.temperature, command),
        Command::Perf {
            command: Some(PerfCommand::Pl1 { watts }),
        } => execute_power_limit(services.tuning, PowerLimitKind::Pl1, watts, apply),
        Command::Perf {
            command: Some(PerfCommand::Pl2 { watts }),
        } => execute_power_limit(services.tuning, PowerLimitKind::Pl2, watts, apply),
        Command::Bios {
            command: Some(command),
        } => execute_bios(services.bios, command, apply, confirmed, interactive),
        Command::Bios { command: None } => execute_bios_list(services.bios),
        Command::Kbd {
            command: Some(KbdCommand::Backlight { level, effect }),
        } => execute_backlight(services.keyboard, level, effect, apply),
        Command::Kbd {
            command: Some(KbdCommand::Fnlock { state }),
        } => execute_fnlock(services.bios, state, apply, confirmed, interactive),
        Command::Kbd {
            command: Some(KbdCommand::FnCtrlSwap { state }),
        } => execute_fn_ctrl_swap(services.bios, state, apply, confirmed, interactive),
        Command::Touchpad { state } => execute_touchpad(services.touchpad, state, apply),
        Command::Panel {
            command: Some(command),
        } => execute_panel(services.panel, command, apply),
        Command::Privacy {
            command: Some(command),
        } => execute_privacy(
            services.privacy,
            services.bios,
            command,
            apply,
            confirmed,
            interactive,
        ),
        Command::Magicbay {
            command: Some(MagicbayCommand::Detect),
        } => execute_magicbay_detect(services.magicbay),
        Command::Update {
            command: Some(command),
        } => execute_update(services.update, command),
        Command::Scan {
            command: Some(command),
        } => execute_scan(services.diagnostics, command),
        Command::Snapshot {
            command: Some(command),
        } => execute_snapshot(&services, command, apply),
        Command::Tune {
            command: Some(TuneCommand::Pl1 { watts, pl2, tau }),
        } => execute_tune_power_limits(services.tuning, watts, pl2, tau, apply),
        Command::Tune {
            command: Some(TuneCommand::Epp { value }),
        } => execute_tune_epp(services.tuning, &value, apply),
        Command::Tune {
            command: Some(TuneCommand::Telemetry { .. }),
        } => execute_tune_telemetry(&services),
        Command::Tune {
            command: Some(TuneCommand::Restore),
        } => execute_tune_restore(&services),
        Command::Tune {
            command:
                Some(TuneCommand::Profile {
                    command: Some(profile),
                }),
        } => execute_tune_profile(&services, profile, apply),
        Command::Completions { shell } => execute_completions(shell),
        command => Err(unsupported(command)),
    }
}
fn command_is_mutating(command: &Command) -> bool {
    matches!(
        command,
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { .. })
        } | Command::Power {
            command: Some(PowerCommand::Scheme {
                command: Some(SchemeCommand::Apply { .. } | SchemeCommand::Set { .. })
            })
        } | Command::Perf {
            command: Some(
                PerfCommand::Mode { .. } | PerfCommand::Pl1 { .. } | PerfCommand::Pl2 { .. }
            )
        } | Command::Perf {
            command: Some(PerfCommand::Fan {
                command: Some(
                    FanCommand::Auto
                        | FanCommand::Manual
                        | FanCommand::Fullspeed
                        | FanCommand::Smart
                        | FanCommand::Curve { .. },
                )
            })
        } | Command::Tune {
            command: Some(
                TuneCommand::Profile {
                    command: Some(ProfileCommand::Apply { .. })
                } | TuneCommand::Restore
                    | TuneCommand::Pl1 { .. }
                    | TuneCommand::Epp { .. }
                    | TuneCommand::Turbo { .. },
            )
        } | Command::Snapshot {
            command: Some(SnapshotCommand::Restore)
        } | Command::Panel { command: Some(_) }
            | Command::Kbd {
                command: Some(
                    KbdCommand::Backlight { .. }
                        | KbdCommand::Fnlock { .. }
                        | KbdCommand::FnCtrlSwap { .. }
                        | KbdCommand::Winlock { .. },
                )
            }
            | Command::Touchpad { .. }
            | Command::Privacy {
                command: Some(
                    PrivacyCommand::Cam { .. }
                        | PrivacyCommand::Mic { .. }
                        | PrivacyCommand::Fingerprint { .. },
                )
            }
            | Command::Bios {
                command: Some(BiosCommand::Set { .. } | BiosCommand::Discard)
            }
    )
}

fn preflight_control_conflicts(
    detector: Option<&dyn ControlConflictDetection>,
    confirmed: bool,
    interactive: bool,
) -> lctrl_core::Result<()> {
    let Some(detector) = detector else {
        return Ok(());
    };
    let controllers = detector.active_vendor_controllers()?;
    if controllers.is_empty() {
        return Ok(());
    }
    if interactive {
        eprintln!(
            "conflicting vendor control application(s) detected: {}",
            controllers.join(", ")
        );
    }
    if confirmed {
        Ok(())
    } else {
        Err(LctrlError::InvalidArgument {
            detail: format!(
                "stop {} before writing, or re-run with --yes to accept concurrent-control risk",
                controllers.join(", ")
            ),
        })
    }
}

fn execute_doctor(services: &CommandServices<'_>) -> CommandResult {
    let mut checks = vec![
        doctor_check("hal.hardware_info", services.hal.hardware_info()),
        doctor_check("hal.capabilities", services.hal.capabilities()),
    ];
    checks.push(match services.conflicts {
        Some(service) => match service.active_vendor_controllers() {
            Ok(controllers) if controllers.is_empty() => DoctorCheck {
                channel: "control.conflicts",
                operational: true,
                detail: "no competing vendor controller detected".into(),
            },
            Ok(controllers) => DoctorCheck {
                channel: "control.conflicts",
                operational: false,
                detail: format!("competing controller(s): {}", controllers.join(", ")),
            },
            Err(error) => DoctorCheck {
                channel: "control.conflicts",
                operational: false,
                detail: error.to_string(),
            },
        },
        None => doctor_missing("control.conflicts"),
    });
    checks.push(match services.battery {
        Some(service) => doctor_check("battery.telemetry", service.battery_telemetry(0)),
        None => doctor_missing("battery.telemetry"),
    });
    checks.push(match services.bios {
        Some(service) => doctor_check("bios.settings", service.list()),
        None => doctor_missing("bios.settings"),
    });
    checks.push(match services.diagnostics {
        Some(service) => doctor_check("diagnostics.inventory", service.diagnostic_items()),
        None => doctor_missing("diagnostics.inventory"),
    });
    checks.push(match services.fan {
        Some(service) => doctor_check("fan.mode", service.fan_mode()),
        None => doctor_missing("fan.mode"),
    });
    checks.push(match services.keyboard {
        Some(service) => doctor_check("keyboard.backlight", service.backlight_state()),
        None => doctor_missing("keyboard.backlight"),
    });
    checks.push(match services.touchpad {
        Some(service) => doctor_check("touchpad.state", service.touchpad_state()),
        None => doctor_missing("touchpad.state"),
    });
    checks.push(match services.panel {
        Some(service) => doctor_check("panel.refresh", service.refresh_capability()),
        None => doctor_missing("panel.refresh"),
    });
    checks.push(match services.temperature {
        Some(service) => doctor_check("perf.temp", service.temperature_sensors()),
        None => doctor_missing("perf.temp"),
    });
    checks.push(match services.magicbay {
        Some(service) => doctor_check("magicbay.inventory", service.detect_magicbay()),
        None => doctor_missing("magicbay.inventory"),
    });
    checks.push(match services.privacy {
        Some(service) => doctor_check("privacy.camera", service.camera_state()),
        None => doctor_missing("privacy.camera"),
    });
    checks.push(match services.performance {
        Some(service) => doctor_check("performance.state", service.performance_state()),
        None => doctor_missing("performance.state"),
    });
    checks.push(match services.power {
        Some(service) => doctor_check("power.schemes", service.power_schemes()),
        None => doctor_missing("power.schemes"),
    });
    checks.push(match services.tuning {
        Some(service) => doctor_check("tune.epp", service.epp()),
        None => doctor_missing("tune.epp"),
    });
    checks.push(match services.update {
        Some(service) => match service.update_capability() {
            Ok(UpdateCapability::TrustedLocalInfOnly) => DoctorCheck {
                channel: "update.capability",
                operational: true,
                detail: "trusted local INF updates are available".into(),
            },
            Ok(UpdateCapability::Unavailable { reason }) => DoctorCheck {
                channel: "update.capability",
                operational: false,
                detail: reason,
            },
            Err(error) => DoctorCheck {
                channel: "update.capability",
                operational: false,
                detail: error.to_string(),
            },
        },
        None => doctor_missing("update.capability"),
    });

    let operational = checks.iter().filter(|check| check.operational).count();
    let mut human = format!(
        "doctor: {operational}/{} channels operational\n",
        checks.len()
    );
    for check in &checks {
        let status = if check.operational {
            "ok"
        } else {
            "unavailable"
        };
        human.push_str(&format!(
            "- {}: {status} ({})\n",
            check.channel, check.detail
        ));
    }
    structured_output(
        &DoctorPayload {
            platform: services.hal.platform(),
            operational,
            checks,
        },
        human,
    )
}

fn doctor_check<T>(channel: &'static str, result: lctrl_core::Result<T>) -> DoctorCheck {
    match result {
        Ok(_) => DoctorCheck {
            channel,
            operational: true,
            detail: "read succeeded".into(),
        },
        Err(error) => DoctorCheck {
            channel,
            operational: false,
            detail: error.to_string(),
        },
    }
}

fn doctor_missing(channel: &'static str) -> DoctorCheck {
    DoctorCheck {
        channel,
        operational: false,
        detail: "service is not wired on this platform".into(),
    }
}

fn execute_battery_adapter(battery: Option<&dyn BatteryControl>) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.adapter".into(),
    })?;
    let adapter = battery.adapter_info()?;
    structured_output(&adapter, format!("adapter: {}\n", adapter.authentication))
}

fn execute_battery_status(battery: Option<&dyn BatteryControl>) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.status".into(),
    })?;
    let telemetry = battery.battery_telemetry(0)?;
    let (adapter, adapter_error) = capture_optional(battery.adapter_info());
    let (charge_mode, charge_mode_error) = capture_optional(battery.charge_mode());
    let payload = BatteryStatusPayload {
        telemetry,
        adapter,
        adapter_error,
        charge_mode,
        charge_mode_error,
    };
    let human = format!(
        "battery: {} mWh remaining\ncharge: {}%\nadapter: {}\ncharge mode: {}\n",
        payload
            .telemetry
            .remaining_capacity_mwh
            .map_or_else(|| "unknown".into(), |value| value.to_string()),
        payload
            .telemetry
            .remaining_percent
            .map_or_else(|| "unknown".into(), |value| value.to_string()),
        payload.adapter.map_or_else(
            || "unavailable".into(),
            |value| value.authentication.to_string()
        ),
        payload
            .charge_mode
            .map_or_else(|| "unavailable".into(), |value| value.to_string())
    );
    structured_output(&payload, human)
}

fn capture_optional<T>(result: lctrl_core::Result<T>) -> (Option<T>, Option<Value>) {
    match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, serde_json::to_value(error.report()).ok()),
    }
}

fn execute_charge_mode(
    battery: Option<&dyn BatteryControl>,
    mode: ChargeModeArg,
    apply: ApplyMode,
) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.charge-mode".into(),
    })?;
    let mode = match mode {
        ChargeModeArg::Normal => ChargeMode::Normal,
        ChargeModeArg::Conservation => ChargeMode::Conservation,
        ChargeModeArg::Rapid => ChargeMode::Rapid,
    };
    let report = battery.set_charge_mode(mode, apply)?;
    structured_output(
        &report,
        format!(
            "charge mode: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
    )
}

fn execute_performance_mode(
    performance: Option<&dyn PerformanceControl>,
    mode: PerfModeArg,
    apply: ApplyMode,
) -> CommandResult {
    let performance = performance.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.mode".into(),
    })?;
    let mode = match mode {
        PerfModeArg::Auto => lctrl_core::PerformanceMode::Balanced,
        PerfModeArg::Cool => lctrl_core::PerformanceMode::Quiet,
        PerfModeArg::Performance => lctrl_core::PerformanceMode::Performance,
        PerfModeArg::Geek => lctrl_core::PerformanceMode::Geek,
    };
    let report = performance.set_performance_mode(mode, apply)?;
    structured_output(
        &report,
        format!(
            "performance mode: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
    )
}

fn execute_power_limit(
    tuning: Option<&dyn TuningControl>,
    kind: PowerLimitKind,
    watts: u32,
    apply: ApplyMode,
) -> CommandResult {
    let tuning = tuning.ok_or_else(|| LctrlError::Unsupported {
        feature: "tune.rapl".into(),
    })?;
    let microwatts =
        u64::from(watts)
            .checked_mul(1_000_000)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("power limit {watts} W overflows µW"),
            })?;
    let report = tuning.set_power_limit(kind, microwatts, apply)?;
    structured_output(&report, format!("power limit {kind:?}: {watts} W\n"))
}

fn execute_tune_power_limits(
    tuning: Option<&dyn TuningControl>,
    pl1_watts: u32,
    pl2_watts: Option<u32>,
    tau_seconds: Option<u32>,
    apply: ApplyMode,
) -> CommandResult {
    let tuning = tuning.ok_or_else(|| LctrlError::Unsupported {
        feature: "tune.rapl".into(),
    })?;
    let pl2_watts = pl2_watts.ok_or_else(|| LctrlError::InvalidArgument {
        detail: "tune pl1 requires --pl2 so PL1 <= PL2 can be validated atomically".into(),
    })?;
    if pl1_watts > pl2_watts {
        return Err(LctrlError::InvalidArgument {
            detail: format!("PL1 must not exceed PL2: {pl1_watts} > {pl2_watts} W"),
        });
    }
    let pl1 = u64::from(pl1_watts) * 1_000_000;
    let pl2 = u64::from(pl2_watts) * 1_000_000;
    let current_pl1 = tuning.power_limit(PowerLimitKind::Pl1)?;
    let current_pl2 = tuning.power_limit(PowerLimitKind::Pl2)?;
    let order = if pl1 > current_pl2 {
        [(PowerLimitKind::Pl2, pl2), (PowerLimitKind::Pl1, pl1)]
    } else if pl2 < current_pl1 {
        [(PowerLimitKind::Pl1, pl1), (PowerLimitKind::Pl2, pl2)]
    } else {
        [(PowerLimitKind::Pl2, pl2), (PowerLimitKind::Pl1, pl1)]
    };
    let mut requests = Vec::from(order);
    if let Some(tau_seconds) = tau_seconds {
        requests.push((PowerLimitKind::Tau, u64::from(tau_seconds) * 1_000_000));
    }
    let reports = apply_limit_sequence(tuning, &requests, apply)?;
    structured_output(&reports, "RAPL power limits applied and read back\n".into())
}

fn apply_limit_sequence(
    tuning: &dyn TuningControl,
    requests: &[(PowerLimitKind, u64)],
    apply: ApplyMode,
) -> lctrl_core::Result<Vec<Value>> {
    let mut reports = Vec::with_capacity(requests.len());
    let mut rollbacks = Vec::with_capacity(requests.len());
    for (kind, value) in requests {
        match tuning.set_power_limit(*kind, *value, apply) {
            Ok(report) => {
                reports.push(serialize_change(&report)?);
                if apply == ApplyMode::Commit {
                    rollbacks.push((*kind, *report.previous()));
                }
            }
            Err(error) => {
                if apply == ApplyMode::Commit {
                    rollback_limit_sequence(tuning, &rollbacks, &error)?;
                }
                return Err(error);
            }
        }
    }
    Ok(reports)
}

fn rollback_limit_sequence(
    tuning: &dyn TuningControl,
    rollbacks: &[(PowerLimitKind, u64)],
    original: &LctrlError,
) -> lctrl_core::Result<()> {
    for (kind, value) in rollbacks.iter().rev() {
        if let Err(rollback) = tuning.set_power_limit(*kind, *value, ApplyMode::Commit) {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "RAPL sequence failed ({original}); rollback also failed ({rollback})"
                ),
            });
        }
    }
    Ok(())
}

fn execute_tune_epp(
    tuning: Option<&dyn TuningControl>,
    value: &str,
    apply: ApplyMode,
) -> CommandResult {
    let tuning = tuning.ok_or_else(|| LctrlError::Unsupported {
        feature: "tune.epp".into(),
    })?;
    let value = match value {
        "performance" => 0,
        "balance-performance" | "balance_performance" => 128,
        "balance-power" | "balance_power" => 192,
        "power" => 255,
        raw => raw.parse::<u8>().map_err(|_| LctrlError::InvalidArgument {
            detail: format!("invalid EPP value {raw:?}"),
        })?,
    };
    let report = tuning.set_epp(value, apply)?;
    structured_output(&report, format!("EPP requested={value}\n"))
}

fn execute_tune_telemetry(services: &CommandServices<'_>) -> CommandResult {
    let mut telemetry = BTreeMap::new();
    if let Some(tuning) = services.tuning {
        telemetry.insert("epp", serialize_change(&capture_optional(tuning.epp()))?);
        for (name, kind) in [
            ("pl1_uw", PowerLimitKind::Pl1),
            ("pl2_uw", PowerLimitKind::Pl2),
            ("tau_us", PowerLimitKind::Tau),
        ] {
            telemetry.insert(
                name,
                serialize_change(&capture_optional(tuning.power_limit(kind)))?,
            );
        }
    }
    if let Some(battery) = services.battery {
        telemetry.insert(
            "battery",
            serialize_change(&capture_optional(battery.battery_telemetry(0)))?,
        );
    }
    if let Some(performance) = services.performance {
        telemetry.insert(
            "performance",
            serialize_change(&capture_optional(performance.performance_state()))?,
        );
    }
    structured_output(&telemetry, "tuning telemetry collected\n".into())
}

fn execute_temperature_list(temperature: Option<&dyn TemperatureControl>) -> CommandResult {
    let temperature = temperature.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.temp".into(),
    })?;
    let sensors = temperature.temperature_sensors()?;
    structured_output(
        &sensors,
        format!("{} temperature sensor(s)\n", sensors.len()),
    )
}

fn execute_fan_status(fan: Option<&dyn FanControl>) -> CommandResult {
    let fan = fan.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.fan".into(),
    })?;
    let mode = fan.fan_mode()?;
    let fans = fan.fans().unwrap_or_default();
    structured_output(
        &serde_json::json!({ "mode": mode, "fans": fans }),
        format!("fan mode: {mode}\n"),
    )
}

fn execute_temperature(
    temperature: Option<&dyn TemperatureControl>,
    command: TempCommand,
) -> CommandResult {
    let temperature = temperature.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.temp".into(),
    })?;
    match command {
        TempCommand::Read { id } => {
            let sensor = temperature.temperature(&id)?;
            structured_output(
                &sensor,
                format!(
                    "{}: {}°C\n",
                    sensor.metadata.name,
                    sensor
                        .value_c
                        .map_or("unknown".into(), |value| format!("{value:.1}"))
                ),
            )
        }
        TempCommand::Watch => Err(LctrlError::Unsupported {
            feature: "perf.temp.watch.daemon-events".into(),
        }),
    }
}

fn execute_fan(
    fan: Option<&dyn FanControl>,
    command: FanCommand,
    apply: ApplyMode,
) -> CommandResult {
    let fan = fan.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.fan".into(),
    })?;
    match command {
        FanCommand::Status => {
            let mode = fan.fan_mode()?;
            structured_output(&mode, format!("fan mode: {mode:?}\n"))
        }
        FanCommand::Auto | FanCommand::Smart => {
            let report = fan.set_fan_mode(FanMode::Standard, apply)?;
            structured_output(&report, "fan mode: balanced/automatic\n".into())
        }
        FanCommand::Fullspeed => {
            let report = fan.set_fan_mode(FanMode::Performance, apply)?;
            structured_output(&report, "fan mode: maximum performance\n".into())
        }
        FanCommand::Manual => {
            let report = fan.set_fan_mode(FanMode::Custom, apply)?;
            structured_output(&report, "fan mode: manual/custom\n".into())
        }
        FanCommand::Curve { .. } => Err(LctrlError::Unsupported {
            feature: "perf.fan.curve.kernel-interface".into(),
        }),
    }
}

fn execute_panel(
    panel: Option<&dyn PanelControl>,
    command: PanelCommand,
    apply: ApplyMode,
) -> CommandResult {
    let panel = panel.ok_or_else(|| LctrlError::Unsupported {
        feature: "panel".into(),
    })?;
    match command {
        PanelCommand::Rate { rate } => {
            let capability = panel.refresh_capability()?;
            let requested = match rate {
                PanelRate::Fps60 => Some(60),
                PanelRate::Fps120 => Some(120),
                PanelRate::Auto => None,
            };
            if let Some(hz) = requested {
                if !capability.supports_hz(hz) {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!("panel refresh rate {hz} Hz is outside supported range"),
                    });
                }
            }
            if apply == ApplyMode::DryRun {
                return structured_output(
                    &serde_json::json!({
                        "requested": format!("{rate:?}"),
                        "capability": capability,
                    }),
                    "panel refresh dry run validated; no mode writer is attached\n".into(),
                );
            }
            Err(LctrlError::Unsupported {
                feature: "panel.rate.write".into(),
            })
        }
        PanelCommand::Color { .. } => Err(LctrlError::Unsupported {
            feature: "panel.color.write".into(),
        }),
        PanelCommand::SuperResolution { .. } => Err(LctrlError::Unsupported {
            feature: "panel.super-resolution.write".into(),
        }),
        PanelCommand::Overdrive { .. } => Err(LctrlError::Unsupported {
            feature: "panel.overdrive.write".into(),
        }),
        PanelCommand::EyeCare { .. } => Err(LctrlError::Unsupported {
            feature: "panel.eye-care.write".into(),
        }),
    }
}

fn execute_active_power_scheme(power: Option<&dyn PowerControl>) -> CommandResult {
    let power = power.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.scheme".into(),
    })?;
    let scheme = power.active_power_scheme()?;
    structured_output(&scheme, format!("active power scheme: {}\n", scheme.name))
}

fn execute_power_scheme(
    power: Option<&dyn PowerControl>,
    command: SchemeCommand,
    apply: ApplyMode,
) -> CommandResult {
    let power = power.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.scheme".into(),
    })?;
    match command {
        SchemeCommand::List => {
            let schemes = power.power_schemes()?;
            structured_output(&schemes, format!("{} power scheme(s)\n", schemes.len()))
        }
        SchemeCommand::Get { name } => {
            let schemes = power.power_schemes()?;
            let scheme = schemes
                .into_iter()
                .find(|scheme| {
                    scheme.id.as_str() == name || scheme.name.eq_ignore_ascii_case(&name)
                })
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("power scheme {name:?} is not enumerated"),
                })?;
            structured_output(&scheme, format!("power scheme: {}\n", scheme.name))
        }
        SchemeCommand::Apply { name } => {
            let mutation = PowerMutation::Activate(PowerSchemeId::new(name)?);
            let report = power.apply_power_mutation(mutation, apply)?;
            structured_output(&report, "power scheme activation requested\n".into())
        }
        SchemeCommand::Set {
            subgroup,
            setting,
            source,
            value,
        } => {
            let key = PowerSettingKey {
                subgroup: PowerGuid::new(subgroup)?,
                setting: PowerGuid::new(setting)?,
            };
            let source = match source {
                PowerSourceArg::Ac => PowerSource::Ac,
                PowerSourceArg::Dc => PowerSource::Dc,
            };
            let range = power.power_value_range(&key)?;
            let mutation = PowerMutation::SetValue {
                key,
                source,
                value: PowerSettingValue::new(value, &range)?,
            };
            let report = power.apply_power_mutation(mutation, apply)?;
            structured_output(&report, "power setting requested\n".into())
        }
    }
}

fn execute_magicbay_detect(magicbay: Option<&dyn MagicBayControl>) -> CommandResult {
    let magicbay = magicbay.ok_or_else(|| LctrlError::Unsupported {
        feature: "magicbay.detect".into(),
    })?;
    let inventory = magicbay.detect_magicbay()?;
    let count = inventory.devices.len() + inventory.acpi_devices.len();
    structured_output(
        &inventory,
        format!("{count} MagicBay device record(s) detected\n"),
    )
}

fn execute_completions(shell: Shell) -> CommandResult {
    use clap::CommandFactory;

    let shell = match shell {
        Shell::Bash => clap_complete::Shell::Bash,
        Shell::Elvish => clap_complete::Shell::Elvish,
        Shell::Fish => clap_complete::Shell::Fish,
        Shell::Powershell => clap_complete::Shell::PowerShell,
        Shell::Zsh => clap_complete::Shell::Zsh,
    };
    let mut command = Cli::command();
    let mut generated = Vec::new();
    clap_complete::generate(shell, &mut command, "vantage", &mut generated);
    let completion = String::from_utf8(generated).map_err(|error| {
        LctrlError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })?;
    Ok(CommandOutput {
        human: completion.clone(),
        json: Value::String(completion),
    })
}

fn load_profile_catalog() -> lctrl_core::Result<lctrl_tune::ProfileCatalog> {
    let system_dir = env::var_os("VANTAGE_SYSTEM_PROFILE_DIR")
        .map(PathBuf::from)
        .or_else(default_system_profile_dir);
    let user_dir = env::var_os("VANTAGE_USER_PROFILE_DIR")
        .map(PathBuf::from)
        .or_else(default_user_profile_dir);
    let system = system_dir.as_deref().map_or_else(
        || Ok(Vec::new()),
        |path| load_profile_layer(path, lctrl_tune::ProfileOrigin::System),
    )?;
    let user = user_dir.as_deref().map_or_else(
        || Ok(Vec::new()),
        |path| load_profile_layer(path, lctrl_tune::ProfileOrigin::User),
    )?;
    lctrl_tune::ProfileCatalog::layered(system, user)
}

fn default_system_profile_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("ProgramData").map(|root| PathBuf::from(root).join("vantage/profiles.d"))
    } else {
        Some(PathBuf::from("/etc/vantage/profiles.d"))
    }
}

fn default_user_profile_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("APPDATA").map(|root| PathBuf::from(root).join("vantage/profiles.d"))
    } else if let Some(root) = env::var_os("XDG_CONFIG_HOME") {
        Some(PathBuf::from(root).join("vantage/profiles.d"))
    } else {
        env::var_os("HOME").map(|root| PathBuf::from(root).join(".config/vantage/profiles.d"))
    }
}

fn load_profile_layer(
    directory: &Path,
    origin: lctrl_tune::ProfileOrigin,
) -> lctrl_core::Result<Vec<lctrl_tune::ProfileDocument>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(LctrlError::Io(error)),
    };
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()).map_err(LctrlError::Io))
        .collect::<lctrl_core::Result<Vec<_>>>()?;
    paths.retain(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
    });
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).map_err(LctrlError::Io)?;
            lctrl_tune::parse_profile_toml(&source, origin).map_err(|error| {
                LctrlError::InvalidArgument {
                    detail: format!("profile {}: {error}", path.display()),
                }
            })
        })
        .collect()
}

fn execute_tune_profile(
    services: &CommandServices<'_>,
    command: ProfileCommand,
    global_apply: ApplyMode,
) -> CommandResult {
    let catalog = load_profile_catalog()?;
    match command {
        ProfileCommand::List => {
            let profiles: Vec<_> = catalog.ranked().into_iter().cloned().collect();
            structured_output(&profiles, format!("{} tuning profile(s)\n", profiles.len()))
        }
        ProfileCommand::Show { name } => {
            let profile = catalog
                .get(&name)
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("tuning profile {name:?} was not found"),
                })?;
            structured_output(profile, format!("tuning profile: {name}\n"))
        }
        ProfileCommand::Apply { name, dry_run } => {
            let profile = catalog
                .get(&name)
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("tuning profile {name:?} was not found"),
                })?;
            let mode = if dry_run || global_apply == ApplyMode::DryRun {
                ApplyMode::DryRun
            } else {
                ApplyMode::Commit
            };
            let hardware = services.hal.hardware_info()?;
            let capabilities = services.hal.capabilities()?;
            let plan = lctrl_tune::Planner::compile(
                profile,
                services.hal.platform(),
                &hardware,
                &capabilities,
                mode,
            )?;
            if mode == ApplyMode::DryRun {
                return structured_output(&plan, format!("tuning plan for {name}\n"));
            }
            let snapshot = snapshot_tune_plan(services, &plan)?;
            write_tune_state(services, &name, &snapshot, "preparing")?;
            let changes = match apply_tune_plan(services, &plan) {
                Ok(changes) => changes,
                Err(error) => {
                    match write_tune_state(services, &name, &snapshot, "recovery_required") {
                        Ok(()) => return Err(error),
                        Err(state_error) => {
                            return Err(LctrlError::FirmwareRejected {
                                detail: format!(
                                    "tuning profile failed ({error}); persisting recovery state also failed ({state_error})"
                                ),
                            });
                        }
                    }
                }
            };
            write_tune_state(services, &name, &snapshot, "active")?;
            structured_output(
                &AppliedTunePayload { plan, changes },
                format!("tuning profile {name} applied and read back\n"),
            )
        }
    }
}

#[derive(Clone)]
enum TuneRollback {
    Battery(ChargeMode),
    Backlight(BacklightState),
    Performance(PerformanceMode),
    Fan(FanMode),
    Epp(u8),
    PowerLimit(PowerLimitKind, u64),
}

fn snapshot_tune_plan(
    services: &CommandServices<'_>,
    plan: &lctrl_tune::TunePlan,
) -> lctrl_core::Result<Vec<TuneRollback>> {
    plan.writes
        .iter()
        .map(|setting| match setting {
            lctrl_tune::TuneSetting::EcMode(_) => {
                let service = services
                    .performance
                    .ok_or_else(|| LctrlError::Unsupported {
                        feature: "perf.mode".into(),
                    })?;
                let state = service.performance_state()?;
                state
                    .active
                    .or(state.requested)
                    .map(TuneRollback::Performance)
                    .ok_or_else(|| LctrlError::ChannelUnavailable {
                        channel: "performance mode snapshot".into(),
                    })
            }
            lctrl_tune::TuneSetting::ChargeMode(_) => {
                let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
                    feature: "battery.charge_mode".into(),
                })?;
                let mode = match service.charge_mode()? {
                    ChargeModeActual::Normal => ChargeMode::Normal,
                    ChargeModeActual::Conservation => ChargeMode::Conservation,
                    ChargeModeActual::Rapid => ChargeMode::Rapid,
                    actual => {
                        return Err(LctrlError::ChannelUnavailable {
                            channel: format!("unsafe charge-mode snapshot state {actual}"),
                        });
                    }
                };
                Ok(TuneRollback::Battery(mode))
            }
            lctrl_tune::TuneSetting::Backlight(_) => {
                let service = services.keyboard.ok_or_else(|| LctrlError::Unsupported {
                    feature: "kbd.backlight".into(),
                })?;
                service.backlight_state().map(TuneRollback::Backlight)
            }
            lctrl_tune::TuneSetting::FanMode(_) => {
                let service = services.fan.ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.fan.mode".into(),
                })?;
                service.fan_mode().map(TuneRollback::Fan)
            }
            lctrl_tune::TuneSetting::Epp(_) => {
                let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                    feature: "tune.epp".into(),
                })?;
                service.epp().map(TuneRollback::Epp)
            }
            lctrl_tune::TuneSetting::Pl1(_) => {
                let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                    feature: "tune.pl1".into(),
                })?;
                service
                    .power_limit(PowerLimitKind::Pl1)
                    .map(|value| TuneRollback::PowerLimit(PowerLimitKind::Pl1, value))
            }
            lctrl_tune::TuneSetting::Pl2(_) => {
                let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                    feature: "tune.pl2".into(),
                })?;
                service
                    .power_limit(PowerLimitKind::Pl2)
                    .map(|value| TuneRollback::PowerLimit(PowerLimitKind::Pl2, value))
            }
            lctrl_tune::TuneSetting::Tau(_) => {
                let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                    feature: "tune.tau".into(),
                })?;
                service
                    .power_limit(PowerLimitKind::Tau)
                    .map(|value| TuneRollback::PowerLimit(PowerLimitKind::Tau, value))
            }
            unsupported => Err(LctrlError::Unsupported {
                feature: format!("tune.{}.snapshot", unsupported.target()),
            }),
        })
        .collect()
}

fn saved_tune_setting(rollback: &TuneRollback) -> SavedTuneSetting {
    match rollback {
        TuneRollback::Battery(mode) => SavedTuneSetting::Battery {
            mode: mode.to_string(),
        },
        TuneRollback::Backlight(state) => SavedTuneSetting::Backlight {
            level: state.level,
            max_level: state.max_level,
            effect_raw: state.effect.raw(),
        },
        TuneRollback::Fan(mode) => SavedTuneSetting::Fan {
            mode: mode.to_string(),
        },
        TuneRollback::Performance(mode) => SavedTuneSetting::Performance {
            mode: mode.to_string(),
        },
        TuneRollback::Epp(value) => SavedTuneSetting::Epp { value: *value },
        TuneRollback::PowerLimit(kind, value_uw) => SavedTuneSetting::PowerLimit {
            kind: power_limit_kind_name(*kind).into(),
            value_uw: *value_uw,
        },
    }
}

fn tune_state_path() -> lctrl_core::Result<PathBuf> {
    if let Some(path) = env::var_os("VANTAGE_STATE_PATH") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(windows) {
        return env::var_os("ProgramData")
            .map(|root| PathBuf::from(root).join("vantage/state.json"))
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "ProgramData environment variable".into(),
            });
    }
    Ok(env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || PathBuf::from("/run/vantage/state.json"),
        |root| PathBuf::from(root).join("vantage/state.json"),
    ))
}

fn write_tune_state(
    services: &CommandServices<'_>,
    profile: &str,
    snapshot: &[TuneRollback],
    status: &str,
) -> lctrl_core::Result<()> {
    let path = tune_state_path()?;
    let parent = path.parent().ok_or_else(|| LctrlError::InvalidArgument {
        detail: format!("state path {} has no parent", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(LctrlError::Io)?;
    let hardware = serialize_change(&services.hal.hardware_info()?)?;
    let platform = platform_label(services.hal.platform()).to_owned();
    let saved_before_apply = match fs::read_to_string(&path) {
        Ok(source) => match serde_json::from_str::<TuneState>(&source) {
            Ok(existing) if existing.version == 1 => {
                if existing.platform != platform || existing.hardware != hardware {
                    return Err(LctrlError::InvalidArgument {
                        detail: "existing tuning recovery state belongs to different hardware"
                            .into(),
                    });
                }
                existing.saved_before_apply
            }
            Ok(existing) => {
                return Err(LctrlError::InvalidArgument {
                    detail: format!("unsupported tuning state version {}", existing.version),
                });
            }
            Err(error) => {
                return Err(LctrlError::InvalidArgument {
                    detail: format!("invalid tuning state {}: {error}", path.display()),
                });
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            snapshot.iter().map(saved_tune_setting).collect()
        }
        Err(error) => return Err(LctrlError::Io(error)),
    };
    let state = TuneState {
        version: 1,
        platform,
        hardware,
        active_profile: profile.into(),
        status: status.into(),
        saved_before_apply,
    };
    let encoded = serde_json::to_vec_pretty(&state)
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, encoded).map_err(LctrlError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(LctrlError::Io)?;
    }
    replace_file_atomic(&temporary, &path)
}

fn read_tune_state() -> lctrl_core::Result<(PathBuf, TuneState)> {
    let path = tune_state_path()?;
    let source = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            LctrlError::Unsupported {
                feature: "tune.restore.snapshot".into(),
            }
        } else {
            LctrlError::Io(error)
        }
    })?;
    let state: TuneState =
        serde_json::from_str(&source).map_err(|error| LctrlError::InvalidArgument {
            detail: format!("invalid tuning state {}: {error}", path.display()),
        })?;
    if state.version != 1 {
        return Err(LctrlError::InvalidArgument {
            detail: format!("unsupported tuning state version {}", state.version),
        });
    }
    if !matches!(
        state.status.as_str(),
        "preparing" | "active" | "recovery_required"
    ) {
        return Err(LctrlError::InvalidArgument {
            detail: format!("invalid tuning state status {:?}", state.status),
        });
    }
    Ok((path, state))
}

fn saved_tune_rollback(saved: SavedTuneSetting) -> lctrl_core::Result<TuneRollback> {
    match saved {
        SavedTuneSetting::Battery { mode } => Ok(TuneRollback::Battery(match mode.as_str() {
            "normal" => ChargeMode::Normal,
            "conservation" => ChargeMode::Conservation,
            "rapid" => ChargeMode::Rapid,
            _ => {
                return Err(LctrlError::InvalidArgument {
                    detail: format!("invalid saved charge mode {mode:?}"),
                });
            }
        })),
        SavedTuneSetting::Backlight {
            level,
            max_level,
            effect_raw,
        } => Ok(TuneRollback::Backlight(BacklightState::new(
            level,
            max_level,
            lctrl_core::LightingEffect::from_raw(effect_raw),
        )?)),
        SavedTuneSetting::Fan { mode } => Ok(TuneRollback::Fan(match mode.as_str() {
            "standard" => FanMode::Standard,
            "silent" => FanMode::Silent,
            "performance" => FanMode::Performance,
            "custom" => FanMode::Custom,
            _ => {
                return Err(LctrlError::InvalidArgument {
                    detail: format!("invalid saved fan mode {mode:?}"),
                });
            }
        })),
        SavedTuneSetting::Performance { mode } => {
            Ok(TuneRollback::Performance(match mode.as_str() {
                "balanced" => PerformanceMode::Balanced,
                "quiet" => PerformanceMode::Quiet,
                "performance" => PerformanceMode::Performance,
                "geek" => PerformanceMode::Geek,
                "silent-high-performance" => PerformanceMode::SilentHighPerformance,
                "custom" => PerformanceMode::Custom,
                _ => {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!("invalid saved performance mode {mode:?}"),
                    });
                }
            }))
        }
        SavedTuneSetting::Epp { value } => Ok(TuneRollback::Epp(value)),
        SavedTuneSetting::PowerLimit { kind, value_uw } => Ok(TuneRollback::PowerLimit(
            parse_power_limit_kind(&kind)?,
            value_uw,
        )),
    }
}

fn power_limit_kind_name(kind: PowerLimitKind) -> &'static str {
    match kind {
        PowerLimitKind::Pl1 => "pl1",
        PowerLimitKind::Pl2 => "pl2",
        PowerLimitKind::Tau => "tau",
    }
}

fn parse_power_limit_kind(value: &str) -> lctrl_core::Result<PowerLimitKind> {
    match value {
        "pl1" => Ok(PowerLimitKind::Pl1),
        "pl2" => Ok(PowerLimitKind::Pl2),
        "tau" => Ok(PowerLimitKind::Tau),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("invalid saved power-limit kind {value:?}"),
        }),
    }
}

fn snapshot_tune_rollback(
    services: &CommandServices<'_>,
    desired: &TuneRollback,
) -> lctrl_core::Result<TuneRollback> {
    match desired {
        TuneRollback::Battery(_) => {
            let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
                feature: "battery.charge_mode.restore".into(),
            })?;
            let mode = match service.charge_mode()? {
                ChargeModeActual::Normal => ChargeMode::Normal,
                ChargeModeActual::Conservation => ChargeMode::Conservation,
                ChargeModeActual::Rapid => ChargeMode::Rapid,
                actual => {
                    return Err(LctrlError::ChannelUnavailable {
                        channel: format!("unsafe charge-mode restore snapshot state {actual}"),
                    });
                }
            };
            Ok(TuneRollback::Battery(mode))
        }
        TuneRollback::Backlight(_) => services
            .keyboard
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "kbd.backlight.restore".into(),
            })?
            .backlight_state()
            .map(TuneRollback::Backlight),
        TuneRollback::Fan(_) => services
            .fan
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.fan.mode.restore".into(),
            })?
            .fan_mode()
            .map(TuneRollback::Fan),
        TuneRollback::Performance(_) => {
            let service = services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.mode.restore".into(),
                })?;
            let state = service.performance_state()?;
            state
                .active
                .or(state.requested)
                .map(TuneRollback::Performance)
                .ok_or_else(|| LctrlError::ChannelUnavailable {
                    channel: "performance mode restore snapshot".into(),
                })
        }
        TuneRollback::Epp(_) => services
            .tuning
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "tune.epp.restore".into(),
            })?
            .epp()
            .map(TuneRollback::Epp),
        TuneRollback::PowerLimit(kind, _) => services
            .tuning
            .ok_or_else(|| LctrlError::Unsupported {
                feature: format!("tune.{}.restore", power_limit_kind_name(*kind)),
            })?
            .power_limit(*kind)
            .map(|value| TuneRollback::PowerLimit(*kind, value)),
    }
}
fn restore_tune_setting(
    services: &CommandServices<'_>,
    rollback: TuneRollback,
) -> lctrl_core::Result<Value> {
    match rollback {
        TuneRollback::Battery(mode) => {
            let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
                feature: "battery.charge_mode.restore".into(),
            })?;
            serialize_change(&service.set_charge_mode(mode, ApplyMode::Commit)?)
        }
        TuneRollback::Backlight(state) => {
            let service = services.keyboard.ok_or_else(|| LctrlError::Unsupported {
                feature: "kbd.backlight.restore".into(),
            })?;
            serialize_change(&service.set_backlight(
                state.level,
                state.effect,
                ApplyMode::Commit,
            )?)
        }
        TuneRollback::Fan(mode) => {
            let service = services.fan.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.fan.mode.restore".into(),
            })?;
            serialize_change(&service.set_fan_mode(mode, ApplyMode::Commit)?)
        }
        TuneRollback::Performance(mode) => {
            let service = services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.mode.restore".into(),
                })?;
            serialize_change(&service.set_performance_mode(mode, ApplyMode::Commit)?)
        }
        TuneRollback::Epp(value) => {
            let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                feature: "tune.epp.restore".into(),
            })?;
            serialize_change(&service.set_epp(value, ApplyMode::Commit)?)
        }
        TuneRollback::PowerLimit(kind, value) => {
            let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                feature: format!("tune.{}.restore", power_limit_kind_name(kind)),
            })?;
            serialize_change(&service.set_power_limit(kind, value, ApplyMode::Commit)?)
        }
    }
}

fn execute_tune_restore(services: &CommandServices<'_>) -> CommandResult {
    let (path, state) = read_tune_state()?;
    let current_hardware = serialize_change(&services.hal.hardware_info()?)?;
    if state.platform != platform_label(services.hal.platform())
        || state.hardware.is_null()
        || state.hardware != current_hardware
    {
        return Err(LctrlError::InvalidArgument {
            detail: "tuning recovery state does not match this platform/hardware; refusing restore"
                .into(),
        });
    }
    let mut changes = Vec::with_capacity(state.saved_before_apply.len());
    let mut applied_rollbacks = Vec::with_capacity(state.saved_before_apply.len());
    for saved in state.saved_before_apply.into_iter().rev() {
        let desired = match saved_tune_rollback(saved) {
            Ok(desired) => desired,
            Err(error) => return Err(rollback_tune_changes(services, applied_rollbacks, error)),
        };
        let inverse = match snapshot_tune_rollback(services, &desired) {
            Ok(inverse) => inverse,
            Err(error) => return Err(rollback_tune_changes(services, applied_rollbacks, error)),
        };
        match restore_tune_setting(services, desired) {
            Ok(change) => {
                changes.push(change);
                applied_rollbacks.push(inverse);
            }
            Err(error) => return Err(rollback_tune_changes(services, applied_rollbacks, error)),
        }
    }
    fs::remove_file(path).map_err(LctrlError::Io)?;
    structured_output(
        &TuneRestorePayload {
            restored_profile: state.active_profile,
            changes,
        },
        "tuning snapshot restored and read back\n".into(),
    )
}

fn managed_snapshot_path() -> lctrl_core::Result<PathBuf> {
    if let Some(path) = env::var_os("VANTAGE_SNAPSHOT_PATH") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(windows) {
        return env::var_os("ProgramData")
            .map(|root| PathBuf::from(root).join("vantage/snapshot.json"))
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "ProgramData environment variable".into(),
            });
    }
    if let Some(root) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(root).join("vantage/snapshot.json"));
    }
    env::var_os("HOME")
        .map(|root| PathBuf::from(root).join(".local/state/vantage/snapshot.json"))
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "HOME or XDG_STATE_HOME".into(),
        })
}

fn capture_managed_snapshot(services: &CommandServices<'_>) -> ManagedSnapshot {
    let mut errors = BTreeMap::new();
    let hardware = match services.hal.hardware_info() {
        Ok(hardware) => serialize_change(&hardware).unwrap_or(Value::Null),
        Err(error) => {
            errors.insert("hardware".into(), error.to_string());
            Value::Null
        }
    };
    let charge_mode = services
        .battery
        .and_then(|service| match service.charge_mode() {
            Ok(mode) => Some(mode.to_string()),
            Err(error) => {
                errors.insert("charge_mode".into(), error.to_string());
                None
            }
        });
    let performance_mode =
        services
            .performance
            .and_then(|service| match service.performance_state() {
                Ok(state) => state
                    .active
                    .or(state.requested)
                    .map(|mode| mode.to_string()),
                Err(error) => {
                    errors.insert("performance_mode".into(), error.to_string());
                    None
                }
            });
    let fan_mode = services.fan.and_then(|service| match service.fan_mode() {
        Ok(mode) => Some(mode.to_string()),
        Err(error) => {
            errors.insert("fan_mode".into(), error.to_string());
            None
        }
    });
    let epp = services.tuning.and_then(|service| match service.epp() {
        Ok(value) => Some(value),
        Err(error) => {
            errors.insert("epp".into(), error.to_string());
            None
        }
    });
    let pl1_uw =
        services
            .tuning
            .and_then(|service| match service.power_limit(PowerLimitKind::Pl1) {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.insert("pl1_uw".into(), error.to_string());
                    None
                }
            });
    let pl2_uw =
        services
            .tuning
            .and_then(|service| match service.power_limit(PowerLimitKind::Pl2) {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.insert("pl2_uw".into(), error.to_string());
                    None
                }
            });
    let tau_us =
        services
            .tuning
            .and_then(|service| match service.power_limit(PowerLimitKind::Tau) {
                Ok(value) => Some(value),
                Err(error) => {
                    errors.insert("tau_us".into(), error.to_string());
                    None
                }
            });
    let backlight = services
        .keyboard
        .and_then(|service| match service.backlight_state() {
            Ok(state) => Some(ManagedBacklight {
                level: state.level,
                max_level: state.max_level,
                effect_raw: state.effect.raw(),
            }),
            Err(error) => {
                errors.insert("backlight".into(), error.to_string());
                None
            }
        });
    let touchpad = services
        .touchpad
        .and_then(|service| match service.touchpad_state() {
            Ok(state) => Some(state.to_string()),
            Err(error) => {
                errors.insert("touchpad".into(), error.to_string());
                None
            }
        });
    let runtime_camera = services
        .privacy
        .and_then(|service| match service.camera_state() {
            Ok(state) => Some(state.to_string()),
            Err(error) => {
                errors.insert("runtime_camera".into(), error.to_string());
                None
            }
        });
    let power_scheme = services
        .power
        .and_then(|service| match service.active_power_scheme() {
            Ok(scheme) => Some(scheme.id.as_str().to_owned()),
            Err(error) => {
                errors.insert("power_scheme".into(), error.to_string());
                None
            }
        });
    ManagedSnapshot {
        version: 1,
        platform: platform_label(services.hal.platform()).into(),
        hardware,
        charge_mode,
        performance_mode,
        fan_mode,
        epp,
        pl1_uw,
        pl2_uw,
        tau_us,
        backlight,
        touchpad,
        runtime_camera,
        power_scheme,
        errors,
    }
}

fn replace_file_atomic(temporary: &Path, destination: &Path) -> lctrl_core::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source: Vec<u16> = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let target: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let replaced = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced == 0 {
            return Err(LctrlError::Io(std::io::Error::last_os_error()));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    fs::rename(temporary, destination).map_err(LctrlError::Io)
}

fn write_managed_snapshot(snapshot: &ManagedSnapshot) -> lctrl_core::Result<PathBuf> {
    let path = managed_snapshot_path()?;
    let parent = path.parent().ok_or_else(|| LctrlError::InvalidArgument {
        detail: format!("snapshot path {} has no parent", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(LctrlError::Io)?;
    let bytes = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(LctrlError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
            .map_err(LctrlError::Io)?;
    }
    replace_file_atomic(&temporary, &path)?;
    Ok(path)
}

fn read_managed_snapshot() -> lctrl_core::Result<ManagedSnapshot> {
    let path = managed_snapshot_path()?;
    let source = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            LctrlError::Unsupported {
                feature: "snapshot.baseline".into(),
            }
        } else {
            LctrlError::Io(error)
        }
    })?;
    let snapshot: ManagedSnapshot =
        serde_json::from_str(&source).map_err(|error| LctrlError::InvalidArgument {
            detail: format!("invalid snapshot {}: {error}", path.display()),
        })?;
    if snapshot.version != 1 {
        return Err(LctrlError::InvalidArgument {
            detail: format!("unsupported snapshot version {}", snapshot.version),
        });
    }
    Ok(snapshot)
}

fn managed_snapshot_changes(baseline: &ManagedSnapshot, current: &ManagedSnapshot) -> Vec<String> {
    let mut changed = Vec::new();
    macro_rules! compare {
        ($field:ident) => {
            if baseline.$field != current.$field {
                changed.push(stringify!($field).into());
            }
        };
    }
    compare!(platform);
    compare!(hardware);
    compare!(charge_mode);
    compare!(performance_mode);
    compare!(fan_mode);
    compare!(epp);
    compare!(pl1_uw);
    compare!(pl2_uw);
    compare!(tau_us);
    compare!(backlight);
    compare!(touchpad);
    compare!(runtime_camera);
    compare!(power_scheme);
    compare!(errors);
    changed
}

fn execute_snapshot(
    services: &CommandServices<'_>,
    command: SnapshotCommand,
    apply: ApplyMode,
) -> CommandResult {
    match command {
        SnapshotCommand::Capture => {
            let snapshot = capture_managed_snapshot(services);
            let human = if apply == ApplyMode::DryRun {
                "snapshot capture dry run; baseline not written\n".into()
            } else {
                let path = write_managed_snapshot(&snapshot)?;
                format!("snapshot baseline written to {}\n", path.display())
            };
            structured_output(&snapshot, human)
        }
        SnapshotCommand::Diff => {
            let baseline = read_managed_snapshot()?;
            let current = capture_managed_snapshot(services);
            let changed = managed_snapshot_changes(&baseline, &current);
            structured_output(
                &ManagedSnapshotDiff {
                    equal: changed.is_empty(),
                    changed,
                    baseline,
                    current,
                },
                "snapshot comparison complete\n".into(),
            )
        }
        SnapshotCommand::Restore => {
            let baseline = read_managed_snapshot()?;
            let changes = restore_managed_snapshot(services, &baseline, apply)?;
            structured_output(
                &changes,
                if apply == ApplyMode::DryRun {
                    "snapshot restore dry run validated\n".into()
                } else {
                    "snapshot restored and read back\n".into()
                },
            )
        }
    }
}

#[derive(Clone)]
enum ManagedRollback {
    Battery(ChargeMode),
    Performance(PerformanceMode),
    Fan(FanMode),
    Epp(u8),
    PowerLimit(PowerLimitKind, u64),
    Backlight(BacklightState),
    Touchpad(DeviceState),
    Camera(DeviceState),
    Power(PowerSchemeId),
}

fn restore_managed_snapshot(
    services: &CommandServices<'_>,
    baseline: &ManagedSnapshot,
    apply: ApplyMode,
) -> lctrl_core::Result<Vec<Value>> {
    let current = capture_managed_snapshot(services);
    if baseline.platform != current.platform
        || baseline.hardware.is_null()
        || current.hardware.is_null()
        || baseline.hardware != current.hardware
        || baseline.errors.contains_key("hardware")
        || current.errors.contains_key("hardware")
    {
        return Err(LctrlError::InvalidArgument {
            detail: "snapshot identity does not match this platform/hardware; refusing restore"
                .into(),
        });
    }
    let mut changes = Vec::new();
    let mut rollbacks = Vec::new();
    macro_rules! apply_change {
        ($expression:expr, $rollback:expr) => {{
            let report = match $expression {
                Ok(report) => report,
                Err(error) => {
                    return Err(rollback_managed_changes(services, rollbacks, error));
                }
            };
            let value = match serialize_change(&report) {
                Ok(value) => value,
                Err(error) => {
                    return Err(rollback_managed_changes(services, rollbacks, error));
                }
            };
            if apply == ApplyMode::Commit {
                rollbacks.push($rollback(&report));
            }
            changes.push(value);
        }};
    }

    if baseline.charge_mode != current.charge_mode
        && let Some(mode) = baseline.charge_mode.as_deref()
    {
        let mode = parse_saved_charge_mode(mode)?;
        let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.charge_mode".into(),
        })?;
        apply_change!(
            service.set_charge_mode(mode, apply),
            |report: &lctrl_core::ChangeReport<ChargeMode>| {
                ManagedRollback::Battery(*report.previous())
            }
        );
    }
    if baseline.performance_mode != current.performance_mode
        && let Some(mode) = baseline.performance_mode.as_deref()
    {
        let mode = parse_saved_performance_mode(mode)?;
        let service = services
            .performance
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "snapshot.restore.performance_mode".into(),
            })?;
        apply_change!(
            service.set_performance_mode(mode, apply),
            |report: &lctrl_core::ChangeReport<PerformanceMode>| {
                ManagedRollback::Performance(*report.previous())
            }
        );
    }
    if baseline.fan_mode != current.fan_mode
        && let Some(mode) = baseline.fan_mode.as_deref()
    {
        let mode = parse_saved_fan_mode(mode)?;
        let service = services.fan.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.fan_mode".into(),
        })?;
        apply_change!(
            service.set_fan_mode(mode, apply),
            |report: &lctrl_core::ChangeReport<FanMode>| {
                ManagedRollback::Fan(*report.previous())
            }
        );
    }
    if baseline.epp != current.epp
        && let Some(value) = baseline.epp
    {
        let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.epp".into(),
        })?;
        apply_change!(
            service.set_epp(value, apply),
            |report: &lctrl_core::ChangeReport<u8>| ManagedRollback::Epp(*report.previous())
        );
    }
    for (kind, baseline_value, current_value) in [
        (PowerLimitKind::Pl1, baseline.pl1_uw, current.pl1_uw),
        (PowerLimitKind::Pl2, baseline.pl2_uw, current.pl2_uw),
        (PowerLimitKind::Tau, baseline.tau_us, current.tau_us),
    ] {
        if baseline_value != current_value {
            if let Some(value) = baseline_value {
                let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                    feature: format!("snapshot.restore.{}", power_limit_kind_name(kind)),
                })?;
                apply_change!(
                    service.set_power_limit(kind, value, apply),
                    |report: &lctrl_core::ChangeReport<u64>| {
                        ManagedRollback::PowerLimit(kind, *report.previous())
                    }
                );
            }
        }
    }
    if baseline.backlight != current.backlight
        && let Some(backlight) = &baseline.backlight
    {
        let service = services.keyboard.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.backlight".into(),
        })?;
        apply_change!(
            service.set_backlight(
                backlight.level,
                lctrl_core::LightingEffect::from_raw(backlight.effect_raw),
                apply,
            ),
            |report: &lctrl_core::ChangeReport<BacklightState>| {
                ManagedRollback::Backlight(*report.previous())
            }
        );
    }
    if baseline.touchpad != current.touchpad
        && let Some(state) = baseline.touchpad.as_deref()
    {
        let state = parse_saved_device_state(state)?;
        let service = services.touchpad.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.touchpad".into(),
        })?;
        apply_change!(
            service.set_touchpad(state, apply),
            |report: &lctrl_core::ChangeReport<DeviceState>| {
                ManagedRollback::Touchpad(*report.previous())
            }
        );
    }
    if baseline.runtime_camera != current.runtime_camera
        && let Some(state) = baseline.runtime_camera.as_deref()
    {
        let state = parse_saved_device_state(state)?;
        let service = services.privacy.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.runtime_camera".into(),
        })?;
        apply_change!(
            service.set_camera(state, apply),
            |report: &lctrl_core::ChangeReport<DeviceState>| {
                ManagedRollback::Camera(*report.previous())
            }
        );
    }
    if baseline.power_scheme != current.power_scheme
        && let Some(id) = baseline.power_scheme.as_deref()
    {
        let service = services.power.ok_or_else(|| LctrlError::Unsupported {
            feature: "snapshot.restore.power_scheme".into(),
        })?;
        let previous_id = service.active_power_scheme()?.id;
        apply_change!(
            service.apply_power_mutation(PowerMutation::Activate(PowerSchemeId::new(id)?), apply,),
            |_report: &lctrl_core::ChangeReport<PowerMutation>| {
                ManagedRollback::Power(previous_id.clone())
            }
        );
    }
    Ok(changes)
}

fn parse_saved_charge_mode(value: &str) -> lctrl_core::Result<ChargeMode> {
    match value {
        "normal" => Ok(ChargeMode::Normal),
        "conservation" => Ok(ChargeMode::Conservation),
        "rapid" => Ok(ChargeMode::Rapid),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("invalid snapshot charge mode {value:?}"),
        }),
    }
}

fn parse_saved_performance_mode(value: &str) -> lctrl_core::Result<PerformanceMode> {
    match value {
        "balanced" => Ok(PerformanceMode::Balanced),
        "quiet" => Ok(PerformanceMode::Quiet),
        "performance" => Ok(PerformanceMode::Performance),
        "geek" => Ok(PerformanceMode::Geek),
        "silent-high-performance" => Ok(PerformanceMode::SilentHighPerformance),
        "custom" => Ok(PerformanceMode::Custom),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("invalid snapshot performance mode {value:?}"),
        }),
    }
}

fn parse_saved_fan_mode(value: &str) -> lctrl_core::Result<FanMode> {
    match value {
        "standard" => Ok(FanMode::Standard),
        "silent" => Ok(FanMode::Silent),
        "performance" => Ok(FanMode::Performance),
        "custom" => Ok(FanMode::Custom),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("invalid snapshot fan mode {value:?}"),
        }),
    }
}

fn parse_saved_device_state(value: &str) -> lctrl_core::Result<DeviceState> {
    match value {
        "enabled" => Ok(DeviceState::Enabled),
        "disabled" => Ok(DeviceState::Disabled),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("invalid snapshot device state {value:?}"),
        }),
    }
}

fn rollback_managed_changes(
    services: &CommandServices<'_>,
    rollbacks: Vec<ManagedRollback>,
    original: LctrlError,
) -> LctrlError {
    for rollback in rollbacks.into_iter().rev() {
        let result = match rollback {
            ManagedRollback::Battery(mode) => services
                .battery
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.charge_mode".into(),
                })
                .and_then(|service| service.set_charge_mode(mode, ApplyMode::Commit).map(|_| ())),
            ManagedRollback::Performance(mode) => services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.performance_mode".into(),
                })
                .and_then(|service| {
                    service
                        .set_performance_mode(mode, ApplyMode::Commit)
                        .map(|_| ())
                }),
            ManagedRollback::Fan(mode) => services
                .fan
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.fan_mode".into(),
                })
                .and_then(|service| service.set_fan_mode(mode, ApplyMode::Commit).map(|_| ())),
            ManagedRollback::Epp(value) => services
                .tuning
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.epp".into(),
                })
                .and_then(|service| service.set_epp(value, ApplyMode::Commit).map(|_| ())),
            ManagedRollback::PowerLimit(kind, value) => services
                .tuning
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: format!("snapshot.rollback.{}", power_limit_kind_name(kind)),
                })
                .and_then(|service| {
                    service
                        .set_power_limit(kind, value, ApplyMode::Commit)
                        .map(|_| ())
                }),
            ManagedRollback::Backlight(state) => services
                .keyboard
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.backlight".into(),
                })
                .and_then(|service| {
                    service
                        .set_backlight(state.level, state.effect, ApplyMode::Commit)
                        .map(|_| ())
                }),
            ManagedRollback::Touchpad(state) => services
                .touchpad
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.touchpad".into(),
                })
                .and_then(|service| service.set_touchpad(state, ApplyMode::Commit).map(|_| ())),
            ManagedRollback::Camera(state) => services
                .privacy
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.runtime_camera".into(),
                })
                .and_then(|service| service.set_camera(state, ApplyMode::Commit).map(|_| ())),
            ManagedRollback::Power(id) => services
                .power
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "snapshot.rollback.power_scheme".into(),
                })
                .and_then(|service| {
                    service
                        .apply_power_mutation(PowerMutation::Activate(id), ApplyMode::Commit)
                        .map(|_| ())
                }),
        };
        if let Err(rollback) = result {
            return LctrlError::FirmwareRejected {
                detail: format!(
                    "snapshot restore failed ({original}); rollback also failed ({rollback})"
                ),
            };
        }
    }
    original
}

fn apply_tune_plan(
    services: &CommandServices<'_>,
    plan: &lctrl_tune::TunePlan,
) -> lctrl_core::Result<Vec<Value>> {
    let mut changes = Vec::with_capacity(plan.writes.len());
    let mut rollbacks = Vec::with_capacity(plan.writes.len());
    for setting in &plan.writes {
        match apply_tune_setting(services, setting) {
            Ok((change, rollback)) => {
                changes.push(change);
                rollbacks.push(rollback);
            }
            Err(error) => return Err(rollback_tune_changes(services, rollbacks, error)),
        }
    }
    Ok(changes)
}

fn apply_tune_setting(
    services: &CommandServices<'_>,
    setting: &lctrl_tune::TuneSetting,
) -> lctrl_core::Result<(Value, TuneRollback)> {
    match setting {
        lctrl_tune::TuneSetting::EcMode(mode) => {
            let service = services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.mode".into(),
                })?;
            let requested = match mode {
                lctrl_tune::EcMode::Smart => PerformanceMode::Balanced,
                lctrl_tune::EcMode::PowerSave => PerformanceMode::Quiet,
                lctrl_tune::EcMode::Beast => PerformanceMode::Performance,
            };
            let report = service.set_performance_mode(requested, ApplyMode::Commit)?;
            let previous = *report.previous();
            Ok((
                serialize_change(&report)?,
                TuneRollback::Performance(previous),
            ))
        }
        lctrl_tune::TuneSetting::ChargeMode(mode) => {
            let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
                feature: "battery.charge_mode".into(),
            })?;
            let report = service.set_charge_mode(*mode, ApplyMode::Commit)?;
            let previous = *report.previous();
            Ok((serialize_change(&report)?, TuneRollback::Battery(previous)))
        }
        lctrl_tune::TuneSetting::Backlight(level) => {
            let service = services.keyboard.ok_or_else(|| LctrlError::Unsupported {
                feature: "kbd.backlight".into(),
            })?;
            let report = service.set_backlight(
                *level,
                lctrl_core::LightingEffect::Static,
                ApplyMode::Commit,
            )?;
            let previous = *report.previous();
            Ok((
                serialize_change(&report)?,
                TuneRollback::Backlight(previous),
            ))
        }
        lctrl_tune::TuneSetting::FanMode(mode) => {
            let service = services.fan.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.fan.mode".into(),
            })?;
            let requested = match mode {
                lctrl_tune::FanMode::Auto | lctrl_tune::FanMode::Smart => FanMode::Standard,
                lctrl_tune::FanMode::Fullspeed | lctrl_tune::FanMode::Performance => {
                    FanMode::Performance
                }
                lctrl_tune::FanMode::Manual | lctrl_tune::FanMode::Off => {
                    return Err(LctrlError::Unsupported {
                        feature: format!("perf.fan.mode.{mode}"),
                    });
                }
            };
            let report = service.set_fan_mode(requested, ApplyMode::Commit)?;
            let previous = *report.previous();
            Ok((serialize_change(&report)?, TuneRollback::Fan(previous)))
        }
        lctrl_tune::TuneSetting::Epp(value) => {
            let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                feature: "tune.epp".into(),
            })?;
            let requested = value.raw().ok_or_else(|| LctrlError::Unsupported {
                feature: "tune.epp.preset".into(),
            })?;
            let report = service.set_epp(requested, ApplyMode::Commit)?;
            let previous = *report.previous();
            Ok((serialize_change(&report)?, TuneRollback::Epp(previous)))
        }
        lctrl_tune::TuneSetting::Pl1(value) => {
            apply_tune_limit(services, PowerLimitKind::Pl1, u64::from(*value) * 1_000_000)
        }
        lctrl_tune::TuneSetting::Pl2(value) => {
            apply_tune_limit(services, PowerLimitKind::Pl2, u64::from(*value) * 1_000_000)
        }
        lctrl_tune::TuneSetting::Tau(value) => {
            apply_tune_limit(services, PowerLimitKind::Tau, u64::from(*value) * 1_000_000)
        }
        unsupported => Err(LctrlError::Unsupported {
            feature: format!("tune.{}", unsupported.target()),
        }),
    }
}

fn apply_tune_limit(
    services: &CommandServices<'_>,
    kind: PowerLimitKind,
    value: u64,
) -> lctrl_core::Result<(Value, TuneRollback)> {
    let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
        feature: format!("tune.{}", power_limit_kind_name(kind)),
    })?;
    let report = service.set_power_limit(kind, value, ApplyMode::Commit)?;
    let previous = *report.previous();
    Ok((
        serialize_change(&report)?,
        TuneRollback::PowerLimit(kind, previous),
    ))
}

fn serialize_change<T: Serialize>(change: &T) -> lctrl_core::Result<Value> {
    serde_json::to_value(change).map_err(|error| LctrlError::Io(std::io::Error::other(error)))
}

fn rollback_tune_changes(
    services: &CommandServices<'_>,
    rollbacks: Vec<TuneRollback>,
    original: LctrlError,
) -> LctrlError {
    for rollback in rollbacks.into_iter().rev() {
        let result = match rollback {
            TuneRollback::Battery(mode) => services
                .battery
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "battery.charge_mode.rollback".into(),
                })
                .and_then(|service| service.set_charge_mode(mode, ApplyMode::Commit).map(|_| ())),
            TuneRollback::Backlight(state) => services
                .keyboard
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "kbd.backlight.rollback".into(),
                })
                .and_then(|service| {
                    service
                        .set_backlight(state.level, state.effect, ApplyMode::Commit)
                        .map(|_| ())
                }),
            TuneRollback::Fan(mode) => services
                .fan
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.fan.mode.rollback".into(),
                })
                .and_then(|service| service.set_fan_mode(mode, ApplyMode::Commit).map(|_| ())),
            TuneRollback::Performance(mode) => services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.mode.rollback".into(),
                })
                .and_then(|service| {
                    service
                        .set_performance_mode(mode, ApplyMode::Commit)
                        .map(|_| ())
                }),
            TuneRollback::Epp(value) => services
                .tuning
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "tune.epp.rollback".into(),
                })
                .and_then(|service| service.set_epp(value, ApplyMode::Commit).map(|_| ())),
            TuneRollback::PowerLimit(kind, value) => services
                .tuning
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: format!("tune.{}.rollback", power_limit_kind_name(kind)),
                })
                .and_then(|service| {
                    service
                        .set_power_limit(kind, value, ApplyMode::Commit)
                        .map(|_| ())
                }),
        };
        if let Err(rollback) = result {
            return LctrlError::FirmwareRejected {
                detail: format!(
                    "tuning profile failed ({original}); restoring a prior setting also failed ({rollback})"
                ),
            };
        }
    }
    original
}

fn execute_update(update: Option<&dyn UpdateControl>, command: UpdateCommand) -> CommandResult {
    let update = update.ok_or_else(|| LctrlError::Unsupported {
        feature: "update".into(),
    })?;
    match command {
        UpdateCommand::Check { .. } => match update.update_capability()? {
            capability @ UpdateCapability::TrustedLocalInfOnly => {
                structured_output(&capability, "update capability checked\n".into())
            }
            UpdateCapability::Unavailable { reason } => Err(LctrlError::ChannelUnavailable {
                channel: format!("update catalog: {reason}"),
            }),
        },
        UpdateCommand::Download { .. } => Err(LctrlError::Unsupported {
            feature: "update.download.without-authenticated-manifest".into(),
        }),
        UpdateCommand::Install { .. } => Err(LctrlError::Unsupported {
            feature: "update.install.without-authenticated-manifest".into(),
        }),
        UpdateCommand::History => Err(LctrlError::Unsupported {
            feature: "update.history.without-catalog".into(),
        }),
        UpdateCommand::Ignore { .. } => Err(LctrlError::Unsupported {
            feature: "update.ignore.without-catalog".into(),
        }),
        UpdateCommand::Rollback { .. } => Err(LctrlError::Unsupported {
            feature: "update.rollback.without-install-ledger".into(),
        }),
        UpdateCommand::Schedule { .. } => Err(LctrlError::Unsupported {
            feature: "update.schedule.without-catalog".into(),
        }),
    }
}

fn execute_scan(
    diagnostics: Option<&dyn DiagnosticsControl>,
    command: ScanCommand,
) -> CommandResult {
    let diagnostics = diagnostics.ok_or_else(|| LctrlError::Unsupported {
        feature: "scan".into(),
    })?;
    match command {
        ScanCommand::List => {
            let items = diagnostics.diagnostic_items()?;
            structured_output(&items, format!("{} diagnostic item(s)\n", items.len()))
        }
        ScanCommand::Run { items } => {
            let items = if items.is_empty() {
                diagnostics.diagnostic_items()?
            } else {
                items
                    .iter()
                    .map(|item| parse_diagnostic_kind(item))
                    .collect::<lctrl_core::Result<Vec<_>>>()?
            };
            let results = diagnostics.run_diagnostics(&items)?;
            structured_output(
                &results,
                format!("{} diagnostic result(s)\n", results.len()),
            )
        }
    }
}

fn parse_diagnostic_kind(value: &str) -> lctrl_core::Result<DiagnosticKind> {
    match value.to_ascii_lowercase().as_str() {
        "battery" => Ok(DiagnosticKind::Battery),
        "thermal" => Ok(DiagnosticKind::Thermal),
        "storage" => Ok(DiagnosticKind::Storage),
        "memory" => Ok(DiagnosticKind::Memory),
        "firmware" => Ok(DiagnosticKind::Firmware),
        "network" | "wifi" => Ok(DiagnosticKind::Network),
        _ => Err(LctrlError::InvalidArgument {
            detail: format!("unknown diagnostic item {value:?}"),
        }),
    }
}

fn execute_backlight(
    keyboard: Option<&dyn KeyboardControl>,
    level: u8,
    effect: Option<BacklightEffect>,
    apply: ApplyMode,
) -> CommandResult {
    let keyboard = keyboard.ok_or_else(|| LctrlError::Unsupported {
        feature: "kbd.backlight".into(),
    })?;
    let effect = match effect.unwrap_or(BacklightEffect::Static) {
        BacklightEffect::Static => lctrl_core::LightingEffect::Static,
        BacklightEffect::Breath => lctrl_core::LightingEffect::Breathing,
    };
    let report = keyboard.set_backlight(level, effect, apply)?;
    structured_output(
        &report,
        format!(
            "keyboard backlight: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
    )
}
fn execute_touchpad(
    touchpad: Option<&dyn TouchpadControl>,
    state: Toggle,
    apply: ApplyMode,
) -> CommandResult {
    let touchpad = touchpad.ok_or_else(|| LctrlError::Unsupported {
        feature: "touchpad".into(),
    })?;
    let requested = match state {
        Toggle::On => DeviceState::Enabled,
        Toggle::Off => DeviceState::Disabled,
    };
    let report = touchpad.set_touchpad(requested, apply)?;
    structured_output(
        &report,
        format!(
            "touchpad: requested={:?} actual={} (previous={:?})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), |value| format!("{value:?}")),
            report.previous(),
        ),
    )
}

fn execute_fnlock(
    bios: Option<&dyn BiosControl>,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
) -> CommandResult {
    let bios = bios.ok_or_else(|| LctrlError::Unsupported {
        feature: "kbd.fnlock".into(),
    })?;
    let requested_values = match state {
        Toggle::On => [
            ("HotkeyMode", "Disable"),
            ("F1-F12AsPrimaryFunction", "Enable"),
        ],
        Toggle::Off => [
            ("HotkeyMode", "Enable"),
            ("F1-F12AsPrimaryFunction", "Disable"),
        ],
    };
    let mut previous = Vec::with_capacity(requested_values.len());
    let mut requested = Vec::with_capacity(requested_values.len());
    for (name, value) in requested_values {
        let current = bios.get(name)?.ok_or_else(|| LctrlError::Unsupported {
            feature: format!("bios.setting.{name}"),
        })?;
        validate_bios_selection(bios, name, value)?;
        previous.push(BiosChange::new(current.name, current.value)?);
        requested.push(BiosChange::new(name, value)?);
    }
    if apply == ApplyMode::DryRun {
        let reports = previous
            .into_iter()
            .zip(requested)
            .map(|(previous, requested)| lctrl_core::ChangeReport::dry_run(previous, requested))
            .collect::<Vec<_>>();
        return structured_output(&reports, "FnLock BIOS dry run validated\n".into());
    }
    confirm_risky_change(
        confirmed,
        interactive,
        &format!(
            "BIOS writes: HotkeyMode={} and F1-F12AsPrimaryFunction={}; effect after reboot; recovery: restore HotkeyMode={} and F1-F12AsPrimaryFunction={} with vantage bios set --save --yes",
            requested[0].value, requested[1].value, previous[0].value, previous[1].value,
        ),
    )?;
    stage_and_save_many(bios, &requested)?;
    let previous_for_recovery = previous.clone();
    let mut reports = Vec::with_capacity(requested.len());
    for ((previous, requested), (name, _)) in
        previous.into_iter().zip(requested).zip(requested_values)
    {
        let actual_item = match bios.get(name) {
            Ok(Some(item)) => item,
            Ok(None) => {
                return Err(recover_bios_changes(
                    bios,
                    &previous_for_recovery,
                    LctrlError::VerifyMismatch {
                        requested: requested.value.to_string(),
                        actual: "setting absent after save".into(),
                    },
                ));
            }
            Err(error) => return Err(recover_bios_changes(bios, &previous_for_recovery, error)),
        };
        let actual = match BiosChange::new(actual_item.name, actual_item.value) {
            Ok(actual) => actual,
            Err(error) => return Err(recover_bios_changes(bios, &previous_for_recovery, error)),
        };
        if actual.value != requested.value {
            return Err(recover_bios_changes(
                bios,
                &previous_for_recovery,
                LctrlError::VerifyMismatch {
                    requested: requested.value.to_string(),
                    actual: actual.value.to_string(),
                },
            ));
        }
        reports.push(lctrl_core::ChangeReport::committed(
            previous, requested, actual,
        ));
    }
    structured_output(
        &reports,
        "FnLock BIOS settings saved and read back\n".into(),
    )
}

fn execute_fn_ctrl_swap(
    bios: Option<&dyn BiosControl>,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
) -> CommandResult {
    execute_persistent_bios_toggle(
        bios,
        "FoolProofFnCtrl",
        state,
        apply,
        confirmed,
        interactive,
    )
}

fn execute_privacy(
    privacy: Option<&dyn PrivacyControl>,
    bios: Option<&dyn BiosControl>,
    command: PrivacyCommand,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
) -> CommandResult {
    match command {
        PrivacyCommand::Cam {
            state,
            runtime,
            persistent,
        } => match privacy_layer(runtime, persistent)? {
            PrivacyLayer::Runtime => execute_runtime_camera(privacy, state, apply),
            PrivacyLayer::Persistent => execute_persistent_bios_toggle(
                bios,
                "IntegratedCamera",
                state,
                apply,
                confirmed,
                interactive,
            ),
        },
        PrivacyCommand::Mic {
            state,
            runtime,
            persistent,
        } => match privacy_layer(runtime, persistent)? {
            PrivacyLayer::Runtime => Err(LctrlError::Unsupported {
                feature: "privacy.mic.runtime".into(),
            }),
            PrivacyLayer::Persistent => execute_persistent_bios_toggle(
                bios,
                "Microphone",
                state,
                apply,
                confirmed,
                interactive,
            ),
        },
        PrivacyCommand::Fingerprint { state } => execute_persistent_bios_toggle(
            bios,
            "FingerprintReader",
            state,
            apply,
            confirmed,
            interactive,
        ),
        PrivacyCommand::Status => {
            if privacy.is_none() && bios.is_none() {
                return Err(LctrlError::Unsupported {
                    feature: "privacy.status".into(),
                });
            }
            let (runtime_camera, runtime_camera_error) = privacy.map_or((None, None), |privacy| {
                capture_optional(privacy.camera_state())
            });
            let mut bios_items = Vec::new();
            if let Some(bios) = bios {
                for name in ["IntegratedCamera", "Microphone", "FingerprintReader"] {
                    if let Some(item) = bios.get(name)? {
                        bios_items.push(item);
                    }
                }
            }
            let payload = PrivacyStatusPayload {
                runtime_camera,
                runtime_camera_error,
                bios: bios_items,
            };
            structured_output(&payload, "privacy status collected\n".into())
        }
    }
}

#[derive(Clone, Copy)]
enum PrivacyLayer {
    Runtime,
    Persistent,
}

fn privacy_layer(runtime: bool, persistent: bool) -> lctrl_core::Result<PrivacyLayer> {
    match (runtime, persistent) {
        (true, false) => Ok(PrivacyLayer::Runtime),
        (false, true) => Ok(PrivacyLayer::Persistent),
        _ => Err(LctrlError::InvalidArgument {
            detail: "privacy camera/microphone requires exactly one of --persistent or --runtime"
                .into(),
        }),
    }
}

fn execute_runtime_camera(
    privacy: Option<&dyn PrivacyControl>,
    state: Toggle,
    apply: ApplyMode,
) -> CommandResult {
    let privacy = privacy.ok_or_else(|| LctrlError::Unsupported {
        feature: "privacy.cam.runtime".into(),
    })?;
    let requested = match state {
        Toggle::On => DeviceState::Enabled,
        Toggle::Off => DeviceState::Disabled,
    };
    let report = privacy.set_camera(requested, apply)?;
    structured_output(&report, "runtime camera state requested\n".into())
}

fn execute_persistent_bios_toggle(
    bios: Option<&dyn BiosControl>,
    name: &str,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
) -> CommandResult {
    let bios = bios.ok_or_else(|| LctrlError::Unsupported {
        feature: format!("bios.{name}"),
    })?;
    let current = bios.get(name)?.ok_or_else(|| LctrlError::Unsupported {
        feature: format!("bios.setting.{name}"),
    })?;
    let requested_value = match state {
        Toggle::On => "Enable",
        Toggle::Off => "Disable",
    };
    validate_bios_selection(bios, name, requested_value)?;
    let previous = BiosChange::new(current.name, current.value)?;
    let requested = BiosChange::new(name, requested_value)?;
    if apply == ApplyMode::DryRun {
        return structured_output(
            &lctrl_core::ChangeReport::dry_run(previous, requested),
            format!("BIOS {name} dry run validated\n"),
        );
    }
    confirm_risky_change(
        confirmed,
        interactive,
        &format!(
            "BIOS write: key={name} value={requested_value}; effect timing may require reboot; recovery: vantage bios set {name} {} --save --yes",
            previous.value
        ),
    )?;
    stage_and_save(bios, requested.clone())?;
    let actual_item = match bios.get(name) {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Err(recover_bios_changes(
                bios,
                std::slice::from_ref(&previous),
                LctrlError::VerifyMismatch {
                    requested: requested_value.into(),
                    actual: "setting absent after save".into(),
                },
            ));
        }
        Err(error) => {
            return Err(recover_bios_changes(
                bios,
                std::slice::from_ref(&previous),
                error,
            ));
        }
    };
    let actual = match BiosChange::new(actual_item.name, actual_item.value) {
        Ok(actual) => actual,
        Err(error) => {
            return Err(recover_bios_changes(
                bios,
                std::slice::from_ref(&previous),
                error,
            ));
        }
    };
    if actual.value != requested.value {
        return Err(recover_bios_changes(
            bios,
            std::slice::from_ref(&previous),
            LctrlError::VerifyMismatch {
                requested: requested.value.to_string(),
                actual: actual.value.to_string(),
            },
        ));
    }
    structured_output(
        &lctrl_core::ChangeReport::committed(previous, requested, actual),
        format!("BIOS {name} saved and read back\n"),
    )
}

fn validate_bios_selection(
    bios: &dyn BiosControl,
    name: &str,
    value: &str,
) -> lctrl_core::Result<()> {
    let selections = bios.selections(name)?;
    if selections.is_empty() {
        return Err(LctrlError::Unsupported {
            feature: format!("bios.selections.{name}"),
        });
    }
    if selections.iter().any(|selection| selection == value) {
        Ok(())
    } else {
        Err(LctrlError::InvalidArgument {
            detail: format!(
                "BIOS value {value:?} is not one of the exact selections for {name}: {}",
                selections.join(", ")
            ),
        })
    }
}

fn confirm_risky_change(
    confirmed: bool,
    interactive: bool,
    impact: &str,
) -> lctrl_core::Result<()> {
    if confirmed {
        if interactive {
            eprintln!("{impact}");
        }
        return Ok(());
    }
    let stdin = io::stdin();
    if !interactive || !stdin.is_terminal() {
        return Err(LctrlError::InvalidArgument {
            detail: "risky BIOS writes require interactive confirmation or --yes".into(),
        });
    }
    eprintln!("{impact}");
    eprint!("Proceed with this BIOS write? [y/N] ");
    io::stderr().flush().map_err(LctrlError::Io)?;
    let mut response = String::new();
    stdin.read_line(&mut response).map_err(LctrlError::Io)?;
    if matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(LctrlError::InvalidArgument {
            detail: "BIOS write was not confirmed".into(),
        })
    }
}

fn recover_bios_changes(
    bios: &dyn BiosControl,
    previous: &[BiosChange],
    original: LctrlError,
) -> LctrlError {
    if let Err(recovery) = stage_and_save_many(bios, previous) {
        return LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS readback failed ({original}); restoring the previous setting also failed ({recovery})"
            ),
        };
    }
    for expected in previous {
        match bios.get(expected.name.as_str()) {
            Ok(Some(actual)) if actual.value == expected.value.as_str() => {}
            Ok(Some(actual)) => {
                return LctrlError::FirmwareRejected {
                    detail: format!(
                        "BIOS readback failed ({original}); rollback for {} read back {:?}, expected {:?}",
                        expected.name, actual.value, expected.value
                    ),
                };
            }
            Ok(None) => {
                return LctrlError::FirmwareRejected {
                    detail: format!(
                        "BIOS readback failed ({original}); rollback omitted {}",
                        expected.name
                    ),
                };
            }
            Err(recovery) => {
                return LctrlError::FirmwareRejected {
                    detail: format!(
                        "BIOS readback failed ({original}); rollback read failed ({recovery})"
                    ),
                };
            }
        }
    }
    original
}

fn stage_and_save(bios: &dyn BiosControl, change: BiosChange) -> lctrl_core::Result<()> {
    stage_and_save_many(bios, std::slice::from_ref(&change))
}

fn stage_and_save_many(bios: &dyn BiosControl, changes: &[BiosChange]) -> lctrl_core::Result<()> {
    for change in changes {
        if let Err(error) = bios.stage(change.clone()) {
            return discard_after_bios_failure(bios, error);
        }
    }
    if let Err(error) = bios.save() {
        return discard_after_bios_failure(bios, error);
    }
    Ok(())
}

fn discard_after_bios_failure(bios: &dyn BiosControl, error: LctrlError) -> lctrl_core::Result<()> {
    match bios.discard() {
        Ok(()) => Err(error),
        Err(discard) => Err(LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS transaction failed ({error}); discarding the staged change also failed ({discard})"
            ),
        }),
    }
}

fn execute_bios_list(bios: Option<&dyn BiosControl>) -> CommandResult {
    let bios = bios.ok_or_else(|| LctrlError::Unsupported {
        feature: "bios".into(),
    })?;
    let items = bios.list()?;
    structured_output(&items, format!("{} BIOS setting(s)\n", items.len()))
}

fn execute_bios(
    bios: Option<&dyn BiosControl>,
    command: BiosCommand,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
) -> CommandResult {
    let bios = bios.ok_or_else(|| LctrlError::Unsupported {
        feature: "bios".into(),
    })?;
    match command {
        BiosCommand::List => {
            let items = bios.list()?;
            structured_output(&items, format!("{} BIOS setting(s)\n", items.len()))
        }
        BiosCommand::Get { name } => {
            let item = bios
                .get(&name)?
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("BIOS setting {name:?} was not found"),
                })?;
            structured_output(&item, format!("{}={}\n", item.name, item.value))
        }
        BiosCommand::Set { name, value, save } => {
            if apply == ApplyMode::Commit && !save {
                return Err(LctrlError::InvalidArgument {
                    detail: "stateless BIOS writes require --save; staged-only changes cannot be committed later"
                        .into(),
                });
            }
            let current = bios
                .get(&name)?
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("BIOS setting {name:?} was not found"),
                })?;
            validate_bios_selection(bios, &name, &value)?;
            let previous = BiosChange::new(current.name, current.value)?;
            let requested = BiosChange::new(name.clone(), value.clone())?;
            if apply == ApplyMode::DryRun {
                return structured_output(
                    &lctrl_core::ChangeReport::dry_run(previous, requested),
                    "BIOS setting dry run validated\n".into(),
                );
            }
            confirm_risky_change(
                confirmed,
                interactive,
                &format!(
                    "BIOS write: key={name} value={value}; effect timing may require reboot; recovery: vantage bios set {name} {} --save --yes",
                    previous.value
                ),
            )?;
            stage_and_save(bios, requested.clone())?;
            let actual_item = match bios.get(&name) {
                Ok(Some(item)) => item,
                Ok(None) => {
                    return Err(recover_bios_changes(
                        bios,
                        std::slice::from_ref(&previous),
                        LctrlError::VerifyMismatch {
                            requested: value.clone(),
                            actual: "setting absent after save".into(),
                        },
                    ));
                }
                Err(error) => {
                    return Err(recover_bios_changes(
                        bios,
                        std::slice::from_ref(&previous),
                        error,
                    ));
                }
            };
            let actual = match BiosChange::new(actual_item.name, actual_item.value) {
                Ok(actual) => actual,
                Err(error) => {
                    return Err(recover_bios_changes(
                        bios,
                        std::slice::from_ref(&previous),
                        error,
                    ));
                }
            };
            if actual.value != requested.value {
                return Err(recover_bios_changes(
                    bios,
                    std::slice::from_ref(&previous),
                    LctrlError::VerifyMismatch {
                        requested: requested.value.to_string(),
                        actual: actual.value.to_string(),
                    },
                ));
            }
            structured_output(
                &lctrl_core::ChangeReport::committed(previous, requested, actual),
                "BIOS setting saved and read back\n".into(),
            )
        }
        BiosCommand::Save => Err(LctrlError::Unsupported {
            feature: "bios.save.global-buffer".into(),
        }),
        BiosCommand::Discard => {
            if apply == ApplyMode::DryRun {
                structured_output(
                    &serde_json::json!({ "operation": "bios_discard", "mode": "dry_run" }),
                    "BIOS discard dry run validated; no staged changes discarded\n".into(),
                )
            } else {
                bios.discard()?;
                structured_output(&(), "BIOS staged changes discarded\n".into())
            }
        }
        BiosCommand::Password {
            command: Some(PasswordCommand::Status),
        } => {
            let status = bios.password_status()?;
            structured_output(&status, "BIOS password status read\n".into())
        }
        BiosCommand::Defaults | BiosCommand::Password { .. } => Err(LctrlError::Unsupported {
            feature: "bios.experimental-or-underspecified".into(),
        }),
    }
}

fn structured_output<T: Serialize>(value: &T, human: String) -> CommandResult {
    let json = serde_json::to_value(value)
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
    Ok(CommandOutput { human, json })
}
fn execute_info(hal: &dyn Hal) -> CommandResult {
    let hardware = hal.hardware_info()?;
    let capabilities = hal.capabilities()?;
    let platform = hal.platform();
    let human = format_info(&platform, &hardware, &capabilities);
    let payload = InfoPayload {
        platform,
        hardware,
        features: capabilities.features,
    };
    let json = serde_json::to_value(payload)
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;

    Ok(CommandOutput { human, json })
}

fn format_info(
    platform: &Platform,
    hardware: &HardwareInfo,
    capabilities: &CapabilitySet,
) -> String {
    let mut output = format!("platform: {}\n", platform_label(*platform));
    output.push_str(&format!(
        "product: {}\n",
        hardware.product_name.as_deref().unwrap_or("unknown")
    ));
    output.push_str(&format!(
        "family: {}\n",
        hardware.family.as_deref().unwrap_or("unknown")
    ));
    output.push_str(&format!(
        "bios: {}\n",
        hardware.bios_version.as_deref().unwrap_or("unknown")
    ));
    output.push_str("features:\n");
    for (feature, capability) in &capabilities.features {
        output.push_str("  ");
        output.push_str(feature);
        output.push_str(": ");
        output.push_str(availability_label(capability.availability));
        if let Some(detail) = &capability.detail {
            output.push_str(" ( ");
            output.push_str(detail);
            output.push_str(" )");
        }
        output.push('\n');
    }
    output
}

fn platform_label(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

fn availability_label(availability: Availability) -> &'static str {
    match availability {
        Availability::Available => "available",
        Availability::Limited => "limited",
        Availability::Unavailable => "unavailable",
    }
}

fn unsupported(command: Command) -> LctrlError {
    let feature = command_feature(&command);
    LctrlError::Unsupported {
        feature: format!("{feature}: {}", unsupported_reason(&command)),
    }
}

fn unsupported_reason(command: &Command) -> &'static str {
    match command {
        Command::Battery { command: None } => "a battery subcommand is required",
        Command::Battery {
            command: Some(BatteryCommand::Thresholds { .. }),
        } => "the target has no verified arbitrary-threshold writer",
        Command::Battery {
            command: Some(BatteryCommand::ExtremeLife { .. }),
        } => "no verified extreme-life channel is attached",
        Command::Battery {
            command: Some(BatteryCommand::NightCharge { .. }),
        } => "no verified night-charge channel is attached",
        Command::Battery {
            command: Some(BatteryCommand::TemporaryMode),
        } => "temporary-mode is read-only and no verified reader is attached",
        Command::Battery {
            command: Some(BatteryCommand::Watch),
        } => "battery event streaming requires the daemon event source",
        Command::Usb { .. } => "no verified USB charging control service is attached",
        Command::Power {
            command: Some(PowerCommand::SaverOnce),
        } => "composite saver workflow has no transactional executor",
        Command::Power { command: None } => "a power subcommand is required",
        Command::Perf { command: None } => "a performance subcommand is required",
        Command::Perf {
            command: Some(PerfCommand::Fan { command: None }),
        } => "a fan subcommand is required",
        Command::Perf {
            command: Some(PerfCommand::Temp { command: None }),
        } => "a temperature subcommand is required",
        Command::Perf {
            command: Some(PerfCommand::Top),
        } => "process telemetry is not a hardware control channel",
        Command::Perf {
            command: Some(PerfCommand::Boost { .. } | PerfCommand::Throttle { .. }),
        } => "process policy control is not attached",
        Command::Tune { command: None } => "a tuning subcommand is required",
        Command::Tune {
            command: Some(TuneCommand::Profile { command: None }),
        } => "a profile subcommand is required",
        Command::Tune {
            command: Some(TuneCommand::Turbo { .. }),
        } => "no verified turbo mutator is attached",
        Command::Tune {
            command: Some(TuneCommand::Watch),
        } => "tuning event streaming requires the daemon event source",
        Command::Kbd { command: None } => "a keyboard subcommand is required",
        Command::Kbd {
            command: Some(KbdCommand::Winlock { .. }),
        } => "GAMEZONE Win-key methods are not verified on the target",
        Command::Panel { command: None } => "a panel subcommand is required",
        Command::Sense { .. } => "SmartSense requires an unavailable vendor RPC channel",
        Command::Audio { .. } => "audio control requires an unavailable licensed/vendor channel",
        Command::Magicbay {
            command: Some(MagicbayCommand::Detect),
        } => "MagicBay detection service is unavailable",
        Command::Magicbay { .. } => "standard MBIM/UVC/DRM control is not attached",
        Command::Osd { .. } => "OSD is daemon-only and no OSD backend is attached",
        Command::Daemon { .. } => "daemon commands are handled by the platform binary",
        Command::Snapshot { command: None } => "a snapshot subcommand is required",
        _ => "no verified implementation is attached on this platform",
    }
}

fn command_feature(command: &Command) -> String {
    match command {
        Command::Info => "info".into(),
        Command::Doctor => "doctor".into(),
        Command::Battery { command } => {
            prefixed("battery", command.as_ref().map(BatteryCommand::feature))
        }
        Command::Usb { command } => prefixed("usb", command.as_ref().map(UsbCommand::feature)),
        Command::Power { command } => {
            prefixed("power", command.as_ref().map(PowerCommand::feature))
        }
        Command::Perf { command } => prefixed("perf", command.as_ref().map(PerfCommand::feature)),
        Command::Tune { command } => prefixed("tune", command.as_ref().map(TuneCommand::feature)),
        Command::Kbd { command } => prefixed("kbd", command.as_ref().map(KbdCommand::feature)),
        Command::Touchpad { .. } => "touchpad".into(),
        Command::Panel { command } => {
            prefixed("panel", command.as_ref().map(PanelCommand::feature))
        }
        Command::Privacy { command } => {
            prefixed("privacy", command.as_ref().map(PrivacyCommand::feature))
        }
        Command::Sense { command } => {
            prefixed("sense", command.as_ref().map(SenseCommand::feature))
        }
        Command::Audio { command } => {
            prefixed("audio", command.as_ref().map(AudioCommand::feature))
        }
        Command::Bios { command } => prefixed("bios", command.as_ref().map(BiosCommand::feature)),
        Command::Magicbay { command } => {
            prefixed("magicbay", command.as_ref().map(MagicbayCommand::feature))
        }
        Command::Update { command } => {
            prefixed("update", command.as_ref().map(UpdateCommand::feature))
        }
        Command::Scan { command } => prefixed("scan", command.as_ref().map(ScanCommand::feature)),
        Command::Snapshot { command } => {
            prefixed("snapshot", command.as_ref().map(SnapshotCommand::feature))
        }
        Command::Osd { command } => prefixed("osd", command.as_ref().map(OsdCommand::feature)),
        Command::Daemon { command } => {
            prefixed("daemon", command.as_ref().map(DaemonCommand::feature))
        }
        Command::Completions { .. } => "completions".into(),
    }
}

fn prefixed(prefix: &str, suffix: Option<&'static str>) -> String {
    match suffix {
        Some(suffix) => format!("{prefix}.{suffix}"),
        None => prefix.into(),
    }
}

impl BatteryCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Adapter => "adapter",
            Self::ChargeMode { .. } => "charge-mode",
            Self::Thresholds { .. } => "thresholds",
            Self::ExtremeLife { .. } => "extreme-life",
            Self::NightCharge { .. } => "night-charge",
            Self::TemporaryMode => "temporary-mode",
            Self::Watch => "watch",
        }
    }
}

impl UsbCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::AlwaysOn { .. } => "always-on",
            Self::ChargeOnBattery { .. } => "charge-on-battery",
        }
    }
}

impl PowerCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Scheme { .. } => "scheme",
            Self::SaverOnce => "saver-once",
        }
    }
}

impl PerfCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Mode { .. } => "mode",
            Self::Fan { .. } => "fan",
            Self::Temp { .. } => "temp",
            Self::Pl1 { .. } => "pl1",
            Self::Pl2 { .. } => "pl2",
            Self::Top => "top",
            Self::Boost { .. } => "boost",
            Self::Throttle { .. } => "throttle",
        }
    }
}

impl TuneCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Profile { .. } => "profile",
            Self::Pl1 { .. } => "pl1",
            Self::Epp { .. } => "epp",
            Self::Turbo { .. } => "turbo",
            Self::Restore => "restore",
            Self::Telemetry { .. } => "telemetry",
            Self::Watch => "watch",
        }
    }
}

impl KbdCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Backlight { .. } => "backlight",
            Self::Fnlock { .. } => "fnlock",
            Self::FnCtrlSwap { .. } => "fn-ctrl-swap",
            Self::Winlock { .. } => "winlock",
        }
    }
}

impl PanelCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Rate { .. } => "rate",
            Self::Color { .. } => "color",
            Self::SuperResolution { .. } => "super-resolution",
            Self::Overdrive { .. } => "overdrive",
            Self::EyeCare { .. } => "eye-care",
        }
    }
}

impl PrivacyCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Cam { .. } => "cam",
            Self::Mic { .. } => "mic",
            Self::Fingerprint { .. } => "fingerprint",
            Self::Status => "status",
        }
    }
}

impl SenseCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::LockOnLeave { .. } => "lock-on-leave",
            Self::WakeOnApproach { .. } => "wake-on-approach",
            Self::PauseVideo { .. } => "pause-video",
            Self::AttentionTracking { .. } => "attention-tracking",
            Self::KbdLightAuto { .. } => "kbd-light-auto",
        }
    }
}

impl AudioCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Dolby { .. } => "dolby",
            Self::NoiseCancel { .. } => "noise-cancel",
        }
    }
}

impl BiosCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Get { .. } => "get",
            Self::Set { .. } => "set",
            Self::Save { .. } => "save",
            Self::Discard => "discard",
            Self::Defaults => "defaults",
            Self::Password { .. } => "password",
        }
    }
}

impl UpdateCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Check { .. } => "check",
            Self::Download { .. } => "download",
            Self::Install { .. } => "install",
            Self::History => "history",
            Self::Ignore { .. } => "ignore",
            Self::Rollback { .. } => "rollback",
            Self::Schedule { .. } => "schedule",
        }
    }
}

impl ScanCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Run { .. } => "run",
        }
    }
}

impl SnapshotCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Diff => "diff",
            Self::Restore => "restore",
        }
    }
}

impl MagicbayCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Detect => "detect",
            Self::Lte { .. } => "lte",
            Self::Cam => "cam",
            Self::Display => "display",
            Self::Watch => "watch",
        }
    }
}

impl OsdCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Test => "test",
        }
    }
}

impl DaemonCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Status => "status",
            Self::Install => "install",
        }
    }
}
