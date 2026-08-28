//! Command-line surface for `lctrl`.
//!
//! The command tree deliberately lives separately from host backends.  At this
//! stage only `info` has an execution path; every other command is parsed so
//! that the eventual controllers can be added without changing the CLI
//! contract, but returns a structured [`lctrl_core::LctrlError::Unsupported`].

use std::collections::BTreeMap;

use clap::{Parser, Subcommand, ValueEnum};
use lctrl_core::{
    ApplyMode, Availability, BiosChange, Capability, CapabilitySet, ChargeMode, HardwareInfo,
    LctrlError, Platform, PowerGuid, PowerMutation, PowerSchemeId, PowerSettingKey,
    PowerSettingValue, PowerSource,
};
use lctrl_hal::{
    BatteryControl, BiosControl, Hal, KeyboardControl, PerformanceControl, PowerControl,
};
use serde::Serialize;
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
    keyboard: Option<&'a dyn KeyboardControl>,
    performance: Option<&'a dyn PerformanceControl>,
    power: Option<&'a dyn PowerControl>,
}

impl<'a> CommandServices<'a> {
    pub const fn new(hal: &'a dyn Hal) -> Self {
        Self {
            hal,
            battery: None,
            bios: None,
            keyboard: None,
            performance: None,
            power: None,
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
    pub fn with_keyboard(mut self, keyboard: &'a dyn KeyboardControl) -> Self {
        self.keyboard = Some(keyboard);
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

/// Root parser for the `lctrl` command.
#[derive(Clone, Debug, Parser, PartialEq, Eq)]
#[command(name = "lctrl", version, about = "Cross-platform hardware control")]
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

/// Top-level `lctrl` commands.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Report platform, hardware identity, and discovered capabilities.
    Info,
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

/// Execute a parsed CLI command using only the root HAL.
///
/// This compatibility entry point intentionally exposes no optional services.
/// Call [`execute_with_services`] from a concrete platform composition root.
pub fn execute(cli: Cli, hal: &dyn Hal) -> CommandResult {
    execute_with_services(cli, CommandServices::new(hal))
}

/// Execute a parsed CLI command without terminating the process.
pub fn execute_with_services(cli: Cli, services: CommandServices<'_>) -> CommandResult {
    let apply = if cli.dry_run {
        ApplyMode::DryRun
    } else {
        ApplyMode::Commit
    };
    let confirmed = cli.yes;
    match cli.command {
        Command::Info => execute_info(services.hal),
        Command::Battery {
            command: Some(BatteryCommand::Status),
        } => execute_battery_status(services.battery),
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
        Command::Perf {
            command: Some(PerfCommand::Mode { mode }),
        } => execute_performance_mode(services.performance, mode, apply),
        Command::Bios {
            command: Some(command),
        } => execute_bios(services.bios, command, apply, confirmed),
        Command::Kbd {
            command: Some(KbdCommand::Backlight { level, effect }),
        } => execute_backlight(services.keyboard, level, effect, apply),
        Command::Kbd {
            command: Some(KbdCommand::FnCtrlSwap { state }),
        } => execute_fn_ctrl_swap(services.bios, state, apply, confirmed),
        Command::Privacy {
            command: Some(command),
        } => execute_privacy(services.bios, command, apply, confirmed),
        command => Err(unsupported(command)),
    }
}

fn execute_battery_status(battery: Option<&dyn BatteryControl>) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.status".into(),
    })?;
    let telemetry = battery.battery_telemetry(0)?;
    structured_output(
        &telemetry,
        format!(
            "battery: {} mWh remaining\ncharge: {}%\n",
            telemetry
                .remaining_capacity_mwh
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            telemetry
                .remaining_percent
                .map_or_else(|| "unknown".into(), |value| value.to_string())
        ),
    )
}

fn execute_battery_adapter(battery: Option<&dyn BatteryControl>) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.adapter".into(),
    })?;
    let adapter = battery.adapter_info()?;
    structured_output(
        &adapter,
        format!(
            "adapter: {}\ndetail: {}\nunderpowered: {}\n",
            adapter.authentication,
            if adapter.has_detail {
                "available"
            } else {
                "unavailable"
            },
            adapter.is_underpowered()
        ),
    )
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
            "charge mode: {} -> {}\n",
            report.previous(),
            report.requested()
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
            "performance mode: {} -> {}\n",
            report.previous(),
            report.requested()
        ),
    )
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
        format!("keyboard backlight level {level} ({effect})\n"),
    )
}

fn execute_fn_ctrl_swap(
    bios: Option<&dyn BiosControl>,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
) -> CommandResult {
    execute_persistent_bios_toggle(bios, "FoolProofFnCtrl", state, apply, confirmed)
}

fn execute_privacy(
    bios: Option<&dyn BiosControl>,
    command: PrivacyCommand,
    apply: ApplyMode,
    confirmed: bool,
) -> CommandResult {
    match command {
        PrivacyCommand::Cam {
            state,
            runtime,
            persistent,
        } => {
            require_persistent_privacy_layer(runtime, persistent, "privacy.cam.runtime")?;
            execute_persistent_bios_toggle(bios, "IntegratedCamera", state, apply, confirmed)
        }
        PrivacyCommand::Mic {
            state,
            runtime,
            persistent,
        } => {
            require_persistent_privacy_layer(runtime, persistent, "privacy.mic.runtime")?;
            execute_persistent_bios_toggle(bios, "Microphone", state, apply, confirmed)
        }
        PrivacyCommand::Fingerprint { state } => {
            execute_persistent_bios_toggle(bios, "FingerprintReader", state, apply, confirmed)
        }
        PrivacyCommand::Status => {
            let bios = bios.ok_or_else(|| LctrlError::Unsupported {
                feature: "privacy.status".into(),
            })?;
            let mut items = Vec::new();
            for name in ["IntegratedCamera", "Microphone", "FingerprintReader"] {
                if let Some(item) = bios.get(name)? {
                    items.push(item);
                }
            }
            structured_output(&items, format!("{} privacy BIOS state(s)\n", items.len()))
        }
    }
}

fn require_persistent_privacy_layer(
    runtime: bool,
    persistent: bool,
    runtime_feature: &str,
) -> lctrl_core::Result<()> {
    if runtime {
        return Err(LctrlError::Unsupported {
            feature: runtime_feature.into(),
        });
    }
    if !persistent {
        return Err(LctrlError::InvalidArgument {
            detail: "privacy camera/microphone requires explicit --persistent or --runtime".into(),
        });
    }
    Ok(())
}

fn execute_persistent_bios_toggle(
    bios: Option<&dyn BiosControl>,
    name: &str,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
) -> CommandResult {
    if apply == ApplyMode::Commit && !confirmed {
        return Err(LctrlError::InvalidArgument {
            detail: format!("persistent BIOS change {name} requires --yes"),
        });
    }
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
    bios.stage(requested.clone())?;
    bios.save()?;
    let actual_item = bios.get(name)?.ok_or_else(|| LctrlError::VerifyMismatch {
        requested: requested_value.into(),
        actual: "setting absent after save".into(),
    })?;
    let actual = BiosChange::new(actual_item.name, actual_item.value)?;
    if actual.value != requested.value {
        return Err(LctrlError::VerifyMismatch {
            requested: requested.value.to_string(),
            actual: actual.value.to_string(),
        });
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

fn execute_bios(
    bios: Option<&dyn BiosControl>,
    command: BiosCommand,
    apply: ApplyMode,
    confirmed: bool,
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
            if apply == ApplyMode::Commit && !confirmed {
                return Err(LctrlError::InvalidArgument {
                    detail: "BIOS writes require --yes after reviewing the change".into(),
                });
            }
            let selections = bios.selections(&name)?;
            if selections.is_empty() {
                return Err(LctrlError::Unsupported {
                    feature: format!("bios.selections.{name}"),
                });
            }
            if !selections.iter().any(|selection| selection == &value) {
                return Err(LctrlError::InvalidArgument {
                    detail: format!(
                        "BIOS value {value:?} is not one of the exact selections for {name}: {}",
                        selections.join(", ")
                    ),
                });
            }
            let change = BiosChange::new(name.clone(), value.clone())?;
            if apply == ApplyMode::DryRun {
                return structured_output(&change, "BIOS setting dry run validated\n".into());
            }
            bios.stage(change.clone())?;
            if save {
                bios.save()?;
                let actual = bios.get(&name)?.ok_or_else(|| LctrlError::VerifyMismatch {
                    requested: value.clone(),
                    actual: "setting absent after save".into(),
                })?;
                if actual.value != value {
                    return Err(LctrlError::VerifyMismatch {
                        requested: value,
                        actual: actual.value,
                    });
                }
            }
            structured_output(
                &change,
                if save {
                    "BIOS setting saved and read back\n".into()
                } else {
                    "BIOS setting staged; use an explicit safe save flow\n".into()
                },
            )
        }
        BiosCommand::Save => Err(LctrlError::Unsupported {
            feature: "bios.save.global-buffer".into(),
        }),
        BiosCommand::Discard | BiosCommand::Defaults | BiosCommand::Password { .. } => {
            Err(LctrlError::Unsupported {
                feature: "bios.experimental-or-underspecified".into(),
            })
        }
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
    LctrlError::Unsupported {
        feature: command_feature(&command),
    }
}

fn command_feature(command: &Command) -> String {
    match command {
        Command::Info => "info".into(),
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
