//! Command-line surface for `vantage`.
//!
//! The command tree deliberately lives separately from host backends. Concrete
//! platform composition roots opt into only the hardware services with safe,
//! verified implementations; missing services return structured unsupported
//! errors rather than synthetic success.

use std::{
    collections::BTreeMap,
    env,
    fmt::Write as _,
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use lctrl_core::{
    ApplyMode, Availability, BacklightState, BiosChange, Capability, CapabilitySet, ChargeMode,
    ChargeModeActual, DeviceState, DiagnosticKind, DolbyProfile, DolbyState, DriverIdentity,
    DriverVersion, FanMode, HardwareInfo, LctrlError, LocalDriverRollbackRequest,
    LocalDriverUpdateRequest, LocalPackageInstallRequest, NoiseCancelState,
    NoiseCancellationMode, PerformanceMode, Platform,
    PowerGuid, PowerMutation, PowerRangeObservation, PowerSchemeId, PowerSettingKey,
    PowerSettingValue, PowerSource, PowerSourceValues, ProcessPolicy, ProcessSnapshot,
    ProcessTarget, ProcessorBoostMode, SceneAction, SensorDescriptor, SensorGuid, SensorInventory,
    SensorReading, SensorValue, SystemIdentity, UpdateCapability,
    UpdateHistoryLedger,
};
use lctrl_hal::ScenePolicyControl;
use lctrl_hal::{
    AudioControl, AudioEndpointControl, BatteryControl, BiosControl, BluetoothControl,
    BluetoothInventory, BluetoothMajorClass, ControlConflictDetection, DbdcControl,
    DiagnosticsControl, EventLogControl, FanControl, GpuOcControl, Hal, HotkeyControl,
    ItsPolicyControl,
    FirmwareControl,
    KeyboardControl, LocalPackageInstallControl, LocalUpdateControl, MachineFixControl,
    MagicBayControl, MobileBroadbandControl, NoiseCancelControl, OsdControl,
    DisplayStatusControl, PanelBrightnessControl, PanelControl, PanelFeaturesControl,
    PanelIdentityControl,
    PerformanceControl, PowerControl, PowerLimitKind, PrivacyControl, ProcessControl,
    ProcessorPowerControl, RadioControl, RadioKind, RadioState, SenseControl, SenseFeature,
    BitLockerState, BitLockerVolume, SecureBootState, SecurityControl, SecuritySource,
    SensorControl,
    SecurityStatusReport, StorageStatusControl, TemperatureControl, TouchpadControl,
    TuningControl, UpdateControl,
    UsbControl, WifiBand, WifiSecurityControl, WifiStatusControl, WifiStatusReport,
    TpmState,
};
use serde::{Deserialize, Serialize};

mod battery_health;
mod battery_observation;
mod capability;
mod power_ac;
use serde_json::Value;

/// Result returned by [`execute`].
pub type CommandResult = lctrl_core::Result<CommandOutput>;

/// The read-only observation produced by each streaming sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamKind {
    Telemetry,
    Temperatures,
    Battery,
    TuneWatch,
}

/// A streaming command and the minimum delay between completed samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSpec {
    pub kind: StreamKind,
    pub interval: Duration,
}

/// The explicit set of hardware services available to one CLI invocation.
///
/// A service is absent when the selected platform has no safe implementation
/// for it. Dispatch then returns `Unsupported`; it never synthesizes success.
#[derive(Clone, Copy)]
pub struct CommandServices<'a> {
    hal: &'a dyn Hal,
    audio: Option<&'a dyn AudioControl>,
    audio_endpoints: Option<&'a dyn AudioEndpointControl>,
    noise_cancel: Option<&'a dyn NoiseCancelControl>,
    battery: Option<&'a dyn BatteryControl>,
    bios: Option<&'a dyn BiosControl>,
    conflicts: Option<&'a dyn ControlConflictDetection>,
    dbdc: Option<&'a dyn DbdcControl>,
    diagnostics: Option<&'a dyn DiagnosticsControl>,
    fan: Option<&'a dyn FanControl>,
    gpu_oc: Option<&'a dyn GpuOcControl>,
    keyboard: Option<&'a dyn KeyboardControl>,
    its_policy: Option<&'a dyn ItsPolicyControl>,
    magicbay: Option<&'a dyn MagicBayControl>,
    mobile_broadband: Option<&'a dyn MobileBroadbandControl>,
    performance: Option<&'a dyn PerformanceControl>,
    power: Option<&'a dyn PowerControl>,
    privacy: Option<&'a dyn PrivacyControl>,
    touchpad: Option<&'a dyn TouchpadControl>,
    panel: Option<&'a dyn PanelControl>,
    panel_brightness: Option<&'a dyn PanelBrightnessControl>,
    panel_features: Option<&'a dyn PanelFeaturesControl>,
    panel_identity: Option<&'a dyn PanelIdentityControl>,
    display_status: Option<&'a dyn DisplayStatusControl>,
    process: Option<&'a dyn ProcessControl>,
    temperature: Option<&'a dyn TemperatureControl>,
    tuning: Option<&'a dyn TuningControl>,
    usb: Option<&'a dyn UsbControl>,
    usb_ports: Option<&'a dyn lctrl_hal::UsbPortInventoryControl>,
    processor_power: Option<&'a dyn ProcessorPowerControl>,
    sense: Option<&'a dyn SenseControl>,
    update: Option<&'a dyn UpdateControl>,
    local_update: Option<&'a dyn LocalUpdateControl>,
    local_package: Option<&'a dyn LocalPackageInstallControl>,
    wifi: Option<&'a dyn WifiSecurityControl>,
    wifi_status: Option<&'a dyn WifiStatusControl>,
    bluetooth: Option<&'a dyn BluetoothControl>,
    radio: Option<&'a dyn RadioControl>,
    sensor: Option<&'a dyn SensorControl>,
    machine_fix: Option<&'a dyn MachineFixControl>,
    scene: Option<&'a dyn ScenePolicyControl>,
    osd: Option<&'a dyn OsdControl>,
    hotkey: Option<&'a dyn HotkeyControl>,
    firmware: Option<&'a dyn FirmwareControl>,
    security: Option<&'a dyn SecurityControl>,
    storage: Option<&'a dyn StorageStatusControl>,
    event_log: Option<&'a dyn EventLogControl>,
}

impl<'a> CommandServices<'a> {
    pub const fn new(hal: &'a dyn Hal) -> Self {
        Self {
            hal,
            audio: None,
            audio_endpoints: None,
            noise_cancel: None,
            battery: None,
            bios: None,
            conflicts: None,
            dbdc: None,
            diagnostics: None,
            fan: None,
            gpu_oc: None,
            keyboard: None,
            its_policy: None,
            magicbay: None,
            mobile_broadband: None,
            performance: None,
            power: None,
            privacy: None,
            touchpad: None,
            panel: None,
            panel_brightness: None,
            panel_features: None,
            panel_identity: None,
            display_status: None,
            process: None,
            temperature: None,
            tuning: None,
            usb: None,
            usb_ports: None,
            processor_power: None,
            sense: None,
            update: None,
            local_update: None,
            local_package: None,
            wifi: None,
            wifi_status: None,
            bluetooth: None,
            radio: None,
            sensor: None,
            machine_fix: None,
            scene: None,
            osd: None,
            hotkey: None,
            firmware: None,
            security: None,
            storage: None,
            event_log: None,
        }
    }

    #[must_use]
    pub fn with_audio(mut self, audio: &'a dyn AudioControl) -> Self {
        self.audio = Some(audio);
        self
    }

    #[must_use]
    pub fn with_audio_endpoints(
        mut self,
        audio_endpoints: &'a dyn AudioEndpointControl,
    ) -> Self {
        self.audio_endpoints = Some(audio_endpoints);
        self
    }

    #[must_use]
    pub fn with_noise_cancel(mut self, noise_cancel: &'a dyn NoiseCancelControl) -> Self {
        self.noise_cancel = Some(noise_cancel);
        self
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
    pub fn with_firmware(mut self, firmware: &'a dyn FirmwareControl) -> Self {
        self.firmware = Some(firmware);
        self
    }

    #[must_use]
    pub fn with_event_log(mut self, event_log: &'a dyn EventLogControl) -> Self {
        self.event_log = Some(event_log);
        self
    }

    #[must_use]
    pub fn with_security(mut self, security: &'a dyn SecurityControl) -> Self {
        self.security = Some(security);
        self
    }

    #[must_use]
    pub fn with_storage(mut self, storage: &'a dyn StorageStatusControl) -> Self {
        self.storage = Some(storage);
        self
    }

    #[must_use]
    pub fn with_conflict_detection(mut self, conflicts: &'a dyn ControlConflictDetection) -> Self {
        self.conflicts = Some(conflicts);
        self
    }

    #[must_use]
    pub fn with_dbdc(mut self, dbdc: &'a dyn DbdcControl) -> Self {
        self.dbdc = Some(dbdc);
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
    pub fn with_gpu_oc(mut self, gpu_oc: &'a dyn GpuOcControl) -> Self {
        self.gpu_oc = Some(gpu_oc);
        self
    }

    #[must_use]
    pub fn with_keyboard(mut self, keyboard: &'a dyn KeyboardControl) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    #[must_use]
    pub fn with_its_policy(mut self, its_policy: &'a dyn ItsPolicyControl) -> Self {
        self.its_policy = Some(its_policy);
        self
    }

    #[must_use]
    pub fn with_magicbay(mut self, magicbay: &'a dyn MagicBayControl) -> Self {
        self.magicbay = Some(magicbay);
        self
    }

    #[must_use]
    pub fn with_mobile_broadband(mut self, mobile_broadband: &'a dyn MobileBroadbandControl) -> Self {
        self.mobile_broadband = Some(mobile_broadband);
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
    pub fn with_panel_features(mut self, panel_features: &'a dyn PanelFeaturesControl) -> Self {
        self.panel_features = Some(panel_features);
        self
    }

    #[must_use]
    pub fn with_panel_brightness(mut self, panel: &'a dyn PanelBrightnessControl) -> Self {
        self.panel_brightness = Some(panel);
        self
    }

    #[must_use]
    pub fn with_panel_identity(mut self, panel_identity: &'a dyn PanelIdentityControl) -> Self {
        self.panel_identity = Some(panel_identity);
        self
    }

    #[must_use]
    pub fn with_display_status(mut self, display_status: &'a dyn DisplayStatusControl) -> Self {
        self.display_status = Some(display_status);
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
    pub fn with_usb(mut self, usb: &'a dyn UsbControl) -> Self {
        self.usb = Some(usb);
        self
    }

    #[must_use]
    pub fn with_usb_ports(mut self, usb_ports: &'a dyn lctrl_hal::UsbPortInventoryControl) -> Self {
        self.usb_ports = Some(usb_ports);
        self
    }

    #[must_use]
    pub fn with_processor_power(mut self, processor_power: &'a dyn ProcessorPowerControl) -> Self {
        self.processor_power = Some(processor_power);
        self
    }

    #[must_use]
    pub fn with_sense(mut self, sense: &'a dyn SenseControl) -> Self {
        self.sense = Some(sense);
        self
    }

    #[must_use]
    pub fn with_process(mut self, process: &'a dyn ProcessControl) -> Self {
        self.process = Some(process);
        self
    }
    #[must_use]
    pub fn with_update(mut self, update: &'a dyn UpdateControl) -> Self {
        self.update = Some(update);
        self
    }

    #[must_use]
    pub fn with_local_update(mut self, local_update: &'a dyn LocalUpdateControl) -> Self {
        self.local_update = Some(local_update);
        self
    }

    #[must_use]
    pub fn with_local_package(
        mut self,
        local_package: &'a dyn LocalPackageInstallControl,
    ) -> Self {
        self.local_package = Some(local_package);
        self
    }

    #[must_use]
    pub fn with_wifi(mut self, wifi: &'a dyn WifiSecurityControl) -> Self {
        self.wifi = Some(wifi);
        self
    }

    #[must_use]
    pub fn with_wifi_status(mut self, wifi_status: &'a dyn WifiStatusControl) -> Self {
        self.wifi_status = Some(wifi_status);
        self
    }

    #[must_use]
    pub fn with_bluetooth(mut self, bluetooth: &'a dyn BluetoothControl) -> Self {
        self.bluetooth = Some(bluetooth);
        self
    }

    #[must_use]
    pub fn with_radio(mut self, radio: &'a dyn RadioControl) -> Self {
        self.radio = Some(radio);
        self
    }

    #[must_use]
    pub fn with_sensor(mut self, sensor: &'a dyn SensorControl) -> Self {
        self.sensor = Some(sensor);
        self
    }

    #[must_use]
    pub fn with_machine_fix(mut self, machine_fix: &'a dyn MachineFixControl) -> Self {
        self.machine_fix = Some(machine_fix);
        self
    }

    #[must_use]
    pub fn with_scene(mut self, scene: &'a dyn ScenePolicyControl) -> Self {
        self.scene = Some(scene);
        self
    }

    #[must_use]
    pub fn with_osd(mut self, osd: &'a dyn OsdControl) -> Self {
        self.osd = Some(osd);
        self
    }

    #[must_use]
    pub fn with_hotkey(mut self, hotkey: &'a dyn HotkeyControl) -> Self {
        self.hotkey = Some(hotkey);
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

/// Render an error. JSON mode is intentionally exactly the core error report
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
    /// Restore the state a write replaced once the write is applied and
    /// verified, so the device ends up where it started.
    #[arg(long, global = true)]
    pub auto_restore: bool,

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

/// Identify a streaming command and validate its sampling interval.
///
/// Telemetry without an interval is a single-result command. Temperature
/// watch always streams, including when its default interval is used.
pub fn stream_spec(cli: &Cli) -> lctrl_core::Result<Option<StreamSpec>> {
    let (kind, seconds) = match &cli.command {
        Command::Battery {
            command: Some(BatteryCommand::Watch { interval }),
        } => (StreamKind::Battery, *interval),
        Command::Tune {
            command:
                Some(TuneCommand::Telemetry {
                    interval: Some(seconds),
                }),
        } => (StreamKind::Telemetry, *seconds),
        Command::Tune {
            command: Some(TuneCommand::Watch { interval }),
        } => (StreamKind::TuneWatch, *interval),
        Command::Perf {
            command:
                Some(PerfCommand::Temp {
                    command: Some(TempCommand::Watch { interval }),
                }),
        } => (StreamKind::Temperatures, *interval),
        _ => return Ok(None),
    };
    if seconds == 0 {
        return Err(LctrlError::InvalidArgument {
            detail: "sampling interval must be at least one second".into(),
        });
    }
    Ok(Some(StreamSpec {
        kind,
        interval: Duration::from_secs(seconds),
    }))
}

/// Take one read-only sample; callers decide when to take the next sample.
pub fn sample_stream(kind: StreamKind, services: &CommandServices<'_>) -> CommandResult {
    match kind {
        StreamKind::Telemetry => execute_tune_telemetry(services),
        StreamKind::Temperatures => execute_temperature_list(services.temperature),
        StreamKind::Battery => battery_observation::execute(services.battery),
        StreamKind::TuneWatch => execute_tune_telemetry(services),
    }
}

fn streaming_command_error(kind: StreamKind) -> LctrlError {
    let command = match kind {
        StreamKind::Telemetry => "tune.telemetry",
        StreamKind::Temperatures => "perf.temp.watch",
        StreamKind::Battery => "battery.watch",
        StreamKind::TuneWatch => "tune.watch",
    };
    LctrlError::Unsupported {
        feature: format!("{command} requires the streaming API"),
    }
}

/// Top-level `vantage` commands.
// The command tree intentionally stays one flat enum: clap derives the CLI
// surface from it and boxing the large variants would only move the bytes.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Report platform, hardware identity, and discovered capabilities.
    Info,
    /// Probe read-only platform channels and report actionable readiness failures.
    Doctor,
    /// Report each command's observed status on this machine as a matrix.
    Capability,
    /// Read the Lenovo records the OS event log holds, newest first.
    ///
    /// Read-only: no channel is enabled, cleared or written. Classic records
    /// carry no message resource, so only the raw `EventData` strings are shown.
    Events {
        /// Maximum number of records to return.
        #[arg(long, default_value_t = lctrl_hal::DEFAULT_EVENT_LOG_RECORDS)]
        limit: usize,
    },
    /// Processor identity, rated clocks, the derived current clock and the
    /// processor power policy (read-only).
    Cpu {
        #[command(subcommand)]
        command: Option<CpuCommand>,
    },
    /// Battery state, charge mode, and charge behaviour.
    Battery {
        #[command(subcommand)]
        command: Option<BatteryCommand>,
    },
    /// USB power delivery while the machine is off or running on battery.
    Usb {
        #[command(subcommand)]
        command: Option<UsbCommand>,
    },
    /// Windows power schemes and the one-shot saver workflow.
    Power {
        #[command(subcommand)]
        command: Option<PowerCommand>,
    },
    /// Performance mode, fans, temperatures, power limits, and process priority.
    Perf {
        #[command(subcommand)]
        command: Option<PerfCommand>,
    },
    /// Tuning profiles, power limits, EPP, turbo, and telemetry.
    Tune {
        #[command(subcommand)]
        command: Option<TuneCommand>,
    },
    /// Keyboard backlight and Fn-key behaviour.
    Kbd {
        #[command(subcommand)]
        command: Option<KbdCommand>,
    },
    /// Touchpad state, on/off, and recovery.
    Touchpad {
        #[arg(value_enum)]
        state: TouchpadAction,
    },
    /// Internal display brightness, refresh rate, colour, and eye care.
    Panel {
        #[command(subcommand)]
        command: Option<PanelCommand>,
    },
    /// Read-only display state: mode, refresh rate, per-monitor scaling, and HDR.
    Display {
        #[command(subcommand)]
        command: Option<DisplayCommand>,
    },
    /// Camera, microphone, and fingerprint access.
    Privacy {
        #[command(subcommand)]
        command: Option<PrivacyCommand>,
    },
    /// Smart sensing: presence, attention, keyboard light, and the HPD reset.
    Sense {
        #[command(subcommand)]
        command: Option<SenseCommand>,
    },
    /// Dolby profile and microphone noise cancellation.
    Audio {
        #[command(subcommand)]
        command: Option<AudioCommand>,
    },
    /// Read and stage Lenovo BIOS settings.
    Bios {
        #[command(subcommand)]
        command: Option<BiosCommand>,
    },
    /// Read UEFI/BIOS firmware identity and the firmware-resource table.
    ///
    /// Inventory only: no capsule is staged, flashed or cleared.
    Firmware {
        #[command(subcommand)]
        command: Option<FirmwareCommand>,
    },
    /// Read-only identity and health of each physical drive.
    ///
    /// Observation only: no self-test, sanitize or firmware-slot request is
    /// issued, because the recovered channel answers queries only.
    Storage {
        #[command(subcommand)]
        command: Option<StorageCommand>,
    },
    /// Magic Bay accessory detection and state.
    Magicbay {
        #[command(subcommand)]
        command: Option<MagicbayCommand>,
    },
    /// Lenovo Fn OSD preference and the replacement on-screen overlay.
    Osd {
        #[command(subcommand)]
        command: Option<OsdCommand>,
    },
    /// Global hotkey registration and the Lenovo Fn hotkey package state.
    Hotkey {
        #[command(subcommand)]
        command: Option<HotkeyCommand>,
    },
    /// Lenovo UDC catalogue, package install and rollback, and local drivers.
    Update {
        #[command(subcommand)]
        command: Option<UpdateCommand>,
    },
    /// Diagnostic scans: list the items, then run some or all of them.
    Scan {
        #[command(subcommand)]
        command: Option<ScanCommand>,
    },
    /// Managed hardware snapshots: capture, diff, and restore.
    Snapshot {
        #[command(subcommand)]
        command: Option<SnapshotCommand>,
    },
    /// Wi-Fi security assessment and the bounded observation history.
    Wifi {
        #[command(subcommand)]
        command: Option<WifiCommand>,
    },
    /// Bluetooth: the local radios and the devices this machine knows.
    Bluetooth {
        #[command(subcommand)]
        command: Option<BluetoothCommand>,
    },
    /// Hardware radios: observe Wi-Fi/Bluetooth/WWAN state, switch one off or on.
    Radio {
        #[command(subcommand)]
        command: Option<RadioCommand>,
    },
    /// Operating-system sensors: list the sensors this machine instantiates and
    /// take one read-only report from a selected sensor.
    Sensor {
        #[command(subcommand)]
        command: Option<SensorCommand>,
    },
    /// Platform security posture: Secure Boot, TPM and BitLocker. Read-only.
    Security {
        #[command(subcommand)]
        command: Option<SecurityCommand>,
    },
    /// MachineFix: transparent ruleset, repair, and the local ledger.
    Fix {
        #[command(subcommand)]
        command: Option<FixCommand>,
    },
    /// Scene policies: ruleset, match test, and the local ledger.
    Scene {
        #[command(subcommand)]
        command: Option<SceneCommand>,
    },
    /// Optional resident daemon lifecycle.
    Daemon {
        #[command(subcommand)]
        command: Option<DaemonCommand>,
    },
    /// Print shell completions for the command tree.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Processor command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum CpuCommand {
    /// Identity, firmware-declared rated clocks, the derived current clock and
    /// the processor power policy. Read-only.
    Status,
}

/// Storage command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum StorageCommand {
    /// Enumerate the physical drives and report the identity and health fields
    /// each one actually answered. Read-only.
    Status,
}

/// MachineFix command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum FixCommand {
    /// List the transparent ruleset and which issues are present.
    Scan,
    /// Apply the named rule's repair, or report the plan under --dry-run.
    ///
    /// `--dry-run` is the root-level global flag; the handler consumes the
    /// resolved `ApplyMode` instead of a second local declaration.
    Run {
        issue: String,
    },
    /// Show the local ledger of applied fixes and their rollbacks.
    Track,
}

/// Scene-policy command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SceneCommand {
    /// List the transparent scene-policy ruleset.
    List,
    /// Match a documented event class against the ruleset; with --dry-run, plan the firing rule's action without writing.
    Test {
        event: String,
        /// Test a specific rule by its id instead of the first rule that fires on the event.
        #[arg(long)]
        rule: Option<String>,
    },
    /// Show the local ledger of applied scene actions and their rollbacks.
    Track,
}

/// Battery command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum BatteryCommand {
    Status,
    /// Show design versus full-charge capacity, wear percent and cycle count (read-only).
    Health,
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
    Watch {
        /// Minimum delay between samples, in seconds.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
        interval: u64,
    },
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
    /// List controllers, hubs and ports with the speed and power each port
    /// actually reports (read-only).
    Ports,
}

/// Power command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PowerCommand {
    Scheme {
        #[command(subcommand)]
        command: Option<SchemeCommand>,
    },
    SaverOnce,
    /// Read-only AC adapter state: line state, vendor adapter identity and
    /// adapter power where the firmware exposes it.
    Ac,
    /// Read-only instantaneous battery-pack power flow and its exact source.
    Draw,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SchemeCommand {
    List,
    Get {
        name: String,
        /// Subgroup GUID of the advanced setting (requires --setting).
        #[arg(long, requires = "setting", value_name = "GUID")]
        subgroup: Option<String>,
        /// Setting GUID within the subgroup (requires --subgroup).
        #[arg(long, requires = "subgroup", value_name = "GUID")]
        setting: Option<String>,
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
    Dbdc {
        #[command(subcommand)]
        command: Option<DbdcCommand>,
    },
    /// Decode the dispatcher's published `Policy_*` PowerSlider values (read-only).
    Policy,
    Gpu {
        #[command(subcommand)]
        command: Option<GpuCommand>,
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
pub enum DbdcCommand {
    /// 读取电池直放逐流限流表(LENOVO_REPORT_DBDC_DATA,只读)
    Get,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum GpuCommand {
    /// 读取 GPU 超频能力与 OC 参数块(LENOVO_GAMEZONE_DATA,只读)
    Status,
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
    Read {
        id: String,
    },
    Watch {
        /// Minimum delay between samples, in seconds.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
        interval: u64,
    },
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
        /// Continuously sample with this minimum delay, in seconds.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        interval: Option<u64>,
    },
    Watch {
        /// Minimum delay between samples, in seconds.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
        interval: u64,
    },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum ProfileCommand {
    List,
    Show {
        name: String,
    },
    Apply {
        name: String,
    },
}

/// Keyboard command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum KbdCommand {
    Backlight {
        /// Backlight state: `off`, `low`, `high`, `auto`, or the raw firmware
        /// level (`0..3`). `auto` is the ambient-light mode of firmware that
        /// advertises a fourth state; other machines reject it.
        #[arg(value_parser = parse_backlight_level)]
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

/// Parse a named backlight state to the firmware level it selects.
///
/// The hardware levels are `0` off, `1` low, `2` high and, only on firmware
/// that advertises it, `3` auto. Which of them exist is discovered per machine
/// by the HAL, so a level outside the published range still fails there with
/// the firmware's own bound rather than in the parser.
fn parse_backlight_level(value: &str) -> std::result::Result<u8, String> {
    match value {
        "off" => Ok(0),
        "low" => Ok(1),
        "high" => Ok(2),
        "auto" => Ok(3),
        other => other.parse::<u8>().map_err(|_| {
            format!("expected off, low, high, auto or a firmware level 0..3, got {other:?}")
        }),
    }
}

/// Panel command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PanelCommand {
    /// Read-only identity of the embedded panel, decoded from its own EDID.
    Info,
    Brightness {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: Option<u8>,
        /// Refuse if the observed current brightness differs from this value.
        #[arg(long, requires = "percent", value_parser = clap::value_parser!(u8).range(0..=100))]
        expect_current: Option<u8>,
    },
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

/// Display-state command tree. Read-only by construction: no HDR, scaling, or
/// mode transition exists here.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum DisplayCommand {
    /// Current mode, per-monitor scale, and HDR/wide-colour state.
    ///
    /// Two scopes are reported and never merged: `displays` is the calling
    /// session's live desktop (which a service or SSH logon cannot see), and
    /// `monitors` is machine-wide driver/EDID state joined by monitor hardware
    /// id.
    Status,
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

/// Privacy camera actions. `on`/`off` are the established spellings;
/// `enable`/`disable` are accepted as the same transitions.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum PrivacyCamAction {
    #[value(alias = "enable")]
    On,
    #[value(alias = "disable")]
    Off,
    /// Read the runtime devnodes and the persistent BIOS switch together
    /// without changing any device.
    Status,
    /// Read the per-application consent and last-use records Windows'
    /// Capability Access Manager stores for the camera. This is what the OS
    /// recorded, not an observation that a camera is streaming.
    #[value(name = "inuse", alias = "in-use")]
    InUse,
}

impl PrivacyCamAction {
    /// The transition this action requests, or `None` for a read-only action.
    const fn toggle(self) -> Option<Toggle> {
        match self {
            Self::On => Some(Toggle::On),
            Self::Off => Some(Toggle::Off),
            Self::Status | Self::InUse => None,
        }
    }
}

/// Privacy command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PrivacyCommand {
    #[command(visible_alias = "camera")]
    Cam {
        #[arg(value_enum)]
        action: PrivacyCamAction,
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
    CameraAssist {
        #[arg(value_enum)]
        state: Toggle,
    },
    ApproachAutoAdjust {
        #[arg(value_enum)]
        state: Toggle,
    },
    LeaveAutoAdjust {
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
    /// Reset all HPD user settings to service defaults via op 7
    /// (`ResetAllSetting`). Use `--auto-restore` to capture the current
    /// values and reapply them after the reset.
    Reset,
}

/// Audio command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum AudioCommand {
    Dolby { profile: DolbyProfileArg },
    NoiseCancel { mode: NoiseCancelArg },
    /// List the OS audio endpoints through the MMDevice API.
    Endpoints {
        /// Include disabled, unplugged, and not-present endpoints.
        #[arg(long)]
        all: bool,
    },
    /// Read or set one endpoint's master volume.
    Volume {
        /// Target volume in whole percent (0-100); omit to read the state.
        percent: Option<u8>,
        /// Endpoint id; defaults to the console-role render endpoint.
        #[arg(long)]
        device: Option<String>,
    },
    /// Read or set one endpoint's mute flag.
    Mute {
        /// `on`/`off`; omit to read the state.
        state: Option<MuteArg>,
        /// Endpoint id; defaults to the console-role render endpoint.
        #[arg(long)]
        device: Option<String>,
    },
}

/// Firmware command tree.
///
/// `status` is the only verb, and it is a read: the recovered firmware
/// contract on this platform exposes the identity and the ESRT projection, not
/// a capsule write this tool may drive.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum FirmwareCommand {
    /// Report firmware identity, the ESRT entries, and the last attempt.
    Status,
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
    Defaults {
        /// Current supervisor password, required when one is already set.
        #[arg(long)]
        password: Option<String>,
    },
    Password {
        #[command(subcommand)]
        command: Option<PasswordCommand>,
    },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum PasswordCommand {
    Status,
    Set {
        /// New supervisor password.
        value: String,
        /// Current supervisor password, required when one is already set.
        #[arg(long)]
        current: Option<String>,
    },
    Clear {
        /// Current supervisor password.
        value: String,
    },
    Verify {
        /// Supervisor password to check against the firmware.
        value: String,
    },
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

// Flat subcommand tree kept as one enum for the same reason as `Command`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum UpdateCommand {
    Check {
        #[arg(long, value_enum)]
        severity: Option<UpdateSeverity>,
    },
    LocalDriver {
        #[command(subcommand)]
        command: LocalDriverCommand,
    },
    /// Install a package the Lenovo UDC service already downloaded, using the
    /// package's own signed `install.ps1` (the payload directory is
    /// `...\Udc\Download\<package_id>\<version>\temp`). The catalogue must
    /// still list the package, its signature and pinned hash are verified, and
    /// firmware packages are refused. Never downloads and never reboots.
    LocalInstall {
        /// The package payload directory (the one holding `install.ps1`).
        package: PathBuf,
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

/// Local (vendored `.inf`) driver update and rollback. The hardware ID binds to
/// exactly one present PnP device; firmware classes are refused; the package
/// signature is verified before install; dry-run performs no write.
// Flat subcommand tree kept as one enum for the same reason as `Command`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum LocalDriverCommand {
    /// Install a locally admitted driver `.inf` bound to one hardware ID.
    Apply {
        /// Path to the local driver `.inf` file.
        inf: PathBuf,
        /// Hardware ID the package must bind to (e.g. `PCI\VEN_8086&DEV_1234`).
        hardware_id: String,
    },
    /// Roll a present device's driver back to a previously admitted identity.
    Rollback {
        device_instance_id: String,
        /// Device class GUID (must bind both the current and restore identities).
        #[arg(long)]
        class_guid: String,
        /// Hardware ID (must bind both the current and restore identities).
        #[arg(long)]
        matching_device_id: String,
        #[arg(long = "expected-provider")]
        expected_provider: String,
        #[arg(long = "expected-description")]
        expected_description: String,
        /// Current driver version, `major.minor.build.revision`.
        #[arg(long = "expected-version")]
        expected_version: String,
        #[arg(long = "expected-date")]
        expected_date: String,
        #[arg(long = "expected-section")]
        expected_section: String,
        #[arg(long = "restore-provider")]
        restore_provider: String,
        #[arg(long = "restore-description")]
        restore_description: String,
        /// Driver version to restore, `major.minor.build.revision`.
        #[arg(long = "restore-version")]
        restore_version: String,
        #[arg(long = "restore-date")]
        restore_date: String,
        #[arg(long = "restore-section")]
        restore_section: String,
    },
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
pub enum WifiCommand {
    /// Read the live association of every WLAN interface: SSID, BSSID, signal
    /// (percent and dBm), band, channel, rates, and the negotiated security.
    /// Read-only; it never scans, so it cannot disturb the current link.
    Status,
    /// Read the current live interfaces, saved profiles, and security
    /// assessment (open/WEP/WPA/WPA2/WPA3/unknown) with evidence-based risk
    /// reasons.
    Security,
    /// Show the bounded, memory-only history policy and observations.
    History,
    /// Clear this process's in-memory Wi-Fi observations. No persisted
    /// history exists to delete.
    ClearHistory,
}

/// Bluetooth command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum BluetoothCommand {
    /// List the local radios and the devices this machine knows (paired,
    /// remembered, or connected) with address, class, state, and last use.
    /// Read-only: it never starts a discovery/inquiry.
    List,
}

/// Radio command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum RadioCommand {
    /// Report every enumerated radio and its state, plus the Wi-Fi per-PHY software and hardware
    /// radio switches and the kinds this platform exposes no radio for.
    Status,
    /// Switch one radio off and verify it by read-back. Refuses by default when the radio carries
    /// this remote session; `--force` is for an operator with out-of-band access.
    Off {
        #[arg(value_enum)]
        kind: RadioKindArg,
        /// Switch the radio off even when it is carrying the current session.
        #[arg(long)]
        force: bool,
    },
    /// Switch one radio on and verify it by read-back.
    On {
        #[arg(value_enum)]
        kind: RadioKindArg,
    },
}

/// Sensor command tree.
///
/// Both verbs are read-only: the platform's sensor stack owns report
/// intervals, thresholds and event sinks, and nothing here subscribes to one.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SensorCommand {
    /// List every sensor this machine instantiates, with its identity, state,
    /// report timestamp and freshness.
    ///
    /// A sensor that was enumerated but returned no report is listed in a
    /// separate `unavailable` set: it has no value, which is not a zero.
    List,
    /// Take one report from the selected sensor.
    ///
    /// A target this machine does not instantiate (an accelerometer on a SKU
    /// that has none, for example) is reported as absent instead of being
    /// answered with a zero reading.
    Read {
        /// List index (`5`), full device path, full name, or a unique
        /// case-insensitive part of the name.
        target: String,
    },
}

impl SensorCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Read { .. } => "read",
        }
    }
}

/// The radio media a switch command can address by name.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum RadioKindArg {
    Wifi,
    Bluetooth,
    /// Mobile broadband (WWAN).
    Wwan,
}

impl RadioKindArg {
    const fn kind(self) -> RadioKind {
        match self {
            Self::Wifi => RadioKind::Wifi,
            Self::Bluetooth => RadioKind::Bluetooth,
            Self::Wwan => RadioKind::MobileBroadband,
        }
    }
}

/// Platform security posture command tree.
#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum SecurityCommand {
    /// Report Secure Boot, TPM and BitLocker state, each with its own source.
    ///
    /// Read-only: no firmware variable, TPM operation, key protector or
    /// recovery password is written or read.
    Status,
}

impl SecurityCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
        }
    }
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum OsdCommand {
    /// Turn the Lenovo Fn on-screen display on (CapsLk/NumLk indicators) via
    /// the `LenovoFnAndFunctionKeys` user-defined control codes, verified by
    /// registry read-back.
    Enable,
    /// Turn the Lenovo Fn on-screen display off. Indicators the installed
    /// hotkey package does not control are reported as `not_supported` and are
    /// never sent a control code.
    Disable,
    /// Paint the replacement overlay as a smoke test of the desktop gate.
    Test,
    /// Show a custom on-screen overlay with the given lines.
    Show {
        /// The lines to display on the overlay.
        #[arg(required = true)]
        lines: Vec<String>,
    },
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum HotkeyCommand {
    /// List the hotkeys this process has registered and made live.
    List,
    /// Report the Lenovo Fn hotkey package state read-only: the owner service
    /// and its session-1 renderers, the `Capbility` flags, the firmware FnLock
    /// and Fn/Ctrl switches, and the persisted per-keyboard tables.
    Status,
    /// Register a global Alt-hotkey natively (id + virtual key code).
    Register {
        /// The internal hotkey identifier.
        #[arg(default_value_t = 1)]
        id: u32,
        /// The virtual-key code (e.g. 0x47 for the G key).
        #[arg(default_value_t = 0x47)]
        vk: u32,
    },
    /// Simulate / report a hotkey press for the given id (dry observation).
    Trigger {
        /// The internal hotkey identifier.
        #[arg(default_value_t = 1)]
        id: u32,
    },
    /// Release a previously registered hotkey.
    Unregister {
        /// The internal hotkey identifier.
        #[arg(default_value_t = 1)]
        id: u32,
    },
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

/// Touchpad actions. `on`/`off` are the established spellings;
/// `enable`/`disable` are accepted as the same transitions.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum TouchpadAction {
    #[value(alias = "enable")]
    On,
    #[value(alias = "disable")]
    Off,
    /// Read the aggregate input state without changing devices.
    Status,
    /// Read the HID capability descriptor: declared contacts, surface ranges,
    /// button type and the usages the collection declares. Read-only; needs no
    /// `--yes` and has no write path.
    Caps,
    /// Repair the input leaves: enable what an interrupted transition left
    /// disabled, or restart them when the pad is enumerated but not serving
    /// input.
    Recover,
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
}

/// Endpoint mute request.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum MuteArg {
    On,
    Off,
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
    /// Absent when the HAL reports no identity module at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<SystemIdentity>,
    features: BTreeMap<String, Capability>,
}

#[derive(Debug, Serialize)]
struct PrivacyStatusPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_camera: Option<DeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_camera_error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_microphone: Option<DeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_microphone_error: Option<Value>,
    bios: Vec<lctrl_core::BiosItem>,
}

/// Windows' occlusion channel is `Windows.Media.Devices.CameraOcclusionInfo`,
/// which is only reachable from the `VideoDeviceController` of an initialized
/// WinRT `MediaCapture` session: reading it opens the camera, it cannot be read
/// while the devnode is disabled, and Microsoft documents the value as
/// meaningful only while streaming and explicitly not as an absolute privacy
/// indicator. The reference target exposes no shutter hardware, so the channel
/// stays closed instead of being faked from the devnode state.
const CAMERA_OCCLUSION_REASON: &str = "CameraOcclusionInfo requires an initialized WinRT MediaCapture session; no shutter hardware on 21VG";

#[derive(Debug, Serialize)]
struct CameraOcclusionAvailability {
    supported: bool,
    reason: &'static str,
}

#[derive(Debug, Serialize)]
struct CameraStatusPayload {
    /// Aggregate answer: `None` when the devnodes disagree, which is exactly
    /// what the per-devnode list below attributes.
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<DeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_error: Option<Value>,
    devices: Vec<lctrl_core::PrivacyDeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    devices_error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    persistent: Option<lctrl_core::BiosItem>,
    occlusion: CameraOcclusionAvailability,
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
pub use capability::{
    ObservedCommandCapability, ObservedStatus, command_selector, evaluate_commands,
    evaluate_commands_with_hardware,
};
pub use lctrl_hal::{
    command_capabilities, command_capability, command_capability_for, command_registry,
};

pub fn execute_with_services(cli: Cli, services: CommandServices<'_>) -> CommandResult {
    // A parent command with no subcommand is a usage error, not a hardware
    // verdict: no channel was asked anything. Classifying it here keeps the
    // `Unsupported` reasons reserved for features the code actually refuses,
    // and stops a bare `privacy`/`magicbay`/`scan` from asserting that the
    // implementation is missing when it is attached.
    if let Some(parent) = missing_subcommand(&cli.command) {
        return Err(LctrlError::InvalidArgument {
            detail: format!("a {parent} subcommand is required; run `vantage {parent} --help`"),
        });
    }
    if let Some(spec) = stream_spec(&cli)? {
        return Err(streaming_command_error(spec.kind));
    }
    let invalid_brightness = match &cli.command {
        Command::Panel {
            command:
                Some(PanelCommand::Brightness {
                    percent,
                    expect_current,
                }),
        } => {
            percent.is_some_and(|value| value > 100)
                || expect_current.is_some_and(|value| value > 100)
                || (percent.is_none() && expect_current.is_some())
        }
        _ => false,
    };
    if invalid_brightness {
        return Err(LctrlError::InvalidArgument {
            detail: "brightness and expected current must be 0..=100; --expect-current requires a requested brightness".into(),
        });
    }
    // Single authoritative dry-run resolution: `--dry-run` is declared once, as
    // the root global flag, and every mutating handler receives this value.
    // No subcommand may declare its own `--dry-run`: clap skips propagating the
    // global into a subcommand that already declares the id
    // (`Command::_propagate_global_args`), which is what made the old
    // per-subcommand fields a second, drift-prone source of truth.
    let apply = if cli.dry_run {
        ApplyMode::DryRun
    } else {
        ApplyMode::Commit
    };
    let confirmed = cli.yes;
    let interactive = !cli.json;
    if apply == ApplyMode::Commit && command_is_mutating(&cli.command) {
        // Concurrent vendor control is reported first: it is a read-only
        // observation and needs no acknowledgement from the operator.
        preflight_control_conflicts(services.conflicts, confirmed, interactive)?;
    }
    match cli.command {
        Command::Info => execute_info(services.hal),
        Command::Doctor => execute_doctor(&services),
        Command::Capability => execute_capability(&services),
        Command::Events { limit } => execute_events_log(services.event_log, limit),
        Command::Cpu {
            command: None | Some(CpuCommand::Status),
        } => execute_cpu_status(services.hal, services.power, services.processor_power),
        Command::Battery {
            command: Some(BatteryCommand::Status),
        } => battery_observation::execute(services.battery),
        Command::Battery { command: None } => battery_observation::execute(services.battery),
        Command::Battery {
            command: Some(BatteryCommand::Health),
        } => battery_health::execute(services.battery),
        Command::Battery {
            command: Some(BatteryCommand::Adapter),
        } => execute_battery_adapter(services.battery),
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { mode }),
        } => execute_charge_mode(services.battery, mode, apply, cli.auto_restore),
        Command::Battery {
            command: Some(BatteryCommand::NightCharge { state }),
        } => execute_night_charge(services.battery, state, apply, cli.auto_restore),
        Command::Battery {
            command: Some(BatteryCommand::TemporaryMode),
        } => execute_temporary_charge(services.battery),
        Command::Power {
            command:
                Some(PowerCommand::Scheme {
                    command: Some(scheme),
                }),
        } => execute_power_scheme(services.power, scheme, apply, cli.auto_restore),
        Command::Power {
            command: Some(PowerCommand::Scheme { command: None }),
        } => execute_active_power_scheme(services.power),
        Command::Power {
            command: Some(PowerCommand::SaverOnce),
        } => execute_power_saver_once(&services, apply, cli.auto_restore),
        Command::Power {
            command: Some(PowerCommand::Ac),
        } => power_ac::execute_ac(services.battery, services.hal.platform()),
        Command::Power {
            command: Some(PowerCommand::Draw),
        } => power_ac::execute_draw(services.battery, services.hal.platform()),
        Command::Perf {
            command: Some(PerfCommand::Mode { mode }),
        } => execute_performance_mode(services.performance, mode, apply, cli.auto_restore),
        Command::Perf {
            command: Some(PerfCommand::Fan {
                command: Some(command),
            }),
        } => execute_fan(services.fan, command, apply, cli.auto_restore),
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
        } => execute_power_limit(
            services.tuning,
            PowerLimitKind::Pl1,
            watts,
            apply,
            cli.auto_restore,
        ),
        Command::Perf {
            command: Some(PerfCommand::Pl2 { watts }),
        } => execute_power_limit(
            services.tuning,
            PowerLimitKind::Pl2,
            watts,
            apply,
            cli.auto_restore,
        ),
        Command::Perf {
            command: Some(PerfCommand::Dbdc { command }),
        } => execute_dbdc(services.dbdc, command.unwrap_or(DbdcCommand::Get)),
        Command::Perf {
            command: Some(PerfCommand::Policy),
        } => execute_its_policy(services.its_policy),
        Command::Perf {
            command: Some(PerfCommand::Gpu { command }),
        } => execute_gpu_oc(services.gpu_oc, command.unwrap_or(GpuCommand::Status)),
        Command::Perf {
            command: Some(PerfCommand::Top),
        } => execute_process_top(services.process),
        Command::Perf {
            command: Some(PerfCommand::Boost { pids }),
        } => execute_process_policy(
            services.process,
            &pids,
            ProcessOperation::Boost,
            apply,
            cli.auto_restore,
        ),
        Command::Perf {
            command: Some(PerfCommand::Throttle { pids }),
        } => execute_process_policy(
            services.process,
            &pids,
            ProcessOperation::Throttle,
            apply,
            cli.auto_restore,
        ),
        Command::Audio { command } => execute_audio(
            services.audio,
            services.audio_endpoints,
            services.noise_cancel,
            command,
            apply,
            cli.auto_restore,
        ),
        Command::Sense { command } => {
            execute_sense(services.sense, command, apply, cli.auto_restore)
        }
        Command::Bios {
            command: Some(command),
        } => execute_bios(
            services.bios,
            command,
            apply,
            confirmed,
            interactive,
            cli.auto_restore,
        ),
        Command::Bios { command: None } => execute_bios_list(services.bios),
        Command::Firmware { command } => execute_firmware(services.firmware, command),
        Command::Storage { .. } => execute_storage(services.storage),
        Command::Usb {
            command: Some(UsbCommand::Ports),
        } => execute_usb_ports(services.usb_ports),
        Command::Usb { command } => execute_usb(
            services.usb,
            services.bios,
            command.ok_or_else(|| LctrlError::InvalidArgument {
                detail: "a usb subcommand is required".into(),
            })?,
            apply,
            confirmed,
            interactive,
            cli.auto_restore,
        ),
        Command::Kbd {
            command: Some(KbdCommand::Backlight { level, effect }),
        } => execute_backlight(services.keyboard, level, effect, apply, cli.auto_restore),
        Command::Kbd {
            command: Some(KbdCommand::Fnlock { state }),
        } => execute_fnlock(
            services.bios,
            state,
            apply,
            confirmed,
            interactive,
            cli.auto_restore,
        ),
        Command::Kbd {
            command: Some(KbdCommand::FnCtrlSwap { state }),
        } => execute_fn_ctrl_swap(
            services.bios,
            state,
            apply,
            confirmed,
            interactive,
            cli.auto_restore,
        ),
        Command::Touchpad { state } => {
            execute_touchpad(services.touchpad, state, apply, cli.auto_restore)
        }
        Command::Panel {
            command:
                Some(PanelCommand::Brightness {
                    percent,
                    expect_current,
                }),
        } => execute_panel_brightness(
            services.panel_brightness,
            percent,
            expect_current,
            apply,
            cli.auto_restore,
        ),
        Command::Panel {
            command: Some(PanelCommand::Info),
        } => execute_panel_info(services.panel_identity),
        Command::Panel {
            command: Some(command),
        } => execute_panel(
            services.panel,
            services.panel_features,
            command,
            apply,
            cli.auto_restore,
        ),
        Command::Display {
            command: Some(DisplayCommand::Status),
        } => execute_display_status(services.display_status, services.panel),
        Command::Privacy {
            command: Some(command),
        } => execute_privacy(
            services.privacy,
            services.bios,
            command,
            apply,
            confirmed,
            interactive,
            cli.auto_restore,
        ),
        Command::Magicbay {
            command: Some(MagicbayCommand::Detect),
        } => execute_magicbay_detect(services.magicbay),
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Status),
                }),
        } => execute_magicbay_lte_status(services.mobile_broadband),
        Command::Update {
            command: Some(command),
        } => execute_update(
            services.update,
            services.local_update,
            services.local_package,
            apply,
            command,
        ),
        Command::Scan {
            command: Some(command),
        } => execute_scan(services.diagnostics, command),
        Command::Snapshot {
            command: Some(command),
        } => execute_snapshot(&services, command, apply),
        Command::Wifi {
            command: Some(command),
        } => execute_wifi(services.wifi, services.wifi_status, command),
        Command::Bluetooth {
            command: Some(command),
        } => execute_bluetooth(services.bluetooth, command),
        Command::Radio {
            command: Some(command),
        } => execute_radio(services.radio, command, apply),
        Command::Sensor { command } => execute_sensor(services.sensor, command.as_ref()),
        Command::Security {
            command: Some(command),
        } => execute_security(services.security, command),
        Command::Tune {
            command: Some(TuneCommand::Pl1 { watts, pl2, tau }),
        } => execute_tune_power_limits(services.tuning, watts, pl2, tau, apply, cli.auto_restore),
        Command::Tune {
            command: Some(TuneCommand::Epp { value }),
        } => execute_tune_epp(services.tuning, &value, apply, cli.auto_restore),
        Command::Tune {
            command: Some(TuneCommand::Turbo { state }),
        } => execute_tune_turbo(services.processor_power, state, apply, cli.auto_restore),
        Command::Tune {
            command: Some(TuneCommand::Telemetry { .. }),
        } => execute_tune_telemetry(&services),
        Command::Tune {
            command: Some(TuneCommand::Restore),
        } => execute_tune_restore(&services, apply),
        Command::Tune {
            command:
                Some(TuneCommand::Profile {
                    command: Some(profile),
                }),
        } => execute_tune_profile(&services, profile, apply, cli.auto_restore),
        Command::Fix { command: None } => execute_fix_rules(services.machine_fix),
        Command::Fix {
            command: Some(FixCommand::Scan),
        } => execute_fix_scan(services.machine_fix),
        Command::Fix {
            command: Some(FixCommand::Run { issue }),
        } => execute_fix_run(services.machine_fix, &issue, apply),
        Command::Fix {
            command: Some(FixCommand::Track),
        } => execute_fix_track(services.machine_fix),
        Command::Scene { command: None } => execute_scene_list(services.scene),
        Command::Scene {
            command: Some(SceneCommand::List),
        } => execute_scene_list(services.scene),
        Command::Scene {
            command: Some(SceneCommand::Test { event, rule }),
        } => execute_scene_test(services.scene, &event, rule.as_deref(), apply),
        Command::Scene {
            command: Some(SceneCommand::Track),
        } => execute_scene_track(services.scene),
        Command::Osd { command } => execute_osd(services.osd, command, apply),
        Command::Hotkey { command } => execute_hotkey(services.hotkey, command, apply),
        Command::Completions { shell } => execute_completions(shell),
        command => Err(unsupported(command)),
    }
}
fn command_is_mutating(command: &Command) -> bool {
    matches!(
        command,
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { .. })
        } | Command::Battery {
            command: Some(BatteryCommand::NightCharge { .. }),
        } | Command::Power {
            command: Some(PowerCommand::Scheme {
                command: Some(SchemeCommand::Apply { .. } | SchemeCommand::Set { .. })
            })
        } | Command::Power {
            command: Some(PowerCommand::SaverOnce),
        } | Command::Perf {
            command: Some(
                PerfCommand::Mode { .. } | PerfCommand::Pl1 { .. } | PerfCommand::Pl2 { .. }
            )
        } | Command::Perf {
            command: Some(PerfCommand::Boost { .. } | PerfCommand::Throttle { .. })
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
        } | Command::Panel {
            command: Some(
                PanelCommand::Brightness {
                    percent: Some(_),
                    ..
                } | PanelCommand::Rate { .. }
                    | PanelCommand::Color { .. }
                    | PanelCommand::SuperResolution { .. }
                    | PanelCommand::Overdrive { .. }
                    | PanelCommand::EyeCare { .. },
            )
        } | Command::Kbd {
            command: Some(
                KbdCommand::Backlight { .. }
                    | KbdCommand::Fnlock { .. }
                    | KbdCommand::FnCtrlSwap { .. }
                    | KbdCommand::Winlock { .. },
            )
        } | Command::Touchpad {
            state: TouchpadAction::On | TouchpadAction::Off | TouchpadAction::Recover,
        } | Command::Radio {
            command: Some(RadioCommand::Off { .. } | RadioCommand::On { .. }),
        } | Command::Privacy {
            command: Some(
                PrivacyCommand::Cam {
                    action: PrivacyCamAction::On | PrivacyCamAction::Off,
                    ..
                } | PrivacyCommand::Mic { .. }
                    | PrivacyCommand::Fingerprint { .. },
            )
        } | Command::Bios {
            command: Some(
                BiosCommand::Set { .. }
                    | BiosCommand::Discard
                    | BiosCommand::Defaults { .. }
                    | BiosCommand::Password {
                        command: Some(PasswordCommand::Set { .. } | PasswordCommand::Clear { .. },),
                    },
            )
        } | Command::Audio {
            command: Some(
                AudioCommand::Volume {
                    percent: Some(_),
                    ..
                } | AudioCommand::Mute {
                    state: Some(_),
                    ..
                } | AudioCommand::Dolby { .. }
                    | AudioCommand::NoiseCancel { .. },
            )
        } | Command::Usb {
            // Both usb verbs write the off/battery charging gate; `--persistent`
            // additionally commits the BIOS `AlwaysOnUSB` setting.
            command: Some(UsbCommand::AlwaysOn { .. } | UsbCommand::ChargeOnBattery { .. }),
        } | Command::Sense {
            // `sense status` is the only read-only form.
            command: Some(
                SenseCommand::LockOnLeave { .. }
                    | SenseCommand::WakeOnApproach { .. }
                    | SenseCommand::PauseVideo { .. }
                    | SenseCommand::CameraAssist { .. }
                    | SenseCommand::ApproachAutoAdjust { .. }
                    | SenseCommand::LeaveAutoAdjust { .. }
                    | SenseCommand::AttentionTracking { .. }
                    | SenseCommand::KbdLightAuto { .. }
                    | SenseCommand::Reset,
            )
        } | Command::Osd {
            // `osd test`/`osd show` only paint our own overlay window; the two
            // persisted control-code writes are the mutating forms.
            command: Some(OsdCommand::Enable | OsdCommand::Disable),
        } | Command::Hotkey {
            // `list`/`status`/`trigger` observe; register/unregister change the
            // live hotkey table.
            command: Some(HotkeyCommand::Register { .. } | HotkeyCommand::Unregister { .. }),
        } | Command::Update {
            // `check`/`history` read; the refusing catalogue verbs never write.
            command: Some(
                UpdateCommand::LocalInstall { .. }
                    | UpdateCommand::LocalDriver {
                        command: LocalDriverCommand::Apply { .. },
                    }
                    | UpdateCommand::LocalDriver {
                        command: LocalDriverCommand::Rollback { .. },
                    },
            ),
        } | Command::Fix {
            // `scan`/`rules`/`track` observe; `run` repairs.
            command: Some(FixCommand::Run { .. }),
        } | Command::Scene {
            // `list`/`track` observe; `test` applies the firing rule.
            command: Some(SceneCommand::Test { .. }),
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
    match services.battery {
        Some(service) => checks.extend([
            doctor_check("battery.device", service.battery_telemetry(0)),
            doctor_check("battery.os", service.os_battery_snapshot()),
            doctor_check("battery.electrical", service.battery_electrical()),
            doctor_check("battery.system_power", service.system_power_status()),
        ]),
        None => checks.extend([
            doctor_missing("battery.device"),
            doctor_missing("battery.os"),
            doctor_missing("battery.electrical"),
            doctor_missing("battery.system_power"),
        ]),
    }
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
    let (adapter, adapter_error) = capture_optional(battery.adapter_info());
    let (system_power, system_power_error) = capture_optional(battery.system_power_status());
    if adapter.is_none() && system_power.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "adapter and aggregate power unavailable: adapter={adapter_error:?}; system_power={system_power_error:?}"
            ),
        });
    }
    let payload = serde_json::json!({
        "vendor_adapter": adapter, "vendor_adapter_error": adapter_error,
        "system_power": system_power, "system_power_error": system_power_error,
        "source_association": "unverified"
    });
    let human = format!(
        "adapter and aggregate system power (independent sources):\n{}\n",
        serde_json::to_string_pretty(&payload)
            .map_err(|error| LctrlError::Io(io::Error::other(error)))?
    );
    structured_output(&payload, human)
}

fn capture_optional<T>(result: lctrl_core::Result<T>) -> (Option<T>, Option<Value>) {
    match result {
        Ok(value) => (Some(value), None),
        Err(error) => (
            None,
            Some(serde_json::to_value(error.report()).expect("error reports are serializable")),
        ),
    }
}

fn execute_charge_mode(
    battery: Option<&dyn BatteryControl>,
    mode: ChargeModeArg,
    apply: ApplyMode,
    auto_restore: bool,
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
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_charge_mode(battery, *report.previous())?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format!(
            "charge mode: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
        restore.as_deref(),
    )
}

fn execute_night_charge(
    battery: Option<&dyn BatteryControl>,
    state: Toggle,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.night-charge".into(),
    })?;
    let enabled = matches!(state, Toggle::On);
    let report = battery.set_night_charge(enabled, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_night_charge(battery, *report.previous())?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format!("night charge: {}\n", report.requested()),
        restore.as_deref(),
    )
}

fn execute_temporary_charge(battery: Option<&dyn BatteryControl>) -> CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.temporary-mode".into(),
    })?;
    let state = battery.temporary_charge()?;
    structured_output(
        &state,
        format!(
            "temporary charge mode: raw={:#x} supported={} valid={} mode={} temporary={}\n",
            state.raw,
            state.supported,
            state.valid,
            state
                .mode
                .map_or_else(|| "unknown".to_string(), |mode| mode.to_string()),
            state.is_temporary()
        ),
    )
}

fn execute_performance_mode(
    performance: Option<&dyn PerformanceControl>,
    mode: PerfModeArg,
    apply: ApplyMode,
    auto_restore: bool,
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
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_performance_mode(performance, *report.previous())?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format!(
            "performance mode: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
        restore.as_deref(),
    )
}

fn restore_performance_mode(
    performance: &dyn PerformanceControl,
    previous: lctrl_core::PerformanceMode,
) -> lctrl_core::Result<String> {
    // set_performance_mode already verifies the overlay readback and rolls
    // back on mismatch; the richer performance_state() read is not needed
    // and on this model fails when optional registry values are absent.
    let restored = performance.set_performance_mode(previous, ApplyMode::Commit)?;
    match restored.actual() {
        Some(actual) if *actual == previous => Ok(format!("restored performance mode: {previous}")),
        actual => Err(LctrlError::FirmwareRejected {
            detail: format!(
                "performance-mode restore mismatch: expected {previous}, actual {actual:?}"
            ),
        }),
    }
}

fn execute_power_limit(
    tuning: Option<&dyn TuningControl>,
    kind: PowerLimitKind,
    watts: u32,
    apply: ApplyMode,
    auto_restore: bool,
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
    let restore = if auto_restore && apply == ApplyMode::Commit {
        let restored = tuning.set_power_limit(kind, *report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored power limit {kind:?}: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        format!("power limit {kind:?}: {watts} W\n"),
        restore.as_deref(),
    )
}

fn execute_tune_power_limits(
    tuning: Option<&dyn TuningControl>,
    pl1_watts: u32,
    pl2_watts: Option<u32>,
    tau_seconds: Option<u32>,
    apply: ApplyMode,
    auto_restore: bool,
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
    let mut current_tau = None;
    if let Some(tau_seconds) = tau_seconds {
        if auto_restore {
            // Capture before apply: reading Tau during restore would return
            // the value this transaction just wrote.
            current_tau = Some(tuning.power_limit(PowerLimitKind::Tau)?);
        }
        requests.push((PowerLimitKind::Tau, u64::from(tau_seconds) * 1_000_000));
    }
    let reports = apply_limit_sequence(tuning, &requests, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        // Reverse the apply order: lower the ceiling before the floor so a
        // mid-restore failure can never leave PL1 > PL2.
        let mut restored = Vec::new();
        for (kind, _) in requests.iter().rev() {
            let previous = match kind {
                PowerLimitKind::Pl1 => current_pl1,
                PowerLimitKind::Pl2 => current_pl2,
                PowerLimitKind::Tau => current_tau.ok_or_else(|| LctrlError::FirmwareRejected {
                    detail: "tau restore snapshot missing".into(),
                })?,
            };
            tuning.set_power_limit(*kind, previous, ApplyMode::Commit)?;
            restored.push(format!("{kind:?}={}", previous / 1_000_000));
        }
        Some(format!("restored power limits: {}\n", restored.join(", ")))
    } else {
        None
    };
    output_with_restore(
        &reports,
        "RAPL power limits applied and read back\n".into(),
        restore.as_deref(),
    )
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

/// Number of processes reported by `vantage perf top`, matching the original
/// scheduler's top-ten process view.
const PROCESS_TOP_DEFAULT_LIMIT: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessOperation {
    Boost,
    Throttle,
}

impl ProcessOperation {
    const fn feature(self) -> &'static str {
        match self {
            Self::Boost => "perf.boost",
            Self::Throttle => "perf.throttle",
        }
    }

    const fn policy(self) -> ProcessPolicy {
        match self {
            Self::Boost => ProcessPolicy::boost(),
            Self::Throttle => ProcessPolicy::throttle(),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Boost => "boosted",
            Self::Throttle => "throttled",
        }
    }
}

fn execute_process_top(process: Option<&dyn ProcessControl>) -> CommandResult {
    let process = process.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.top".into(),
    })?;
    let inventory = process.top_processes(PROCESS_TOP_DEFAULT_LIMIT)?;
    let human = format!(
        "Top {} processes by interval CPU time ({} skipped):\n{}",
        inventory.processes.len(),
        inventory.skipped_processes,
        serde_json::to_string_pretty(&inventory).map_err(json_error)?
    );
    structured_output(&inventory, human)
}

/// Apply one process policy to every requested PID. Each target is resolved to
/// its creation-time identity before any write, and a failure after a partial
/// sequence restores the targets that were already changed.
fn execute_process_policy(
    process: Option<&dyn ProcessControl>,
    pids: &[u32],
    operation: ProcessOperation,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let process = process.ok_or_else(|| LctrlError::Unsupported {
        feature: operation.feature().into(),
    })?;
    if pids.is_empty() {
        return Err(LctrlError::InvalidArgument {
            detail: "process policy control requires at least one PID".into(),
        });
    }
    let mut changes = Vec::new();
    let mut applied: Vec<(ProcessTarget, ProcessSnapshot)> = Vec::new();
    for pid in pids {
        let target = process.resolve_target(*pid)?;
        let snapshot = process.process_snapshot(&target)?;
        let report = match operation {
            ProcessOperation::Boost => process.boost_process(&target, operation.policy(), apply)?,
            ProcessOperation::Throttle => {
                process.throttle_process(&target, operation.policy(), apply)?
            }
        };
        changes.push(serde_json::json!({
            "pid": target.identity().pid,
            "image_name": target.image_name(),
            "previous": report.previous(),
            "requested": report.requested(),
            "actual": report.actual(),
            "applied": matches!(apply, ApplyMode::Commit),
        }));
        if matches!(apply, ApplyMode::Commit) {
            applied.push((target, snapshot));
        }
    }
    let mut restore = None;
    if matches!(apply, ApplyMode::Commit) && auto_restore {
        restore_processes(process, applied)?;
        restore = Some(format!(
            "restored {} process(es) to their pre-policy state\n",
            changes.len()
        ));
    }
    output_with_restore(
        &changes,
        format!(
            "{} {} process(es):\n{}",
            operation.label(),
            changes.len(),
            serde_json::to_string_pretty(&changes).map_err(json_error)?
        ),
        restore.as_deref(),
    )
}

fn json_error(error: serde_json::Error) -> LctrlError {
    LctrlError::Io(std::io::Error::other(error))
}

fn restore_processes(
    process: &dyn ProcessControl,
    applied: Vec<(ProcessTarget, ProcessSnapshot)>,
) -> lctrl_core::Result<()> {
    for (target, snapshot) in applied.into_iter().rev() {
        process.restore_process(&target, &snapshot, ApplyMode::Commit)?;
    }
    Ok(())
}

fn execute_tune_epp(
    tuning: Option<&dyn TuningControl>,
    value: &str,
    apply: ApplyMode,
    auto_restore: bool,
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
    let restore = if auto_restore && apply == ApplyMode::Commit {
        let restored = tuning.set_epp(*report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored EPP: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        format!("EPP requested={value}\n"),
        restore.as_deref(),
    )
}

fn execute_tune_turbo(
    power: Option<&dyn ProcessorPowerControl>,
    state: Toggle,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let power = power.ok_or_else(|| LctrlError::Unsupported {
        feature: "tune.turbo".into(),
    })?;
    let enabled = match state {
        Toggle::On => true,
        Toggle::Off => false,
    };
    let report = power.set_turbo(enabled, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        // The report carries the exact per-source boost snapshot; replaying
        // set_turbo(previous bool) would collapse aggressive/efficiency
        // variants the snapshot preserved.
        let restored = power.set_boost_ac_dc(*report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored turbo: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        format!(
            "processor boost policy {} requested; this is an OS policy, not a hardware-frequency guarantee\n",
            if enabled { "enabled" } else { "disabled" }
        ),
        restore.as_deref(),
    )
}

fn execute_tune_telemetry(services: &CommandServices<'_>) -> CommandResult {
    let mut telemetry = serde_json::Map::new();
    if let Some(tuning) = services.tuning {
        telemetry.insert(
            "epp".into(),
            serialize_change(&capture_optional(tuning.epp()))?,
        );
        for (name, kind) in [
            ("pl1_uw", PowerLimitKind::Pl1),
            ("pl2_uw", PowerLimitKind::Pl2),
            ("tau_us", PowerLimitKind::Tau),
        ] {
            telemetry.insert(
                name.into(),
                serialize_change(&capture_optional(tuning.power_limit(kind)))?,
            );
        }
    }
    if let Some(battery) = services.battery {
        telemetry.insert(
            "device_battery".into(),
            serialize_change(&capture_optional(battery.battery_telemetry(0)))?,
        );
        telemetry.insert(
            "os_battery".into(),
            serialize_change(&capture_optional(battery.os_battery_snapshot()))?,
        );
        telemetry.insert(
            "battery_electrical".into(),
            serialize_change(&capture_optional(battery.battery_electrical()))?,
        );
        telemetry.insert(
            "system_power".into(),
            serialize_change(&capture_optional(battery.system_power_status()))?,
        );
    }
    if let Some(performance) = services.performance {
        telemetry.insert(
            "performance".into(),
            serialize_change(&capture_optional(performance.performance_state()))?,
        );
    }
    let temperatures = match services.temperature {
        Some(temperature) => temperature.temperature_sensors(),
        None => Err(LctrlError::Unsupported {
            feature: "perf.temp".into(),
        }),
    };
    telemetry.insert(
        "temperatures".into(),
        serialize_change(&capture_optional(temperatures))?,
    );

    let mut human = String::from("tuning telemetry:\n");
    for (name, observation) in &telemetry {
        if let Some(error) = observation.get(1).filter(|value| !value.is_null()) {
            writeln!(&mut human, "  {name}: unavailable ({error})")
                .expect("writing to a string cannot fail");
        } else if let Some(value) = observation.get(0) {
            writeln!(&mut human, "  {name}: {value}").expect("writing to a string cannot fail");
        }
    }
    Ok(CommandOutput {
        human,
        json: Value::Object(telemetry),
    })
}

fn execute_temperature_list(temperature: Option<&dyn TemperatureControl>) -> CommandResult {
    let temperature = temperature.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.temp".into(),
    })?;
    let sensors = temperature.temperature_sensors()?;
    let mut human = String::new();
    if sensors.is_empty() {
        human.push_str("no temperature sensors reported\n");
    }
    for sensor in &sensors {
        if let Some(value) = sensor.value_c {
            write!(&mut human, "{}: {value:.1}°C", sensor.metadata.name)
                .expect("writing to a string cannot fail");
        } else {
            write!(&mut human, "{}: unavailable", sensor.metadata.name)
                .expect("writing to a string cannot fail");
        }
        writeln!(
            &mut human,
            " (id: {}; source: {:?}; location: {:?}; availability: {})",
            sensor.metadata.id,
            sensor.metadata.source,
            sensor.metadata.location,
            availability_label(sensor.metadata.availability),
        )
        .expect("writing to a string cannot fail");
    }
    structured_output(&sensors, human)
}

fn execute_fan_status(fan: Option<&dyn FanControl>) -> CommandResult {
    let fan = fan.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.fan".into(),
    })?;
    let (mode, mode_error) = capture_optional(fan.fan_mode());
    // An empty inventory record is the same answer the capability probe
    // records as `perf.fan.inventory` unavailable (windows_hal.rs
    // `record_fan_capabilities`), so it is an unavailable sub-read here rather
    // than a successful "0 fan(s)" count. A failed read is never flattened
    // into an empty list.
    let (fans, fans_error) = capture_optional(fan.fans().and_then(|fans| {
        if fans.is_empty() {
            Err(LctrlError::Unsupported {
                feature: "perf.fan.inventory: firmware exposed no fan inventory record".into(),
            })
        } else {
            Ok(fans)
        }
    }));
    if mode.is_none() && fans.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "perf.fan: mode and inventory both unavailable: mode={mode_error:?}; inventory={fans_error:?}"
            ),
        });
    }
    let error_detail = |error: &Option<Value>| {
        error
            .as_ref()
            .map_or_else(|| "no detail recorded".to_string(), ToString::to_string)
    };
    let human = format!(
        "{}\n{}\n",
        match (&mode, &mode_error) {
            (Some(mode), _) => format!("fan mode: {mode}"),
            (None, error) => format!("fan mode: unavailable ({})", error_detail(error)),
        },
        match (&fans, &fans_error) {
            (Some(fans), _) => format!("{} fan device record(s)", fans.len()),
            (None, error) => format!("fan inventory: unavailable ({})", error_detail(error)),
        }
    );
    structured_output(
        &serde_json::json!({
            "mode": mode,
            "mode_error": mode_error,
            "fans": fans,
            "fans_error": fans_error,
        }),
        human,
    )
}

fn execute_temperature(
    temperature: Option<&dyn TemperatureControl>,
    command: TempCommand,
) -> CommandResult {
    match command {
        TempCommand::Read { id } => {
            let temperature = temperature.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.temp".into(),
            })?;
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
        TempCommand::Watch { .. } => Err(streaming_command_error(StreamKind::Temperatures)),
    }
}

fn execute_dbdc(dbdc: Option<&dyn DbdcControl>, command: DbdcCommand) -> CommandResult {
    match command {
        DbdcCommand::Get => {
            let dbdc = dbdc.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.dbdc".into(),
            })?;
            let profile = dbdc.dbdc_profile()?;
            let mut text = String::new();
            for tier in 0..profile.counts as usize {
                let threshold = profile
                    .threshold_pct
                    .get(tier)
                    .map_or_else(|| "unknown".to_string(), |value| value.to_string());
                let limit = profile
                    .current_limit_ma
                    .get(tier)
                    .map_or_else(|| "unknown".to_string(), |value| value.to_string());
                let power = profile
                    .ros_power_mw
                    .get(tier)
                    .map_or_else(|| "unknown".to_string(), |value| value.to_string());
                let _ = writeln!(
                    text,
                    "tier {tier}: <= {threshold}% battery: discharge limit {limit} mA, system power {power} mW"
                );
            }
            structured_output(&profile, text)
        }
    }
}

fn execute_its_policy(policy: Option<&dyn ItsPolicyControl>) -> CommandResult {
    let policy = policy.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.policy".into(),
    })?;
    let policy = policy.its_policy()?;
    structured_output(&policy, format_its_policy(&policy))
}

/// Human rendering of the decoded policy.
///
/// Interpreted values always sit beside the raw one, an absent value says so
/// instead of disappearing, and a value the decode refused prints the raw text
/// plus the reason.
fn format_its_policy(policy: &lctrl_core::ItsPolicy) -> String {
    let mut text = String::from(
        "dispatcher policy (read-only, PowerSlider key; opaque tables reported verbatim)\n",
    );
    match &policy.plx_set_mode {
        Some(mode) => {
            let _ = writeln!(
                text,
                "  plx-set-mode: raw=0x{:08x} ac={} dc={} (AC reads the low lane, DC the high lane)",
                mode.raw, mode.ac, mode.dc
            );
        }
        None => text.push_str("  plx-set-mode: not published\n"),
    }
    match policy.fan_speed_address {
        Some(address) => {
            let _ = writeln!(
                text,
                "  fan-speed-address: {address} (0x{address:08x}); no dispatcher reader located"
            );
        }
        None => text.push_str("  fan-speed-address: not published\n"),
    }
    match &policy.ap_min_std_fan {
        Some(fan) => {
            let _ = writeln!(
                text,
                "  ap-min-std-fan: enable={} gpu={} cpu={} (thresholds clamp to 0..=100, 50 when \
                 unreadable)",
                u8::from(fan.enable),
                fan.gpu_threshold_pct,
                fan.cpu_threshold_pct
            );
        }
        None => text.push_str("  ap-min-std-fan: not published\n"),
    }
    match &policy.turbo_value {
        Some(turbo) => {
            let _ = writeln!(
                text,
                "  turbo-value: raw={} default_epp={} (0xff means \"use the default EPP\")",
                turbo.raw,
                u8::from(turbo.default_epp)
            );
        }
        None => text.push_str("  turbo-value: not published\n"),
    }
    match &policy.turbo_time {
        Some(time) => {
            let _ = writeln!(
                text,
                "  turbo-time: raw=0x{:08x} turbo_time={} byte0={} byte1={} byte3={} \
                 (turbo_time is max'd with epp-turbo's)",
                time.raw, time.turbo_time, time.byte0, time.byte1, time.byte3
            );
        }
        None => text.push_str("  turbo-time: not published\n"),
    }
    match &policy.epp_turbo {
        Some(turbo) => {
            let _ = writeln!(
                text,
                "  epp-turbo: raw=0x{:08x} mode_nibble={} turbo_time={} not_turbo_to_apm={}",
                turbo.raw,
                turbo.mode_nibble,
                turbo.turbo_time,
                u8::from(turbo.not_turbo_to_apm)
            );
        }
        None => text.push_str("  epp-turbo: not published\n"),
    }
    for (label, table) in [
        ("power-bucket", &policy.power_bucket),
        ("cppc", &policy.cppc),
        ("cfqc", &policy.cfqc),
        ("cfqc1", &policy.cfqc1),
    ] {
        match table {
            Some(table) => {
                let pairs = table
                    .pairs()
                    .iter()
                    .map(|(x, y)| format!("{x}:{y}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let _ = writeln!(
                    text,
                    "  {label}: {}/{} slot(s) {pairs}",
                    table.slots.len(),
                    table.capacity
                );
            }
            None => {
                let _ = writeln!(text, "  {label}: not published");
            }
        }
    }
    for issue in &policy.issues {
        match &issue.raw {
            Some(raw) => {
                let _ = writeln!(
                    text,
                    "  decode issue: {} = {:?}: {}",
                    issue.value, raw, issue.detail
                );
            }
            None => {
                let _ = writeln!(text, "  decode issue: {}: {}", issue.value, issue.detail);
            }
        }
    }
    text
}

fn execute_gpu_oc(gpu_oc: Option<&dyn GpuOcControl>, command: GpuCommand) -> CommandResult {
    match command {
        GpuCommand::Status => {
            let gpu_oc = gpu_oc.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.gpu".into(),
            })?;
            let status = gpu_oc.gpu_oc_status()?;
            let mut text = format!(
                "GPU overclock: {}\n",
                if status.supported {
                    "supported by the firmware"
                } else {
                    "not supported by the firmware"
                }
            );
            let _ = writeln!(
                text,
                "  probe {}: ReturnValue={}, Data={}",
                status.probe.method, status.probe.return_value, status.probe.data
            );
            match &status.parameter {
                Some(parameter) => {
                    let _ = writeln!(
                        text,
                        "  OC parameter block: clock_id={} pstate_id={} offset={} scale={} range={}..={} default={} interval={} capability={}",
                        parameter.clock_id,
                        parameter.pstate_id,
                        parameter.oc_offset_freq,
                        parameter.oc_offset_scale,
                        parameter.oc_min_offset,
                        parameter.oc_max_offset,
                        parameter.default_value,
                        parameter.interval,
                        parameter.capability
                    );
                }
                None => {
                    let _ = writeln!(
                        text,
                        "  OC parameter block: unavailable ({})",
                        status
                            .parameter_error
                            .as_deref()
                            .unwrap_or("the firmware published no instance")
                    );
                }
            }
            let _ = writeln!(
                text,
                "  write: unsupported (no reversible OC set with readback)"
            );
            structured_output(&status, text)
        }
    }
}

/// Put the state a write replaced back, then read it back to prove the
/// restore took effect. A restore that does not read back as the captured
/// state is reported instead of being papered over.
fn restore_dolby_state(
    audio: &dyn AudioControl,
    previous: &DolbyState,
) -> lctrl_core::Result<String> {
    // The endpoint keeps the last selected profile when Dolby is switched
    // off, so the profile is selected first and the enable bit afterwards.
    audio.set_dolby_profile(previous.active_profile, ApplyMode::Commit)?;
    if !previous.enabled {
        audio.set_dolby_profile(DolbyProfile::Off, ApplyMode::Commit)?;
    }
    let actual = audio.dolby_state()?;
    let profile_matches = previous.active_profile == DolbyProfile::Off
        || actual.active_profile == previous.active_profile;
    if actual.enabled != previous.enabled || !profile_matches {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "dolby restore mismatch: expected enabled={} profile={:?}, actual enabled={} profile={:?}",
                previous.enabled, previous.active_profile, actual.enabled, actual.active_profile
            ),
        });
    }
    Ok(format!(
        "restored dolby: enabled={} profile={:?}",
        actual.enabled, actual.active_profile
    ))
}

fn restore_noise_cancel_state(
    noise_cancel: &dyn NoiseCancelControl,
    previous: &NoiseCancelState,
) -> lctrl_core::Result<String> {
    noise_cancel.set_noise_cancel_mode(previous.mic_mode, ApplyMode::Commit)?;
    let actual = noise_cancel.noise_cancel_state()?;
    if actual.mic_mode != previous.mic_mode {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "noise-cancel restore mismatch: expected mic mode {:?}, actual {:?}",
                previous.mic_mode, actual.mic_mode
            ),
        });
    }
    Ok(format!(
        "restored noise-cancel: mic mode {:?}",
        actual.mic_mode
    ))
}

fn restore_charge_mode(
    battery: &dyn BatteryControl,
    previous: ChargeMode,
) -> lctrl_core::Result<String> {
    battery.set_charge_mode(previous, ApplyMode::Commit)?;
    let actual = battery.charge_mode()?;
    let matches = matches!(
        (actual, previous),
        (ChargeModeActual::Normal, ChargeMode::Normal)
            | (ChargeModeActual::Conservation, ChargeMode::Conservation)
            | (ChargeModeActual::Rapid, ChargeMode::Rapid)
    );
    if !matches {
        return Err(LctrlError::FirmwareRejected {
            detail: format!("charge-mode restore mismatch: expected {previous}, actual {actual}"),
        });
    }
    Ok(format!("restored charge mode: {actual}"))
}

fn restore_night_charge(
    battery: &dyn BatteryControl,
    previous: bool,
) -> lctrl_core::Result<String> {
    battery.set_night_charge(previous, ApplyMode::Commit)?;
    let actual = battery.night_charge()?;
    if actual.enabled != previous {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "night-charge restore mismatch: expected enabled={previous}, actual enabled={}",
                actual.enabled
            ),
        });
    }
    Ok(format!("restored night charge: enabled={}", actual.enabled))
}

fn restore_usb_ac(usb: &dyn UsbControl, previous: bool) -> lctrl_core::Result<String> {
    usb.set_usb_ac_charging(previous, ApplyMode::Commit)?;
    let actual = usb.usb_charging()?;
    if actual.ac_enabled != previous {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "USB AC charging restore mismatch: expected enabled={previous}, actual enabled={}",
                actual.ac_enabled
            ),
        });
    }
    Ok(format!(
        "restored USB AC charging: enabled={}",
        actual.ac_enabled
    ))
}

fn restore_usb_battery(usb: &dyn UsbControl, previous: bool) -> lctrl_core::Result<String> {
    usb.set_usb_battery_charging(previous, ApplyMode::Commit)?;
    let actual = usb.usb_charging()?;
    if actual.battery_enabled != previous {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "USB battery charging restore mismatch: expected enabled={previous}, actual enabled={}",
                actual.battery_enabled
            ),
        });
    }
    Ok(format!(
        "restored USB battery charging: enabled={}",
        actual.battery_enabled
    ))
}

/// Report a write together with the restore that followed it.
fn output_with_restore<T: Serialize>(
    report: &T,
    human: String,
    restore: Option<&str>,
) -> CommandResult {
    match restore {
        Some(text) => {
            let payload = serde_json::json!({ "change": report, "auto_restore": text });
            structured_output(&payload, format!("{human}{text}\n"))
        }
        None => structured_output(report, human),
    }
}

fn execute_audio(
    audio: Option<&dyn AudioControl>,
    audio_endpoints: Option<&dyn AudioEndpointControl>,
    noise_cancel: Option<&dyn NoiseCancelControl>,
    command: Option<AudioCommand>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    match command {
        Some(AudioCommand::Dolby { profile }) => execute_dolby(audio, profile, apply, auto_restore),
        Some(AudioCommand::Endpoints { all }) => {
            execute_audio_endpoints(audio_endpoints, all)
        }
        Some(AudioCommand::Volume { percent, device }) => execute_audio_volume(
            audio_endpoints,
            device.as_deref(),
            percent,
            apply,
            auto_restore,
        ),
        Some(AudioCommand::Mute { state, device }) => execute_audio_mute(
            audio_endpoints,
            device.as_deref(),
            state,
            apply,
            auto_restore,
        ),
        Some(AudioCommand::NoiseCancel { mode }) => {
            execute_noise_cancel(noise_cancel, mode, apply, auto_restore)
        }
        None => Err(LctrlError::InvalidArgument {
            detail: "an audio subcommand is required".into(),
        }),
    }
}

fn audio_endpoint_service<'a>(
    audio_endpoints: Option<&'a dyn AudioEndpointControl>,
    feature: &str,
) -> lctrl_core::Result<&'a dyn AudioEndpointControl> {
    audio_endpoints.ok_or_else(|| LctrlError::Unsupported {
        feature: feature.to_string(),
    })
}

fn execute_audio_endpoints(
    audio_endpoints: Option<&dyn AudioEndpointControl>,
    all: bool,
) -> CommandResult {
    let audio_endpoints = audio_endpoint_service(audio_endpoints, "audio.endpoints")?;
    let endpoints = audio_endpoints.endpoints(all)?;
    if endpoints.is_empty() {
        return Err(LctrlError::ChannelUnavailable {
            channel: if all {
                "the MMDevice API reported no audio endpoints".into()
            } else {
                "the MMDevice API reported no active audio endpoints".into()
            },
        });
    }
    let mut text = String::new();
    for endpoint in &endpoints {
        let _ = writeln!(text, "{endpoint}");
    }
    structured_output(&endpoints, text)
}

fn execute_audio_volume(
    audio_endpoints: Option<&dyn AudioEndpointControl>,
    device: Option<&str>,
    percent: Option<u8>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let audio_endpoints = audio_endpoint_service(audio_endpoints, "audio.volume")?;
    let Some(percent) = percent else {
        let state = audio_endpoints.endpoint_volume(device)?;
        return structured_output(&state, format!("{state}\n"));
    };
    let report = audio_endpoints.set_endpoint_volume(device, percent, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_endpoint_volume(
            audio_endpoints,
            device,
            report.previous(),
        )?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format_endpoint_volume_change(&report),
        restore.as_deref(),
    )
}

fn execute_audio_mute(
    audio_endpoints: Option<&dyn AudioEndpointControl>,
    device: Option<&str>,
    state: Option<MuteArg>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let audio_endpoints = audio_endpoint_service(audio_endpoints, "audio.mute")?;
    let Some(state) = state else {
        let current = audio_endpoints.endpoint_volume(device)?;
        return structured_output(&current, format!("{current}\n"));
    };
    let mute = matches!(state, MuteArg::On);
    let report = audio_endpoints.set_endpoint_mute(device, mute, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_endpoint_mute(
            audio_endpoints,
            device,
            report.previous(),
        )?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format_endpoint_mute_change(&report),
        restore.as_deref(),
    )
}

fn format_endpoint_volume_change(
    report: &lctrl_core::ChangeReport<lctrl_core::EndpointVolume>,
) -> String {
    let mode = if report.mode() == ApplyMode::DryRun {
        "dry run"
    } else {
        "applied"
    };
    let mut text = format!(
        "volume ({mode}): previous {}, requested {}%, actual {}",
        report.previous(),
        report.requested().percent,
        report
            .actual()
            .map_or_else(|| "unread".to_string(), |actual| format!("{}%", actual.percent)),
    );
    text.push('\n');
    text
}

fn format_endpoint_mute_change(
    report: &lctrl_core::ChangeReport<lctrl_core::EndpointVolume>,
) -> String {
    let mode = if report.mode() == ApplyMode::DryRun {
        "dry run"
    } else {
        "applied"
    };
    let mute = |value: bool| if value { "on" } else { "off" };
    let mut text = format!(
        "mute ({mode}): previous {}, requested {}, actual {}",
        report.previous(),
        mute(report.requested().mute),
        report
            .actual()
            .map_or_else(|| "unread".to_string(), |actual| mute(actual.mute).to_string()),
    );
    text.push('\n');
    text
}

/// Put the volume a write replaced back and prove the readback. A restore that
/// does not read back within the endpoint's step tolerance is reported, not
/// papered over.
fn restore_endpoint_volume(
    audio_endpoints: &dyn AudioEndpointControl,
    device: Option<&str>,
    previous: &lctrl_core::EndpointVolume,
) -> lctrl_core::Result<String> {
    audio_endpoints.set_endpoint_volume(device, previous.percent, ApplyMode::Commit)?;
    let actual = audio_endpoints.endpoint_volume(device)?;
    let tolerance = lctrl_core::volume_readback_tolerance_percent(previous.steps);
    if actual.percent.abs_diff(previous.percent) > tolerance {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "volume restore mismatch: expected {}%, actual {}%",
                previous.percent, actual.percent
            ),
        });
    }
    Ok(format!("restored volume: {}%", actual.percent))
}

fn restore_endpoint_mute(
    audio_endpoints: &dyn AudioEndpointControl,
    device: Option<&str>,
    previous: &lctrl_core::EndpointVolume,
) -> lctrl_core::Result<String> {
    audio_endpoints.set_endpoint_mute(device, previous.mute, ApplyMode::Commit)?;
    let actual = audio_endpoints.endpoint_volume(device)?;
    if actual.mute != previous.mute {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "mute restore mismatch: expected mute={}, actual mute={}",
                previous.mute, actual.mute
            ),
        });
    }
    Ok(format!("restored mute: {}", if actual.mute { "on" } else { "off" }))
}

fn execute_noise_cancel(
    noise_cancel: Option<&dyn NoiseCancelControl>,
    mode: NoiseCancelArg,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let noise_cancel = noise_cancel.ok_or_else(|| LctrlError::Unsupported {
        feature: "audio.noise-cancel".into(),
    })?;
    let target = match mode {
        NoiseCancelArg::Off => NoiseCancellationMode::Off,
        NoiseCancelArg::Single => NoiseCancellationMode::Single,
        NoiseCancelArg::Shared => NoiseCancellationMode::Shared,
        NoiseCancelArg::Spatial => NoiseCancellationMode::Spatial,
        NoiseCancelArg::VoiceId => NoiseCancellationMode::VoiceId,
    };
    let report = noise_cancel.set_noise_cancel_mode(target, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_noise_cancel_state(noise_cancel, report.previous())?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format_noise_cancel_change(&report),
        restore.as_deref(),
    )
}

fn format_noise_cancel_change(report: &lctrl_core::ChangeReport<NoiseCancelState>) -> String {
    let mode = if report.mode() == ApplyMode::DryRun {
        "dry run"
    } else {
        "applied"
    };
    let mut text = format!(
        "noise-cancel ({mode}): previous {}, requested mic mode={:?}",
        report.previous(),
        report.requested().mic_mode,
    );
    if let Some(actual) = report.actual() {
        text.push_str(&format!(", actual {actual}"));
    }
    text.push('\n');
    text
}

fn execute_dolby(
    audio: Option<&dyn AudioControl>,
    profile: DolbyProfileArg,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let audio = audio.ok_or_else(|| LctrlError::Unsupported {
        feature: "audio.dolby".into(),
    })?;
    let target = match profile {
        DolbyProfileArg::Off => DolbyProfile::Off,
        DolbyProfileArg::Movie => DolbyProfile::Movie,
        DolbyProfileArg::Music => DolbyProfile::Music,
        DolbyProfileArg::Voice => DolbyProfile::Voice,
        DolbyProfileArg::Game => DolbyProfile::Game,
        DolbyProfileArg::Personalize => DolbyProfile::Personalize,
        DolbyProfileArg::Dynamic => DolbyProfile::Dynamic,
    };
    let report = audio.set_dolby_profile(target, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        Some(restore_dolby_state(audio, report.previous())?)
    } else {
        None
    };
    output_with_restore(&report, format_dolby_change(&report), restore.as_deref())
}

fn format_dolby_change(report: &lctrl_core::ChangeReport<DolbyState>) -> String {
    let mode = if report.mode() == ApplyMode::DryRun {
        "dry run"
    } else {
        "applied"
    };
    let mut text = format!(
        "dolby ({mode}): previous enabled={} profile={:?}, requested enabled={} profile={:?}",
        report.previous().enabled,
        report.previous().active_profile,
        report.requested().enabled,
        report.requested().active_profile,
    );
    if let Some(actual) = report.actual() {
        text.push_str(&format!(
            ", actual enabled={} profile={:?}",
            actual.enabled, actual.active_profile
        ));
    }
    text.push('\n');
    text
}

fn execute_sense(
    sense: Option<&dyn SenseControl>,
    command: Option<SenseCommand>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    match command {
        Some(SenseCommand::Status) => {
            let sense = sense.ok_or_else(|| LctrlError::Unsupported {
                feature: "sense.status".into(),
            })?;
            let state = sense.sense_state()?;
            structured_output(&state, format!("smartsense: {state}\n"))
        }
        Some(SenseCommand::LockOnLeave {
            state,
            distance,
            wait,
        }) => {
            let sense = required_sense(sense, "sense.lock-on-leave")?;
            let mut features = vec![SenseFeature::LockOnLeave(state == Toggle::On)];
            if let Some(raw) = distance {
                features.push(SenseFeature::LeaveDistance(sense_distance(raw)?));
            }
            if let Some(raw) = wait {
                features.push(SenseFeature::LeaveWait(sense_wait(raw)?));
            }
            apply_sense_features(sense, &features, apply, auto_restore)
        }
        Some(SenseCommand::WakeOnApproach { state, distance }) => {
            let sense = required_sense(sense, "sense.wake-on-approach")?;
            let mut features = vec![SenseFeature::WakeOnApproach(state == Toggle::On)];
            if let Some(raw) = distance {
                features.push(SenseFeature::ApproachDistance(sense_distance(raw)?));
            }
            apply_sense_features(sense, &features, apply, auto_restore)
        }
        Some(SenseCommand::PauseVideo { state }) => {
            let sense = required_sense(sense, "sense.pause-video")?;
            apply_sense_features(
                sense,
                &[SenseFeature::PauseVideo(state == Toggle::On)],
                apply,
                auto_restore,
            )
        }
        Some(SenseCommand::CameraAssist { state }) => {
            let sense = required_sense(sense, "sense.camera-assist")?;
            apply_sense_features(
                sense,
                &[SenseFeature::CameraAssist(state == Toggle::On)],
                apply,
                auto_restore,
            )
        }
        Some(SenseCommand::ApproachAutoAdjust { state }) => {
            let sense = required_sense(sense, "sense.approach-auto-adjust")?;
            apply_sense_features(
                sense,
                &[SenseFeature::ApproachAutoAdjust(state == Toggle::On)],
                apply,
                auto_restore,
            )
        }
        Some(SenseCommand::LeaveAutoAdjust { state }) => {
            let sense = required_sense(sense, "sense.leave-auto-adjust")?;
            apply_sense_features(
                sense,
                &[SenseFeature::LeaveAutoAdjust(state == Toggle::On)],
                apply,
                auto_restore,
            )
        }
        Some(SenseCommand::AttentionTracking {
            state,
            dim,
            ac_only,
        }) => {
            if dim || ac_only {
                return Err(LctrlError::Unsupported {
                    feature: "sense.attention-tracking options: --dim/--ac-only have no recovered persistence mapping on this model".into(),
                });
            }
            let sense = required_sense(sense, "sense.attention-tracking")?;
            apply_sense_features(
                sense,
                &[SenseFeature::AttentionTracking(state == Toggle::On)],
                apply,
                auto_restore,
            )
        }
        Some(SenseCommand::KbdLightAuto { .. }) => Err(LctrlError::Unsupported {
            feature: "sense.kbd-light-auto: automatic keyboard backlight has no consuming service on this model: ThinkSmartSenseAddin 1.0.0.77 targets LPlatSvc/LITSSvc registry parameters, and both services are absent on 21VG (docs/interfaces.md §13.3)"
                .into(),
        }),
        Some(SenseCommand::Reset) => {
            let sense = required_sense(sense, "sense.reset")?;
            apply_sense_features(sense, &[SenseFeature::ResetAll], apply, auto_restore)
        }
        None => Err(LctrlError::InvalidArgument {
            detail: "a sense subcommand is required".into(),
        }),
    }
}

fn required_sense<'a>(
    sense: Option<&'a dyn SenseControl>,
    feature: &str,
) -> lctrl_core::Result<&'a dyn SenseControl> {
    sense.ok_or_else(|| LctrlError::Unsupported {
        feature: feature.to_string(),
    })
}

fn sense_distance(raw: u32) -> lctrl_core::Result<lctrl_core::SenseDistance> {
    lctrl_core::SenseDistance::from_raw(raw).ok_or_else(|| LctrlError::InvalidArgument {
        detail: format!("distance must be 0 (near), 1 (middle), or 2 (far), got {raw}"),
    })
}

fn sense_wait(raw: u32) -> lctrl_core::Result<lctrl_core::SenseWait> {
    lctrl_core::SenseWait::from_raw(raw).ok_or_else(|| LctrlError::InvalidArgument {
        detail: format!("wait must be 0 (short), 1 (middle), or 2 (long), got {raw}"),
    })
}

fn apply_sense_features(
    sense: &dyn SenseControl,
    features: &[SenseFeature],
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let mut last = None;
    // Capture the multi-feature baseline before the first write: every
    // restore step is planned against this state, never against the
    // previous field of the last per-feature report.
    let initial = sense.sense_state()?;
    // Preflight the full restore plan before mutating anything. A feature
    // whose persisted previous value is unknown can never be put back, so
    // auto-restore refuses the command up front instead of silently
    // skipping it after the write.
    let plan = if auto_restore && apply == ApplyMode::Commit {
        Some(plan_sense_restore(features, &initial)?)
    } else {
        None
    };
    let mut written = 0usize;
    let mut primary = None;
    for feature in features {
        match sense.set_sense_feature(*feature, apply) {
            Ok(report) => {
                last = Some(report);
                written += 1;
            }
            Err(error) => {
                primary = Some(error);
                break;
            }
        }
    }
    // The restore runs even when the sequence failed mid-way (a failed
    // write or readback), covering the committed prefix plus the failed
    // feature itself, whose write may have taken effect before the
    // readback rejected it. Restoring an untouched feature only re-writes
    // its previous value, so the extra step is harmless.
    let mut restore = None;
    if let Some(plan) = &plan {
        // The plan is grouped per requested feature: ResetAll expands to a
        // re-apply of every captured field. On failure we restore the
        // committed prefix plus the failed feature's own group (its write
        // may have taken effect before readback rejected it); on success
        // the whole plan runs.
        let groups = if primary.is_some() {
            &plan[..(written + 1).min(plan.len())]
        } else {
            &plan[..written]
        };
        let flat: Vec<SenseFeature> = groups.iter().flatten().copied().collect();
        match restore_sense_features(sense, &flat) {
            Ok(text) => restore = Some(text),
            Err(cleanup) => {
                return Err(match primary {
                    Some(original) => LctrlError::FirmwareRejected {
                        detail: format!(
                            "sense write failed ({original}); auto-restore also failed ({cleanup})"
                        ),
                    },
                    None => cleanup,
                });
            }
        }
    }
    if let Some(original) = primary {
        return Err(original);
    }
    let report = last.expect("at least one sense feature");
    let mode = if report.mode() == ApplyMode::DryRun {
        "dry run"
    } else {
        "applied"
    };
    let actual = report
        .actual()
        .map_or("pending readback".to_string(), |state| state.to_string());
    let human = format!(
        "smartsense ({mode}): {} feature(s), latest state: {actual} (previous: {})\n",
        features.len(),
        report.previous(),
    );
    output_with_restore(&report, human, restore.as_deref())
}

/// Inverse of the per-feature write: reconstruct the feature that restores
/// the persisted value recorded before the change. Refuses the whole
/// command before any write when a previous value was never persisted —
/// an auto-restore that cannot put the original value back must not leave
/// the device silently changed. Attention tracking has no observed
/// persistence key on this model, so its baseline is always unknown.
fn plan_sense_restore(
    written: &[SenseFeature],
    previous: &lctrl_core::SenseState,
) -> lctrl_core::Result<Vec<Vec<SenseFeature>>> {
    let mut plan = Vec::with_capacity(written.len());
    for feature in written {
        let unknown = || LctrlError::InvalidArgument {
            detail: format!(
                "--auto-restore refused before write: the previous persisted value of {feature:?} is unknown, so the original state cannot be restored"
            ),
        };
        // ResetAll rewrites every field server-side, so a faithful restore
        // must re-apply every RPC-backed field that had a captured value —
        // not one inverse. Fields without a persisted baseline cannot be
        // restored, so the whole command is refused before the write.
        if matches!(feature, SenseFeature::ResetAll) {
            let mut inverses = Vec::with_capacity(9);
            if let Some(v) = previous.lock_on_leave {
                inverses.push(SenseFeature::LockOnLeave(v));
            }
            if let Some(v) = previous.leave_distance {
                inverses.push(SenseFeature::LeaveDistance(v));
            }
            if let Some(v) = previous.leave_wait {
                inverses.push(SenseFeature::LeaveWait(v));
            }
            if let Some(v) = previous.wake_on_approach {
                inverses.push(SenseFeature::WakeOnApproach(v));
            }
            if let Some(v) = previous.approach_distance {
                inverses.push(SenseFeature::ApproachDistance(v));
            }
            if let Some(v) = previous.approach_auto_adjust {
                inverses.push(SenseFeature::ApproachAutoAdjust(v));
            }
            if let Some(v) = previous.camera_assist {
                inverses.push(SenseFeature::CameraAssist(v));
            }
            if let Some(v) = previous.leave_auto_adjust {
                inverses.push(SenseFeature::LeaveAutoAdjust(v));
            }
            if let Some(v) = previous.pause_video {
                inverses.push(SenseFeature::PauseVideo(v));
            }
            plan.push(inverses);
            continue;
        }
        let inverse = match feature {
            SenseFeature::LockOnLeave(_) => {
                SenseFeature::LockOnLeave(previous.lock_on_leave.ok_or_else(unknown)?)
            }
            SenseFeature::LeaveDistance(_) => {
                SenseFeature::LeaveDistance(previous.leave_distance.ok_or_else(unknown)?)
            }
            SenseFeature::LeaveWait(_) => {
                SenseFeature::LeaveWait(previous.leave_wait.ok_or_else(unknown)?)
            }
            SenseFeature::WakeOnApproach(_) => {
                SenseFeature::WakeOnApproach(previous.wake_on_approach.ok_or_else(unknown)?)
            }
            SenseFeature::ApproachDistance(_) => {
                SenseFeature::ApproachDistance(previous.approach_distance.ok_or_else(unknown)?)
            }
            SenseFeature::PauseVideo(_) => {
                SenseFeature::PauseVideo(previous.pause_video.ok_or_else(unknown)?)
            }
            SenseFeature::CameraAssist(_) => {
                SenseFeature::CameraAssist(previous.camera_assist.ok_or_else(unknown)?)
            }
            SenseFeature::ApproachAutoAdjust(_) => {
                SenseFeature::ApproachAutoAdjust(previous.approach_auto_adjust.ok_or_else(unknown)?)
            }
            SenseFeature::LeaveAutoAdjust(_) => {
                SenseFeature::LeaveAutoAdjust(previous.leave_auto_adjust.ok_or_else(unknown)?)
            }
            SenseFeature::PrivacyAlert(_) => {
                SenseFeature::PrivacyAlert(previous.privacy_alert.ok_or_else(unknown)?)
            }
            SenseFeature::AttentionTracking(_) => {
                SenseFeature::AttentionTracking(previous.attention_tracking.ok_or_else(unknown)?)
            }
            SenseFeature::ResetAll => unreachable!("handled above"),
        };
        plan.push(vec![inverse]);
    }
    Ok(plan)
}

/// Re-apply a preflighted restore plan after the committed write. Called
/// both on success (full plan) and after a mid-sequence failure (plan
/// truncated to the written prefix plus the failed feature).
fn restore_sense_features(
    sense: &dyn SenseControl,
    plan: &[SenseFeature],
) -> lctrl_core::Result<String> {
    let mut restored = Vec::new();
    for inverse in plan {
        sense.set_sense_feature(*inverse, ApplyMode::Commit)?;
        restored.push(format!("{inverse:?}"));
    }
    let state = sense.sense_state()?;
    Ok(format!(
        "auto-restore: re-applied {} feature(s) ({}); state now {state}",
        restored.len(),
        restored.join(", ")
    ))
}

/// `usb ports`: the read-only inventory path. It shares no service with the
/// charging switches, because those only toggle a power gate and hold no port
/// or power-delivery data.
fn execute_usb_ports(usb_ports: Option<&dyn lctrl_hal::UsbPortInventoryControl>) -> CommandResult {
    let usb_ports = usb_ports.ok_or_else(|| LctrlError::Unsupported {
        feature: "usb.ports".into(),
    })?;
    let inventory = usb_ports.usb_port_inventory()?;
    structured_output(&inventory, format_usb_ports(&inventory))
}

fn execute_usb(
    usb: Option<&dyn UsbControl>,
    bios: Option<&dyn BiosControl>,
    command: UsbCommand,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
    auto_restore: bool,
) -> CommandResult {
    match command {
        // The dispatcher routes `usb ports` to `execute_usb_ports`, which owns
        // the read-only inventory service. This arm keeps the charging path
        // total without inventing a port list from a write service.
        UsbCommand::Ports => Err(LctrlError::Unsupported {
            feature: "usb.ports".into(),
        }),
        UsbCommand::AlwaysOn { state, persistent } => {
            let enabled = matches!(state, Toggle::On);
            if persistent {
                // BIOS-persistent layer: Lenovo firmware setting
                // `AlwaysOnUSB` (docs/02 §6), committed with --save through
                // the same validated BIOS transaction as `vantage bios set`.
                let bios = bios.ok_or_else(|| LctrlError::Unsupported {
                    feature: "bios".into(),
                })?;
                return execute_bios(
                    Some(bios),
                    BiosCommand::Set {
                        name: "AlwaysOnUSB".into(),
                        value: if enabled { "Enable" } else { "Disable" }.into(),
                        save: true,
                    },
                    apply,
                    confirmed,
                    interactive,
                    auto_restore,
                );
            }
            let usb = usb.ok_or_else(|| LctrlError::Unsupported {
                feature: "usb.always-on".into(),
            })?;
            let report = usb.set_usb_ac_charging(enabled, apply)?;
            // Say plainly whether the state was changed or only planned.
            let (label, state) = match report.actual() {
                Some(actual) => ("USB AC charging", actual),
                None => ("USB AC charging (dry run, not applied)", report.requested()),
            };
            let restore = if auto_restore && apply == ApplyMode::Commit {
                Some(restore_usb_ac(usb, report.previous().ac_enabled)?)
            } else {
                None
            };
            output_with_restore(&report, format!("{label}: {state}\n"), restore.as_deref())
        }
        UsbCommand::ChargeOnBattery { state, persistent } => {
            let enabled = matches!(state, Toggle::On);
            if persistent {
                let bios = bios.ok_or_else(|| LctrlError::Unsupported {
                    feature: "bios".into(),
                })?;
                return execute_bios(
                    Some(bios),
                    BiosCommand::Set {
                        name: "ChargeInBatteryMode".into(),
                        value: if enabled { "Enable" } else { "Disable" }.into(),
                        save: true,
                    },
                    apply,
                    confirmed,
                    interactive,
                    auto_restore,
                );
            }
            let usb = usb.ok_or_else(|| LctrlError::Unsupported {
                feature: "usb.charge-on-battery".into(),
            })?;
            let report = usb.set_usb_battery_charging(enabled, apply)?;
            let restore = if auto_restore && apply == ApplyMode::Commit {
                Some(restore_usb_battery(usb, report.previous().battery_enabled)?)
            } else {
                None
            };
            output_with_restore(
                &report,
                format!("USB battery-mode charging: {}\n", report.requested()),
                restore.as_deref(),
            )
        }
    }
}

/// Render the port tree the way the channels reported it. A port with no
/// reported speed/device/power prints exactly that, and a driver speed outside
/// the documented grades prints its raw value.
fn format_usb_ports(inventory: &lctrl_core::UsbPortInventory) -> String {
    use std::fmt::Write as _;

    let mut output = String::new();
    if inventory.controllers.is_empty() {
        output.push_str("no USB controller exposes ports on this machine\n");
    }
    for controller in &inventory.controllers {
        let instance_id = &controller.instance_id;
        let kind = controller.kind.label();
        let description = &controller.description;
        let _ = writeln!(output, "controller {instance_id} ({kind}) {description}");
        if let Some(driver) = &controller.driver {
            let _ = writeln!(output, "  driver: {driver}");
        }
        for hub in &controller.hubs {
            let hub_id = &hub.instance_id;
            let port_count = hub.port_count;
            let power_source = if hub.bus_powered {
                "bus-powered"
            } else {
                "self-powered"
            };
            let _ = writeln!(output, "  hub {hub_id}: {port_count} port(s), {power_source}");
            for port in &hub.ports {
                let number = port.number;
                let facts = format_usb_port(port);
                let _ = writeln!(output, "    port {number}: {facts}");
            }
        }
        if let Some(detail) = &controller.ports_unavailable {
            let _ = writeln!(output, "  ports: unavailable ({detail})");
        }
    }

    let power = &inventory.power_delivery;
    output.push_str("power delivery:\n");
    match &power.pd_controller {
        Some(controller) => {
            let id = &controller.instance_id;
            let driver = controller
                .driver
                .as_ref()
                .map(|driver| format!(" (driver {driver})"))
                .unwrap_or_default();
            let firmware = controller
                .firmware_version
                .as_ref()
                .map(|version| format!(", firmware {version}"))
                .unwrap_or_default();
            let _ = writeln!(output, "  controller: {id}{driver}{firmware}");
        }
        None => output.push_str("  controller: none present\n"),
    }
    for router in &power.usb4_host_routers {
        let _ = writeln!(output, "  usb4 host router: {router}");
    }
    if let Some(detail) = &power.per_port_unavailable {
        let _ = writeln!(output, "  per-port power: unavailable ({detail})");
    }
    output
}

fn format_usb_port(port: &lctrl_core::UsbPort) -> String {
    use lctrl_core::UsbPortConnection;

    let mut text = match port.connection {
        UsbPortConnection::Empty => "empty".to_string(),
        UsbPortConnection::Connected => "connected".to_string(),
        UsbPortConnection::Other(raw) => format!("connection status {raw}"),
    };
    if let Some(speed) = port.speed {
        text.push_str(&format!(", speed {speed}"));
    }
    if let Some(device) = &port.device {
        let vendor = device.vendor_id;
        let product = device.product_id;
        let revision = &device.usb_revision;
        text.push_str(&format!(
            ", device {vendor:04x}:{product:04x} (USB {revision})"
        ));
        if let Some(description) = &device.description {
            text.push_str(&format!(" \"{description}\""));
        }
        if let Some(instance_id) = &device.instance_id {
            text.push_str(&format!(" [{instance_id}]"));
        }
        if let Some(milliamps) = device.max_power_ma {
            text.push_str(&format!(", declares {milliamps} mA"));
        }
        if device.is_hub {
            text.push_str(" (hub)");
        }
    }
    text
}

fn execute_fan(
    fan: Option<&dyn FanControl>,
    command: FanCommand,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let fan = fan.ok_or_else(|| LctrlError::Unsupported {
        feature: "perf.fan".into(),
    })?;
    let restore =
        |report: &lctrl_core::ChangeReport<FanMode>| -> lctrl_core::Result<Option<String>> {
            if auto_restore
                && apply == ApplyMode::Commit
                && *report.previous() != *report.actual().unwrap_or(report.previous())
            {
                let restored = fan.set_fan_mode(*report.previous(), ApplyMode::Commit)?;
                return Ok(Some(format!(
                    "restored fan mode: {:?} (verified)\n",
                    restored.actual()
                )));
            }
            Ok(None)
        };
    match command {
        FanCommand::Status => execute_fan_status(Some(fan)),
        FanCommand::Auto | FanCommand::Smart => {
            let report = fan.set_fan_mode(FanMode::Standard, apply)?;
            output_with_restore(
                &report,
                "fan mode: balanced/automatic\n".into(),
                restore(&report)?.as_deref(),
            )
        }
        FanCommand::Fullspeed => {
            let report = fan.set_fan_mode(FanMode::Performance, apply)?;
            output_with_restore(
                &report,
                "fan mode: maximum performance\n".into(),
                restore(&report)?.as_deref(),
            )
        }
        FanCommand::Manual => {
            let report = fan.set_fan_mode(FanMode::Custom, apply)?;
            output_with_restore(
                &report,
                "fan mode: manual/custom\n".into(),
                restore(&report)?.as_deref(),
            )
        }
        FanCommand::Curve { .. } => Err(LctrlError::Unsupported {
            feature: "perf.fan.curve.kernel-interface".into(),
        }),
    }
}

/// Identity of the embedded panel, decoded from its own EDID.
///
/// Everything printed here comes from the panel's base block except
/// `connection`, which also needs the OS output-technology classification (an
/// internal DisplayPort interface code is eDP, an external one is not).
fn execute_panel_info(identity: Option<&dyn PanelIdentityControl>) -> CommandResult {
    let identity = identity
        .ok_or_else(|| LctrlError::Unsupported {
            feature: "panel.identity".into(),
        })?
        .identity()?;
    let mut human = String::new();
    let model = identity.model_name.as_deref().unwrap_or("no name descriptor");
    let _ = writeln!(
        human,
        "panel: {}{:04X} ({model})",
        identity.manufacturer, identity.product_code
    );
    let _ = writeln!(
        human,
        "manufacturer: {} (id 0x{:04x})",
        identity.manufacturer, identity.manufacturer_id
    );
    if let Some(name) = &identity.product_name {
        let _ = writeln!(human, "product name: {name}");
    }
    match identity.serial_number {
        Some(serial) => {
            let _ = writeln!(human, "serial: {serial} (0x{serial:08x})");
        }
        None => {
            let _ = writeln!(human, "serial: not specified by the EDID");
        }
    }
    if let Some(serial) = &identity.serial_text {
        let _ = writeln!(human, "serial text: {serial}");
    }
    let manufacture = &identity.manufacture;
    let date = match (manufacture.model_year, manufacture.week) {
        (true, _) => format!("{} (model year)", manufacture.year),
        (false, Some(week)) => format!("{} week {}", manufacture.year, week),
        (false, None) => format!("{} (week not specified)", manufacture.year),
    };
    let _ = writeln!(human, "manufactured: {date}");
    let _ = writeln!(
        human,
        "edid: version {}.{}, {} extension block(s)",
        identity.edid_version.major, identity.edid_version.minor, identity.extension_blocks
    );
    match (&identity.native_resolution, identity.native_refresh_hz) {
        (Some(resolution), Some(hz)) => {
            let _ = writeln!(
                human,
                "native resolution: {}x{} @ {hz}Hz (preferred timing)",
                resolution.width, resolution.height
            );
        }
        (Some(resolution), None) => {
            let _ = writeln!(
                human,
                "native resolution: {}x{} (preferred timing)",
                resolution.width, resolution.height
            );
        }
        (None, _) => {
            let _ = writeln!(
                human,
                "native resolution: unavailable (no preferred timing descriptor)"
            );
        }
    }
    match identity.physical_size {
        Some(size) => {
            let diagonal = (f64::from(size.width_mm).powi(2)
                + f64::from(size.height_mm).powi(2))
            .sqrt();
            let _ = writeln!(
                human,
                "physical size: {}x{} mm ({:.1} in diagonal)",
                size.width_mm,
                size.height_mm,
                diagonal / 25.4
            );
        }
        None => {
            let _ = writeln!(human, "physical size: not specified by the EDID");
        }
    }
    let input = &identity.input;
    if input.digital {
        let depth = match input.bit_depth {
            Some(bits) => format!("{bits} bits per primary"),
            None => "undefined bit depth".to_owned(),
        };
        let _ = writeln!(
            human,
            "input: digital, {depth}, interface code {}",
            input.interface_code
        );
    } else {
        let _ = writeln!(human, "input: analog");
    }
    let connection = if identity.internal {
        match identity.connection {
            lctrl_core::PanelConnection::Edp => "edp (internal panel)".to_owned(),
            lctrl_core::PanelConnection::Unspecified => {
                "unspecified (internal panel; the EDID names no DisplayPort interface, \
                 so LVDS and an EDID-1.3-style eDP byte cannot be told apart)"
                    .to_owned()
            }
            other => format!("{other} (internal panel)"),
        }
    } else {
        format!("{} (external sink)", identity.connection)
    };
    let _ = writeln!(human, "connection: {connection}");
    if let Some(limits) = &identity.limits {
        let _ = writeln!(
            human,
            "limits: {}-{} Hz vertical, {}-{} kHz horizontal, <= {} MHz pixel clock",
            limits.min_refresh_hz,
            limits.max_refresh_hz,
            limits.min_horizontal_khz,
            limits.max_horizontal_khz,
            limits.max_pixel_clock_mhz
        );
    }
    structured_output(&identity, human)
}

/// `display status` payload. Every field carries or names its scope:
/// `session`+`displays` are the calling session's live desktop, `monitors` is
/// machine-wide driver/EDID state joined by monitor hardware id, and
/// `firmware_refresh` is the firmware refresh-range table.
///
/// A refused live read is `displays_error` (never an empty `displays` list), and
/// `session_desktops` then names the surface this session does have, so a
/// synthetic desktop cannot be mistaken for the panel.
#[derive(Serialize)]
struct DisplayStatusPayload {
    session: lctrl_core::DisplaySession,
    displays: Option<Vec<lctrl_core::DisplayPathState>>,
    displays_error: Option<Value>,
    session_desktops: Option<Vec<lctrl_core::SessionDesktop>>,
    monitors: Option<Vec<lctrl_core::MonitorColorState>>,
    monitors_error: Option<Value>,
    firmware_refresh: Option<lctrl_core::PanelRefreshCapability>,
    firmware_refresh_error: Option<Value>,
}

/// Capture a read together with its serialized error report and its rendered
/// message, so the human text can say why a scope is missing.
fn capture_optional_with<T>(
    result: lctrl_core::Result<T>,
) -> (Option<T>, Option<Value>, Option<String>) {
    match result {
        Ok(value) => (Some(value), None, None),
        Err(error) => (
            None,
            Some(serde_json::to_value(error.report()).expect("error reports are serializable")),
            Some(error.to_string()),
        ),
    }
}

fn execute_display_status(
    display: Option<&dyn DisplayStatusControl>,
    panel: Option<&dyn PanelControl>,
) -> CommandResult {
    let display = display.ok_or_else(|| LctrlError::Unsupported {
        feature: "display.status".into(),
    })?;
    let session = display.session();
    let (displays, displays_error, displays_reason) = capture_optional_with(display.displays());
    let (monitors, monitors_error, monitors_reason) = capture_optional_with(display.monitors());
    // The session's own monitor list only explains a refused live read; when the
    // live paths are available it would just repeat them.
    let session_desktops = if displays.is_none() {
        display.desktops().ok()
    } else {
        None
    };
    let refresh = match panel {
        Some(panel) => panel.refresh_capability(),
        None => Err(LctrlError::Unsupported {
            feature: "panel.refresh".into(),
        }),
    };
    let (firmware_refresh, firmware_refresh_error, firmware_reason) =
        capture_optional_with(refresh);
    let payload = DisplayStatusPayload {
        session,
        displays,
        displays_error,
        session_desktops,
        monitors,
        monitors_error,
        firmware_refresh,
        firmware_refresh_error,
    };
    let human = format_display_status(
        &payload,
        displays_reason.as_deref(),
        monitors_reason.as_deref(),
        firmware_reason.as_deref(),
    );
    structured_output(&payload, human)
}

/// Rendered reason for a scope whose read failed, when there is one.
type ScopeReason<'a> = Option<&'a str>;

fn format_display_status(
    payload: &DisplayStatusPayload,
    displays_reason: ScopeReason<'_>,
    monitors_reason: ScopeReason<'_>,
    firmware_reason: ScopeReason<'_>,
) -> String {
    use std::fmt::Write as _;

    let mut human = String::new();
    let session = &payload.session;
    let console = session
        .console_session_id
        .map_or_else(|| "none".to_owned(), |id| id.to_string());
    let _ = writeln!(
        human,
        "display state: session {} (console session {console}, interactive: {})",
        session.id,
        yes_no(session.interactive)
    );
    match &payload.displays {
        Some(displays) if displays.is_empty() => {
            let _ = writeln!(human, "  live display paths (session scope): none active");
        }
        Some(displays) => {
            let _ = writeln!(human, "  live display paths (session scope):");
            for display in displays {
                let _ = writeln!(human, "    {}", display_path_text(display));
            }
        }
        None => {
            let reason = displays_reason.unwrap_or("the OS refused the display read");
            let _ = writeln!(
                human,
                "  live display paths (session scope): unavailable — {reason}"
            );
            for desktop in payload.session_desktops.iter().flatten() {
                let _ = writeln!(
                    human,
                    "  this session's own desktop, not a panel: {} {}x{} scaling={}",
                    desktop.device_name,
                    desktop.width,
                    desktop.height,
                    scaling_text(desktop.scaling)
                );
            }
        }
    }
    match &payload.monitors {
        Some(monitors) => {
            let _ = writeln!(
                human,
                "  machine-wide monitors (display-driver colour store + EDID, joined by hardware id):"
            );
            for monitor in monitors {
                let _ = writeln!(human, "    {}", monitor_text(monitor));
            }
        }
        None => {
            let reason = monitors_reason.unwrap_or("the machine-wide read failed");
            let _ = writeln!(human, "  machine-wide monitors: unavailable — {reason}");
        }
    }
    match (
        &payload.firmware_refresh,
        payload.firmware_refresh_error.is_some(),
    ) {
        (Some(refresh), _) => {
            let _ = writeln!(
                human,
                "  firmware refresh table: {}..={} Hz (default {} Hz)",
                refresh.min_hz, refresh.max_hz, refresh.default_hz
            );
        }
        (None, true) => {
            let reason = firmware_reason.unwrap_or("the firmware table is unavailable");
            let _ = writeln!(human, "  firmware refresh table: unavailable — {reason}");
        }
        (None, false) => {}
    }
    human
}

fn display_path_text(display: &lctrl_core::DisplayPathState) -> String {
    use std::fmt::Write as _;

    let mode = &display.mode;
    let mut text = format!(
        "{} {} {}x{}@{}Hz {}bpp rotation={} position=({}, {}) scaling={}",
        display.device_name,
        display.connection,
        mode.width,
        mode.height,
        mode.refresh_hz,
        mode.bits_per_pixel,
        mode.rotation.raw(),
        mode.position_x,
        mode.position_y,
        scaling_text(display.scaling)
    );
    match &display.color {
        Some(color) => {
            let _ = write!(text, " colour={}", color_text(color));
        }
        None => text.push_str(" colour=rejected by the target"),
    }
    text
}

fn scaling_text(observation: lctrl_core::DisplayScalingObservation) -> String {
    match observation {
        lctrl_core::DisplayScalingObservation::Effective(scaling) => format!(
            "{}% ({}x{} dpi, OS effective dpi)",
            scaling.percent, scaling.dpi_x, scaling.dpi_y
        ),
        lctrl_core::DisplayScalingObservation::NoMonitor => {
            "unavailable (no monitor covers this desktop position)".to_owned()
        }
        lctrl_core::DisplayScalingObservation::ProcessNotPerMonitorAware => {
            "unavailable (process is not per-monitor DPI aware)".to_owned()
        }
    }
}

fn color_text(color: &lctrl_core::DisplayColorState) -> String {
    use std::fmt::Write as _;

    let mut text = match (color.hdr_supported, color.hdr_user_enabled) {
        (Some(supported), Some(enabled)) => {
            let mode = match color.active_mode {
                Some(lctrl_core::ActiveColorMode::Sdr) => "sdr",
                Some(lctrl_core::ActiveColorMode::WideColorGamut) => "wide-colour",
                Some(lctrl_core::ActiveColorMode::Hdr) => "hdr",
                Some(lctrl_core::ActiveColorMode::Unknown(_)) | None => "unknown",
            };
            format!(
                "hdr supported={} enabled={} active-mode={mode}",
                yes_no(supported),
                yes_no(enabled)
            )
        }
        _ => "hdr unknown (pre-24H2 request: advanced colour only)".to_owned(),
    };
    let limited = match color.advanced_color_limited_by_policy {
        Some(limited) => format!(" limited-by-policy={}", yes_no(limited)),
        None => String::new(),
    };
    let _ = write!(
        text,
        " advanced-colour supported={} active={}{limited} encoding={} {}bpc{}",
        yes_no(color.advanced_color_supported),
        yes_no(color.advanced_color_active),
        encoding_name(color.color_encoding),
        color.bits_per_color,
        match color.sdr_white_millinits {
            Some(millinits) => format!(" sdr-white={} nits", nits_text(millinits)),
            None => " sdr-white=unknown".to_owned(),
        }
    );
    text
}

fn monitor_text(monitor: &lctrl_core::MonitorColorState) -> String {
    use std::fmt::Write as _;

    let mut text = format!(
        "{} {}: hdr={} acm={} (supported={})",
        monitor.hardware_id,
        monitor.connection,
        tri_state(monitor.hdr_enabled),
        tri_state(monitor.auto_color_management_enabled),
        tri_state(monitor.auto_color_management_supported)
    );
    if let Some(raw) = monitor.sdr_white_level_raw {
        let _ = write!(text, " sdr-white-raw={raw}");
    }
    if let Some(millinits) = monitor.sdr_white_millinits {
        let _ = write!(text, " sdr-white={} nits", nits_text(millinits));
    }
    match &monitor.hdr_capability {
        lctrl_core::HdrCapabilityObservation::Declared(capability) => {
            let _ = write!(text, " edid-hdr={}", hdr_capability_text(capability));
        }
        lctrl_core::HdrCapabilityObservation::NotDeclared => {
            text.push_str(" edid-hdr=none declared");
        }
        lctrl_core::HdrCapabilityObservation::Unavailable(reason) => {
            let _ = write!(text, " edid-hdr=unreadable ({reason})");
        }
    }
    if let Some(notified) = monitor.display_hdr_level_notified {
        let _ = write!(text, " hdr-level-notified={}", yes_no(notified));
    }
    text
}

fn hdr_capability_text(capability: &lctrl_core::PanelHdrCapability) -> String {
    use std::fmt::Write as _;

    let mut eotfs = Vec::new();
    if capability.eotf_sdr {
        eotfs.push("sdr");
    }
    if capability.eotf_hdr10_pq {
        eotfs.push("hdr10-pq");
    }
    if capability.eotf_hlg {
        eotfs.push("hlg");
    }
    let mut text = if eotfs.is_empty() {
        "no-eotf".to_owned()
    } else {
        eotfs.join("+")
    };
    if capability.bt2020_rgb {
        text.push_str(" bt2020-rgb");
    }
    if capability.static_metadata_type1 {
        text.push_str(" static-metadata-1");
    }
    if let Some(millinits) = capability.max_luminance_millinits {
        let _ = write!(text, " max={} nits", nits_text(millinits));
    }
    if let Some(millinits) = capability.min_luminance_millinits {
        let _ = write!(text, " min={} nits", nits_text(millinits));
    }
    text
}

/// Millinits rendered as nits without losing the fractional part.
fn nits_text(millinits: u32) -> String {
    format!("{}.{:03}", millinits / 1000, millinits % 1000)
}

fn encoding_name(encoding: lctrl_core::DisplayColorEncoding) -> &'static str {
    match encoding {
        lctrl_core::DisplayColorEncoding::Rgb => "rgb",
        lctrl_core::DisplayColorEncoding::YCbCr444 => "ycbcr444",
        lctrl_core::DisplayColorEncoding::YCbCr422 => "ycbcr422",
        lctrl_core::DisplayColorEncoding::YCbCr420 => "ycbcr420",
        lctrl_core::DisplayColorEncoding::Intensity => "intensity",
        lctrl_core::DisplayColorEncoding::Unknown(_) => "unknown",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn tri_state(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn execute_panel_brightness(
    panel: Option<&dyn PanelBrightnessControl>,
    percent: Option<u8>,
    expect_current: Option<u8>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let panel = panel.ok_or_else(|| LctrlError::Unsupported {
        feature: "panel.brightness".into(),
    })?;
    match percent {
        Some(percent) => {
            let report = panel.set_brightness(percent, expect_current, apply)?;
            let restore =
                if auto_restore && apply == ApplyMode::Commit && *report.previous() != percent {
                    // No expected_current on restore: the just-verified write is
                    // the freshest observation, and the restore should not be
                    // gated on a stale snapshot.
                    let restored =
                        panel.set_brightness(*report.previous(), None, ApplyMode::Commit)?;
                    Some(format!(
                        "restored panel brightness: {}% (verified)\n",
                        restored
                            .actual()
                            .map_or("unknown".into(), |v| v.to_string())
                    ))
                } else {
                    None
                };
            output_with_restore(
                &report,
                format!("panel brightness transition requested: {percent}%\n"),
                restore.as_deref(),
            )
        }
        None => {
            let brightness = panel.brightness()?;
            structured_output(
                &brightness,
                format!("panel brightness: {}%\n", brightness.current_percent),
            )
        }
    }
}

fn execute_panel(
    panel: Option<&dyn PanelControl>,
    panel_features: Option<&dyn PanelFeaturesControl>,
    command: PanelCommand,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let panel = panel.ok_or_else(|| LctrlError::Unsupported {
        feature: "panel".into(),
    })?;
    match command {
        // `panel info` is dispatched before the refresh controller is required:
        // it reads the panel's EDID and never needs a display mode channel. If a
        // future dispatch reached here it would be routed wrongly, which is an
        // error to report, not a panic in a read-only command.
        PanelCommand::Info => Err(LctrlError::InvalidArgument {
            detail: "panel info is dispatched before the panel refresh controller".into(),
        }),
        PanelCommand::Brightness {
            percent,
            expect_current,
        } => execute_panel_brightness(None, percent, expect_current, apply, auto_restore),
        PanelCommand::Rate { rate } => {
            let requested = match rate {
                PanelRate::Fps60 => lctrl_core::PanelRefreshRequest::Fixed(60),
                PanelRate::Fps120 => lctrl_core::PanelRefreshRequest::Fixed(120),
                PanelRate::Auto => lctrl_core::PanelRefreshRequest::Auto,
            };
            let report = panel.set_refresh_rate(requested, apply)?;
            let restore = if auto_restore && apply == ApplyMode::Commit {
                // The report carries the full PanelMode, not the request;
                // restoring means replaying the exact observed previous mode.
                // `PanelRefreshRequest::Auto` is never a restore target: it
                // resolves to the firmware default rate (`DefaultRefreshRate`),
                // which differs from the observed rate on any panel that is not
                // sitting on that default. Measured on the target: firmware
                // refresh `Mode=1` (adaptive) with a 120 Hz desktop and
                // `DefaultRefreshRate=60`, so an Auto restore asked for 60 Hz
                // while the panel was already at 60 Hz, changed nothing, and
                // failed the mismatch check below with the panel left on the
                // new rate. The firmware refresh `Mode` has no write channel, so
                // a fixed replay is the only restore this HAL can verify;
                // adaptivity cannot be restored by any request shape.
                let previous = report.previous();
                let restore_request = lctrl_core::PanelRefreshRequest::Fixed(
                    u16::try_from(previous.refresh_hz).map_err(|_| {
                        LctrlError::FirmwareRejected {
                            detail: format!(
                                "panel refresh {} Hz cannot be restored: exceeds u16",
                                previous.refresh_hz
                            ),
                        }
                    })?,
                );
                let restored = panel.set_refresh_rate(restore_request, ApplyMode::Commit)?;
                if restored.actual().map(|m| m.refresh_hz) != Some(previous.refresh_hz) {
                    return Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "panel refresh restore mismatch: expected {} Hz, actual {:?} Hz",
                            previous.refresh_hz,
                            restored.actual().map(|m| m.refresh_hz)
                        ),
                    });
                }
                Some(format!(
                    "restored panel refresh: {} Hz (verified)\n",
                    previous.refresh_hz
                ))
            } else {
                None
            };
            output_with_restore(
                &report,
                "panel refresh transition requested\n".into(),
                restore.as_deref(),
            )
        }
        PanelCommand::Color { .. } => Err(LctrlError::Unsupported {
            feature: "panel.color.write".into(),
        }),
        PanelCommand::SuperResolution { state } => {
            let features = panel_features.ok_or_else(|| LctrlError::Unsupported {
                feature: "panel.super-resolution.write".into(),
            })?;
            let report = features.set_super_resolution(state == Toggle::On, apply)?;
            structured_output(
                &report,
                format!(
                    "panel super-resolution ({}): previous {}, requested {}\n",
                    if report.mode() == ApplyMode::DryRun {
                        "dry run"
                    } else {
                        "applied"
                    },
                    report
                        .previous()
                        .map(|value| if value { "on" } else { "off" })
                        .unwrap_or("unknown"),
                    if report.requested() == &Some(true) {
                        "on"
                    } else {
                        "off"
                    }
                ),
            )
        }
        PanelCommand::Overdrive { .. } => Err(LctrlError::Unsupported {
            feature: "panel.overdrive.write: LENOVO_CAPABILITY_DATA_00 advertises no 0x1A family and the TCON device is absent (docs/evidence/re-panel-20261105.md §3)".into(),
        }),
        PanelCommand::EyeCare { level } => {
            let features = panel_features.ok_or_else(|| LctrlError::Unsupported {
                feature: "panel.eye-care.write".into(),
            })?;
            let target = match level {
                EyeCareLevel::Off => lctrl_core::EyeCareLevel::Off,
                EyeCareLevel::Mid => lctrl_core::EyeCareLevel::Mid,
                EyeCareLevel::High => lctrl_core::EyeCareLevel::High,
            };
            let report = features.set_eye_care(target, apply)?;
            structured_output(
                &report,
                format!(
                    "panel eye-care ({}): previous {}, requested {}\n",
                    if report.mode() == ApplyMode::DryRun {
                        "dry run"
                    } else {
                        "applied"
                    },
                    report
                        .previous()
                        .map(|level| level.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    report
                        .requested()
                        .map(|level| level.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ),
            )
        }
    }
}

fn execute_active_power_scheme(power: Option<&dyn PowerControl>) -> CommandResult {
    let power = power.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.scheme".into(),
    })?;
    let scheme = power.active_power_scheme()?;
    structured_output(&scheme, format!("active power scheme: {}\n", scheme.name))
}

/// Steps applied (or would-be-applied) by the one-shot power saver workflow.
///
/// Mirrors the original `COneKeyPowerSaverFactory` contract within the
/// primitives this platform verifies: activate the power-saver scheme, push
/// CPU policy to efficiency (PERFEPP=100%, PERFBOOSTMODE=disabled), and dim
/// the internal panel. The original also closes the dGPU and shrinks process
/// working sets through its ResScheduler; this target has no dGPU leg and no
/// process-trim contract, so those legs are reported as skipped, not faked.
#[derive(Serialize)]
struct SaverOnceStep {
    step: &'static str,
    outcome: String,
}

#[derive(Serialize)]
struct SaverOnceReport {
    apply: String,
    steps: Vec<SaverOnceStep>,
    skipped: Vec<SaverOnceStep>,
    auto_restore: Option<Vec<String>>,
    notes: &'static str,
}

fn execute_power_saver_once(
    services: &CommandServices<'_>,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let power = services.power.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.saver-once: no power control service".into(),
    })?;

    const PROCTHROTTLEMAX: &str = "bc5038f7-23e0-4960-96da-33abaf5935ec";
    const PROCESSOR_SUBGROUP: &str = "54533251-82be-4824-96c1-47b60b740d00";
    const SAVER_CPU_CAP_PERCENT: u32 = 60;
    const SAVER_BRIGHTNESS_PERCENT: u8 = 30;

    // ---- Capture pre-state (read-only; needed for every unwind/restore path).
    let scheme = power.active_power_scheme()?;
    let throttle_key = PowerSettingKey {
        subgroup: PowerGuid::new(PROCESSOR_SUBGROUP)?,
        setting: PowerGuid::new(PROCTHROTTLEMAX)?,
    };
    // PROCTHROTTLEMAX keeps its AC/DC index reads while
    // PowerReadValue{Min,Max,Increment} return ERROR_FILE_NOT_FOUND on this
    // target; the setting's domain is documented as a percentage 0..=100, so
    // a missing range falls back to a checked-percentage write rather than
    // blocking the saver.
    let throttle_range = power.power_value_range(&throttle_key).ok();
    let throttle_value = |percent: u32| -> lctrl_core::Result<PowerSettingValue> {
        if percent > 100 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("processor maximum {percent}% is outside 0..=100"),
            });
        }
        match &throttle_range {
            Some(range) => PowerSettingValue::new(percent, range),
            None => Ok(PowerSettingValue::unchecked(percent)),
        }
    };
    let prev_throttle = power
        .read_power_setting(&scheme.id, &throttle_key)
        .map(|observation| observation.values)?;
    let processor = services.processor_power;
    let prev_epp = processor.map(|p| p.epp_percent_ac_dc()).transpose()?;
    let prev_boost = processor.map(|p| p.boost_ac_dc()).transpose()?;
    let panel = services.panel_brightness;
    let prev_brightness = panel
        .map(|p| p.brightness().map(|b| b.current_percent))
        .transpose()?;

    let mut steps: Vec<SaverOnceStep> = Vec::new();
    let mut skipped: Vec<SaverOnceStep> = Vec::new();
    let dry = apply == ApplyMode::DryRun;

    // The original's "activate saver scheme" leg is not expressible on this
    // target: powercfg enumerates only 平衡 (Balanced) — there is no
    // dedicated Power saver scheme to activate. The saver policy is applied
    // directly into the active scheme instead (which is exactly how the
    // original's CPU-cap leg works: it writes the processor maximum of the
    // active scheme, it does not clone one).
    skipped.push(SaverOnceStep {
        step: "scheme",
        outcome: format!(
            "no dedicated Power-saver scheme is enumerated on this target; writing saver policy into active scheme {} ({})",
            scheme.name,
            scheme.id.as_str()
        ),
    });

    // Step 1: cap the processor maximum on DC (the original's CPU降频 leg).
    let mut throttle_dc_report = None;
    if prev_throttle.dc > SAVER_CPU_CAP_PERCENT {
        let mutation = PowerMutation::SetValue {
            key: throttle_key.clone(),
            source: PowerSource::Dc,
            value: throttle_value(SAVER_CPU_CAP_PERCENT)?,
        };
        if dry {
            skipped.push(SaverOnceStep {
                step: "cpu_cap_dc",
                outcome: format!(
                    "dry-run: would write PROCTHROTTLEMAX DC {}% → {}%",
                    prev_throttle.dc, SAVER_CPU_CAP_PERCENT
                ),
            });
        } else {
            throttle_dc_report = Some(power.apply_power_mutation(mutation, apply)?);
            steps.push(SaverOnceStep {
                step: "cpu_cap_dc",
                outcome: format!(
                    "PROCTHROTTLEMAX DC {}% → {}%",
                    prev_throttle.dc, SAVER_CPU_CAP_PERCENT
                ),
            });
        }
    } else {
        skipped.push(SaverOnceStep {
            step: "cpu_cap_dc",
            outcome: format!(
                "processor maximum already at/below {}% on DC ({}%)",
                SAVER_CPU_CAP_PERCENT, prev_throttle.dc
            ),
        });
    }

    // Step 2: CPU efficiency policy — EPP 100% on AC+DC.
    let mut epp_report = None;
    if let (Some(processor), Some(_prev)) = (processor, prev_epp) {
        if dry {
            skipped.push(SaverOnceStep {
                step: "epp",
                outcome: "dry-run: would set PERFEPP → 100% (AC+DC)".into(),
            });
        } else {
            epp_report =
                Some(processor.set_epp_percent_ac_dc(PowerSourceValues::same(100), apply)?);
            steps.push(SaverOnceStep {
                step: "epp",
                outcome: "PERFEPP → 100% (maximum energy preference, AC+DC)".into(),
            });
        }
    } else {
        skipped.push(SaverOnceStep {
            step: "epp",
            outcome: "processor-power service or EPP read unavailable; skipped".into(),
        });
    }

    // Step 3: processor boost off — the largest single saver lever.
    let mut boost_report = None;
    if let (Some(processor), Some(_prev)) = (processor, prev_boost) {
        if dry {
            skipped.push(SaverOnceStep {
                step: "boost",
                outcome: "dry-run: would set PERFBOOSTMODE → disabled (AC+DC)".into(),
            });
        } else {
            boost_report =
                Some(processor.set_boost_ac_dc(
                    PowerSourceValues::same(ProcessorBoostMode::DISABLED),
                    apply,
                )?);
            steps.push(SaverOnceStep {
                step: "boost",
                outcome: "PERFBOOSTMODE → disabled (AC+DC)".into(),
            });
        }
    } else {
        skipped.push(SaverOnceStep {
            step: "boost",
            outcome: "processor-power service or boost read unavailable; skipped".into(),
        });
    }

    // Step 4: dim panel to 30%.
    let mut brightness_report = None;
    if let (Some(panel), Some(prev)) = (panel, prev_brightness) {
        if prev > SAVER_BRIGHTNESS_PERCENT {
            if dry {
                skipped.push(SaverOnceStep {
                    step: "brightness",
                    outcome: format!(
                        "dry-run: would dim panel {prev}% → {SAVER_BRIGHTNESS_PERCENT}%"
                    ),
                });
            } else {
                brightness_report =
                    Some(panel.set_brightness(SAVER_BRIGHTNESS_PERCENT, Some(prev), apply)?);
                steps.push(SaverOnceStep {
                    step: "brightness",
                    outcome: format!("panel brightness {prev}% → {SAVER_BRIGHTNESS_PERCENT}%"),
                });
            }
        } else {
            skipped.push(SaverOnceStep {
                step: "brightness",
                outcome: format!(
                    "panel brightness already at/below {SAVER_BRIGHTNESS_PERCENT}% ({prev}%)"
                ),
            });
        }
    } else {
        skipped.push(SaverOnceStep {
            step: "brightness",
            outcome: "panel-brightness service or read unavailable; skipped".into(),
        });
    }

    // Legs the original performs that this platform cannot verify; they are
    // disclosed rather than claimed.
    skipped.push(SaverOnceStep {
        step: "dgpu_off",
        outcome: "no verified target dGPU power-down primitive; skipped".into(),
    });
    skipped.push(SaverOnceStep {
        step: "workset_trim",
        outcome: "no process working-set trim contract attached; skipped".into(),
    });

    // ---- Optional auto-restore: replay each captured previous state.
    let mut restored_log: Vec<String> = Vec::new();
    if auto_restore && apply == ApplyMode::Commit {
        if let (Some(processor), Some(prev)) = (processor, prev_boost) {
            if boost_report.is_some() {
                processor
                    .set_boost_ac_dc(prev, ApplyMode::Commit)
                    .map(|report| {
                        restored_log.push(format!("boost → {:?}", report.actual()));
                    })?;
            }
        }
        if let (Some(processor), Some(prev)) = (processor, prev_epp) {
            if epp_report.is_some() {
                processor
                    .set_epp_percent_ac_dc(prev, ApplyMode::Commit)
                    .map(|report| {
                        restored_log.push(format!("epp → {:?}", report.actual()));
                    })?;
            }
        }
        if let (Some(panel), Some(prev)) = (panel, prev_brightness) {
            if brightness_report.is_some() {
                panel
                    .set_brightness(prev, None, ApplyMode::Commit)
                    .map(|report| {
                        restored_log.push(format!("brightness → {:?}%", report.actual()));
                    })?;
            }
        }
        if throttle_dc_report.is_some() {
            let restore_mutation = PowerMutation::SetValue {
                key: throttle_key.clone(),
                source: PowerSource::Dc,
                value: throttle_value(prev_throttle.dc)?,
            };
            power.apply_power_mutation(restore_mutation, ApplyMode::Commit)?;
            restored_log.push(format!("PROCTHROTTLEMAX DC → {}%", prev_throttle.dc));
        }
    }

    let report = SaverOnceReport {
        apply: if dry {
            "dry_run".into()
        } else {
            "commit".into()
        },
        steps,
        skipped,
        auto_restore: if restored_log.is_empty() {
            None
        } else {
            Some(restored_log)
        },
        notes: "one-shot saver: writes are applied but not transactional; the restore plan is \
                best-effort. The dedicated power-saver scheme, dGPU-off and workset-trim legs \
                are absent on this platform.",
    };
    let mut human = String::from("power saver-once\n");
    for s in &report.steps {
        human.push_str(&format!("  ✓ {}: {}\n", s.step, s.outcome));
    }
    for s in &report.skipped {
        human.push_str(&format!("  – {}: {}\n", s.step, s.outcome));
    }
    if let Some(restored) = &report.auto_restore {
        human.push_str("  auto-restore applied:\n");
        for line in restored {
            human.push_str(&format!("    {line}\n"));
        }
    }
    structured_output(&report, human)
}

fn resolve_power_scheme(
    mut schemes: Vec<lctrl_core::PowerScheme>,
    name: &str,
) -> lctrl_core::Result<lctrl_core::PowerScheme> {
    let exact_id = {
        let mut matches = schemes
            .iter()
            .enumerate()
            .filter(|(_, scheme)| scheme.id.as_str() == name);
        let first = matches.next().map(|(index, _)| index);
        if matches.next().is_some() {
            return Err(LctrlError::InvalidArgument {
                detail: format!("power scheme ID {name:?} is duplicated"),
            });
        }
        first
    };
    if let Some(index) = exact_id {
        return Ok(schemes.swap_remove(index));
    }
    let index = {
        let mut matches = schemes
            .iter()
            .enumerate()
            .filter(|(_, scheme)| scheme.name.eq_ignore_ascii_case(name));
        let first = matches.next().map(|(index, _)| index);
        if matches.next().is_some() {
            return Err(LctrlError::InvalidArgument {
                detail: format!("power scheme name {name:?} is ambiguous; use an exact scheme ID"),
            });
        }
        first.ok_or_else(|| LctrlError::InvalidArgument {
            detail: format!("power scheme {name:?} is not enumerated"),
        })?
    };
    Ok(schemes.swap_remove(index))
}

fn execute_power_scheme(
    power: Option<&dyn PowerControl>,
    command: SchemeCommand,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let power = power.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.scheme".into(),
    })?;
    match command {
        SchemeCommand::List => {
            let schemes = power.power_schemes()?;
            structured_output(&schemes, format!("{} power scheme(s)\n", schemes.len()))
        }
        SchemeCommand::Get {
            name,
            subgroup,
            setting,
        } => {
            let key = match (subgroup, setting) {
                (None, None) => None,
                (Some(subgroup), Some(setting)) => Some(PowerSettingKey {
                    subgroup: PowerGuid::new(subgroup)?,
                    setting: PowerGuid::new(setting)?,
                }),
                _ => {
                    return Err(LctrlError::InvalidArgument {
                        detail: "--subgroup and --setting must be provided together".into(),
                    });
                }
            };
            let scheme = resolve_power_scheme(power.power_schemes()?, &name)?;
            if let Some(key) = key {
                let observation = power.read_power_setting(&scheme.id, &key)?;
                let mut human = format!(
                    "power setting observation (non-atomic):\n  scheme: {} ({})\n  subgroup: {}\n  setting: {}\n  AC value: {} (group-policy overridden: {})\n  DC value: {} (group-policy overridden: {})\n",
                    scheme.name,
                    observation.scheme.as_str(),
                    observation.key.subgroup.as_str(),
                    observation.key.setting.as_str(),
                    observation.values.ac,
                    observation.policy_overrides.ac,
                    observation.values.dc,
                    observation.policy_overrides.dc,
                );
                use std::fmt::Write as _;
                match &observation.range {
                    PowerRangeObservation::Available { range } => writeln!(
                        human,
                        "  range: {}..={} (increment: {})",
                        range.min, range.max, range.increment
                    ),
                    PowerRangeObservation::Unavailable { detail } => {
                        writeln!(human, "  range: unavailable ({detail})")
                    }
                }
                .expect("writing to a String cannot fail");
                structured_output(&observation, human)
            } else {
                structured_output(&scheme, format!("power scheme: {}\n", scheme.name))
            }
        }
        SchemeCommand::Apply { name } => {
            let scheme = resolve_power_scheme(power.power_schemes()?, &name)?;
            let mutation = PowerMutation::Activate(scheme.id);
            let report = power.apply_power_mutation(mutation, apply)?;
            let restore = if auto_restore && apply == ApplyMode::Commit {
                Some(restore_power_mutation(power, report.previous())?)
            } else {
                None
            };
            output_with_restore(
                &report,
                "power scheme activation requested\n".into(),
                restore.as_deref(),
            )
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
            // Numeric range metadata may be absent while AC/DC indices still
            // read and write (PowerReadValue{Min,Max,Increment} can return
            // ERROR_FILE_NOT_FOUND on PERFEPP/PERFBOOSTMODE/PROCTHROTTLEMAX);
            // fall back to the caller-provided value without range checking.
            let range = power.power_value_range(&key).ok();
            let value = match &range {
                Some(range) => PowerSettingValue::new(value, range)?,
                None => PowerSettingValue::unchecked(value),
            };
            let mutation = PowerMutation::SetValue { key, source, value };
            let report = power.apply_power_mutation(mutation, apply)?;
            let restore = if auto_restore && apply == ApplyMode::Commit {
                Some(restore_power_mutation(power, report.previous())?)
            } else {
                None
            };
            output_with_restore(
                &report,
                "power setting requested\n".into(),
                restore.as_deref(),
            )
        }
    }
}

/// Re-apply the mutation the transaction captured as `previous`. The power
/// layer verifies each mutation with its own readback/rollback, so a
/// successful return already proves the restore took effect.
fn restore_power_mutation(
    power: &dyn PowerControl,
    previous: &PowerMutation,
) -> lctrl_core::Result<String> {
    power.apply_power_mutation(previous.clone(), ApplyMode::Commit)?;
    Ok(format!("restored power state: {previous}"))
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

fn execute_magicbay_lte_status(
    mobile_broadband: Option<&dyn MobileBroadbandControl>,
) -> CommandResult {
    let mobile_broadband = mobile_broadband.ok_or_else(|| LctrlError::Unsupported {
        feature: "magicbay.lte: no Mobile Broadband status service is attached on this platform"
            .into(),
    })?;
    let status = mobile_broadband.lte_status()?;
    let human = format!(
        "{} MagicBay LTE module(s), {} WWAN interface(s), data session {}\n",
        status.modules.len(),
        status.interfaces.len(),
        if status.data_session { "up" } else { "down" },
    );
    structured_output(&status, human)
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
    auto_restore: bool,
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
        ProfileCommand::Apply { name } => {
            let profile = catalog
                .get(&name)
                .ok_or_else(|| LctrlError::InvalidArgument {
                    detail: format!("tuning profile {name:?} was not found"),
                })?;
            let mode = global_apply;
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
            let restore = if auto_restore {
                // Replay the pre-apply snapshot through the same guarded
                // restore path tune.restore uses, then drop the recovery
                // state — there is nothing left to restore manually.
                let mut restored_changes = Vec::with_capacity(snapshot.len());
                for rollback in snapshot.into_iter().rev() {
                    restored_changes.push(restore_tune_setting(
                        services,
                        rollback,
                        ApplyMode::Commit,
                    )?);
                }
                fs::remove_file(tune_state_path()?).map_err(LctrlError::Io)?;
                Some(format!(
                    "restored tuning profile snapshot ({} setting(s))\n",
                    restored_changes.len()
                ))
            } else {
                None
            };
            output_with_restore(
                &AppliedTunePayload { plan, changes },
                format!("tuning profile {name} applied and read back\n"),
                restore.as_deref(),
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
    apply: ApplyMode,
) -> lctrl_core::Result<Value> {
    match rollback {
        TuneRollback::Battery(mode) => {
            let service = services.battery.ok_or_else(|| LctrlError::Unsupported {
                feature: "battery.charge_mode.restore".into(),
            })?;
            serialize_change(&service.set_charge_mode(mode, apply)?)
        }
        TuneRollback::Backlight(state) => {
            let service = services.keyboard.ok_or_else(|| LctrlError::Unsupported {
                feature: "kbd.backlight.restore".into(),
            })?;
            serialize_change(&service.set_backlight(state.level, state.effect, apply)?)
        }
        TuneRollback::Fan(mode) => {
            let service = services.fan.ok_or_else(|| LctrlError::Unsupported {
                feature: "perf.fan.mode.restore".into(),
            })?;
            serialize_change(&service.set_fan_mode(mode, apply)?)
        }
        TuneRollback::Performance(mode) => {
            let service = services
                .performance
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: "perf.mode.restore".into(),
                })?;
            serialize_change(&service.set_performance_mode(mode, apply)?)
        }
        TuneRollback::Epp(value) => {
            let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                feature: "tune.epp.restore".into(),
            })?;
            serialize_change(&service.set_epp(value, apply)?)
        }
        TuneRollback::PowerLimit(kind, value) => {
            let service = services.tuning.ok_or_else(|| LctrlError::Unsupported {
                feature: format!("tune.{}.restore", power_limit_kind_name(kind)),
            })?;
            serialize_change(&service.set_power_limit(kind, value, apply)?)
        }
    }
}

fn execute_tune_restore(services: &CommandServices<'_>, apply: ApplyMode) -> CommandResult {
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
        match restore_tune_setting(services, desired, apply) {
            Ok(change) => {
                changes.push(change);
                if apply == ApplyMode::Commit {
                    applied_rollbacks.push(inverse);
                }
            }
            Err(error) => return Err(rollback_tune_changes(services, applied_rollbacks, error)),
        }
    }
    if apply == ApplyMode::Commit {
        fs::remove_file(path).map_err(LctrlError::Io)?;
    }
    structured_output(
        &TuneRestorePayload {
            restored_profile: state.active_profile,
            changes,
        },
        if apply == ApplyMode::DryRun {
            "tuning restore dry run validated; no settings or recovery state changed\n".into()
        } else {
            "tuning snapshot restored and read back\n".into()
        },
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
        Ok(())
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

fn execute_update(
    update: Option<&dyn UpdateControl>,
    local_update: Option<&dyn LocalUpdateControl>,
    local_package: Option<&dyn LocalPackageInstallControl>,
    mode: ApplyMode,
    command: UpdateCommand,
) -> CommandResult {
    /// A local package install needs no update-catalogue service; the
    /// catalogue-backed commands require one.
    fn require_update<'a>(
        update: Option<&'a dyn UpdateControl>,
    ) -> lctrl_core::Result<&'a dyn UpdateControl> {
        update.ok_or_else(|| LctrlError::Unsupported {
            feature: "update".into(),
        })
    }
    match command {
        UpdateCommand::Check { .. } => {
            let update = require_update(update)?;
            // Prefer the synced-catalog report when the backend can evaluate
            // it (Windows UDC). Fall back to the capability summary for
            // backends without a readable catalog (Linux / no UDC config).
            match update.check_updates() {
                Ok(report) => {
                    let mut human = format!(
                        "{} package(s) offered by the synced update catalog\n",
                        report.packages.len()
                    );
                    if let Some(sync) = &report.last_sync {
                        human.push_str(&format!("catalog synced: {sync}\n"));
                    }
                    if report.stale {
                        human.push_str("catalog is marked stale\n");
                    }
                    for package in &report.packages {
                        human.push_str(&format!(
                            "  {} {} ({})\n",
                            package.name,
                            package.version,
                            package
                                .update_type
                                .clone()
                                .unwrap_or_else(|| "unknown".into())
                        ));
                    }
                    structured_output(&report, human)
                }
                Err(check_error) => {
                    if matches!(check_error, LctrlError::Unsupported { .. }) {
                        // Backend has no catalog evaluator (Linux): keep the
                        // capability-summary fallback.
                        match update.update_capability()? {
                            capability @ UpdateCapability::TrustedLocalInfOnly => {
                                structured_output(&capability, "update capability checked\n".into())
                            }
                            UpdateCapability::Unavailable { reason } => {
                                Err(LctrlError::ChannelUnavailable {
                                    channel: format!("update catalog: {reason}"),
                                })
                            }
                        }
                    } else {
                        // The catalog exists but the read/eval failed; the real
                        // error is more useful than the generic Unavailable.
                        Err(check_error)
                    }
                }
            }
        }
        UpdateCommand::LocalDriver { command } => execute_local_driver(local_update, mode, command),
        UpdateCommand::LocalInstall { package } => {
            execute_local_package(local_package, package, mode)
        }
        UpdateCommand::Download { .. } => Err(LctrlError::Unsupported {
            feature: "update.download.without-authenticated-manifest".into(),
        }),
        UpdateCommand::Install { .. } => Err(LctrlError::Unsupported {
            feature: "update.install.without-authenticated-manifest".into(),
        }),
        UpdateCommand::History => execute_update_history(require_update(update)?),
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

/// Read the update ledger the machine's own Lenovo System Update add-in wrote.
/// No catalogue, network request, or vendor service is involved.
fn execute_update_history(update: &dyn UpdateControl) -> CommandResult {
    let report = update.update_history()?;
    let unit = match report.ledger {
        UpdateHistoryLedger::InstallEvents => "install event(s)",
        UpdateHistoryLedger::PackageStatus => "package status row(s)",
    };
    let mut human = format!(
        "{} {unit} from {}\n",
        report.entries.len(),
        report.source
    );
    for entry in &report.entries {
        let name = entry.name.as_deref().unwrap_or(&entry.id);
        let version = entry.version.as_deref().unwrap_or("unknown version");
        let when = entry
            .installed_at
            .as_deref()
            .unwrap_or("no recorded install time");
        human.push_str(&format!("  {when}  {name} {version} [{}]\n", entry.status));
    }
    structured_output(&report, human)
}

/// Install a package the UDC service already downloaded, through the
/// package's own signed installer.
fn execute_local_package(
    local_package: Option<&dyn LocalPackageInstallControl>,
    package: PathBuf,
    mode: ApplyMode,
) -> CommandResult {
    let local_package = local_package.ok_or_else(|| LctrlError::Unsupported {
        feature: "update.local-install".into(),
    })?;
    let request = LocalPackageInstallRequest::new(package)?;
    let report = local_package.apply_local_package(&request, mode)?;
    let mut human = format!(
        "package {} {} ({}) id {}\n",
        report.package_name,
        report.package_version,
        report.package_type.as_deref().unwrap_or("unknown type"),
        report.package_id
    );
    match &report.payload {
        Some(payload) => human.push_str(&format!(
            "  payload {} sha256 {}\n",
            payload.file, payload.sha256
        )),
        None => human.push_str(
            "  the package script validates its own payload (no pinned hash declared)\n",
        ),
    }
    human.push_str(&format!("  signer {}\n", report.signer));
    match report.exit_code {
        Some(code) => human.push_str(&format!("  installer exit code {code}\n")),
        None => human.push_str("  dry-run: the installer was not started\n"),
    }
    if let Some(log) = &report.log {
        human.push_str(&format!("  installer log {log}\n"));
    }
    structured_output(&report, human)
}

fn execute_local_driver(
    local_update: Option<&dyn LocalUpdateControl>,
    mode: ApplyMode,
    command: LocalDriverCommand,
) -> CommandResult {
    let local_update = local_update.ok_or_else(|| LctrlError::Unsupported {
        feature: "update.local".into(),
    })?;
    match command {
        LocalDriverCommand::Apply { inf, hardware_id } => {
            let request = LocalDriverUpdateRequest::new(inf, hardware_id)?;
            let report = local_update.apply_local_driver_update(&request, mode)?;
            structured_output(&report, local_driver_apply_message(&report))
        }
        LocalDriverCommand::Rollback {
            device_instance_id,
            class_guid,
            matching_device_id,
            expected_provider,
            expected_description,
            expected_version,
            expected_date,
            expected_section,
            restore_provider,
            restore_description,
            restore_version,
            restore_date,
            restore_section,
        } => {
            let expected = DriverIdentity {
                provider: expected_provider,
                description: expected_description,
                class_guid: class_guid.clone(),
                driver_version: parse_driver_version(&expected_version)?,
                driver_date: expected_date,
                matching_device_id: matching_device_id.clone(),
                inf_section: expected_section,
            };
            let restore = DriverIdentity {
                provider: restore_provider,
                description: restore_description,
                class_guid,
                driver_version: parse_driver_version(&restore_version)?,
                driver_date: restore_date,
                matching_device_id,
                inf_section: restore_section,
            };
            let request = LocalDriverRollbackRequest::new(device_instance_id, expected, restore)?;
            let report = local_update.rollback_local_driver(&request, mode)?;
            structured_output(&report, local_driver_rollback_message(&report))
        }
    }
}

fn local_driver_apply_message(report: &lctrl_core::DriverUpdateReport) -> String {
    match (&report.actual, &report.reboot_required) {
        (Some(actual), Some(true)) => format!(
            "local driver installed for {} ({}); reboot required",
            report.device_instance_id, actual.published_inf
        ),
        (Some(actual), _) => format!(
            "local driver installed for {} ({})",
            report.device_instance_id, actual.published_inf
        ),
        (None, _) => format!(
            "local driver dry-run for {}; no installation performed",
            report.device_instance_id
        ),
    }
}

fn local_driver_rollback_message(report: &lctrl_core::DriverRollbackReport) -> String {
    match &report.actual {
        Some(actual) => format!(
            "local driver rollback for {} restored {}",
            report.device_instance_id, actual.published_inf
        ),
        None => format!(
            "local driver rollback dry-run for {}; backup identity unverified, no restore performed",
            report.device_instance_id
        ),
    }
}

fn parse_driver_version(text: &str) -> lctrl_core::Result<DriverVersion> {
    let parts = text.split('.').map(str::trim).collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err(LctrlError::InvalidArgument {
            detail: format!("driver version must be major.minor.build.revision, got {text}"),
        });
    }
    let mut numbers = [0u16; 4];
    for (slot, part) in numbers.iter_mut().zip(parts.iter()) {
        let value = part
            .parse::<u16>()
            .map_err(|_| LctrlError::InvalidArgument {
                detail: format!(
                    "driver version component must be an unsigned 16-bit integer, got {part}"
                ),
            })?;
        *slot = value;
    }
    Ok(DriverVersion::new(numbers))
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
            let options = lctrl_core::DiagnosticRunOptions::default();
            let results = diagnostics.run_diagnostics(&items, options)?;
            structured_output(
                &results,
                format!("{} diagnostic result(s)\n", results.len()),
            )
        }
    }
}

fn execute_wifi(
    wifi: Option<&dyn WifiSecurityControl>,
    wifi_status: Option<&dyn WifiStatusControl>,
    command: WifiCommand,
) -> CommandResult {
    match command {
        WifiCommand::Status => {
            let wifi_status = wifi_status.ok_or_else(|| LctrlError::Unsupported {
                feature: "wifi.status".into(),
            })?;
            let report = wifi_status.wifi_status()?;
            let human = format_wifi_status(&report);
            structured_output(&report, human)
        }
        WifiCommand::Security => {
            let wifi = wifi.ok_or_else(|| LctrlError::Unsupported {
                feature: "wifi.security".into(),
            })?;
            let report = wifi.wifi_security()?;
            structured_output(
                &report,
                format!(
                    "{} interface(s), {} saved profile(s)\n",
                    report.interfaces.len(),
                    report.saved_profiles.len(),
                ),
            )
        }
        WifiCommand::History => {
            let wifi = wifi.ok_or_else(|| LctrlError::Unsupported {
                feature: "wifi.security".into(),
            })?;
            let snapshot = wifi.wifi_security_history()?;
            structured_output(
                &snapshot,
                format!(
                    "Wi-Fi history: capacity {}, retain_ssid {}, {} observation(s), memory-only\n",
                    snapshot.policy.capacity,
                    snapshot.policy.retain_ssid,
                    snapshot.entries.len(),
                ),
            )
        }
        WifiCommand::ClearHistory => {
            let wifi = wifi.ok_or_else(|| LctrlError::Unsupported {
                feature: "wifi.security".into(),
            })?;
            wifi.clear_wifi_security_history()?;
            structured_output(
                &serde_json::json!({ "cleared": true }),
                "cleared in-memory Wi-Fi observations (no persisted history)\n".into(),
            )
        }
    }
}

fn execute_security(
    security: Option<&dyn SecurityControl>,
    command: SecurityCommand,
) -> CommandResult {
    let security = security.ok_or_else(|| LctrlError::Unsupported {
        feature: "security.status".into(),
    })?;
    match command {
        SecurityCommand::Status => {
            let report = security.security_status()?;
            let human = format_security_status(&report);
            structured_output(&report, human)
        }
    }
}

/// Human rendering of `security status`.
///
/// Each component prints its own state and source, and a value the source did
/// not return prints as `unknown`, so a failed read never looks like "off".
fn format_security_status(report: &SecurityStatusReport) -> String {
    let mut text = String::new();

    let secure_boot = &report.secure_boot;
    let _ = writeln!(text, "secure boot: {}", secure_boot.state.as_str());
    let _ = writeln!(
        text,
        "  source: {} ({})",
        secure_boot.source.as_str(),
        security_source_detail(secure_boot.source)
    );
    let _ = writeln!(text, "  firmware: {}", secure_boot.firmware_type);
    if secure_boot.state != SecureBootState::NotUefi {
        let _ = writeln!(
            text,
            "  secure boot variable: {}",
            yes_no_optional(secure_boot.secure_boot_variable)
        );
        let _ = writeln!(text, "  setup mode: {}", yes_no_optional(secure_boot.setup_mode));
        let _ = writeln!(
            text,
            "  platform key: {}",
            platform_key_summary(
                secure_boot.platform_key_present,
                secure_boot.platform_key_bytes
            )
        );
        let _ = writeln!(
            text,
            "  registry mirror (UEFISecureBootEnabled): {}",
            yes_no_optional(secure_boot.registry_secure_boot_enabled)
        );
    }
    if let Some(detail) = &secure_boot.detail {
        let _ = writeln!(text, "  detail: {detail}");
    }

    let tpm = &report.tpm;
    let _ = writeln!(text, "tpm: {}", tpm.state.as_str());
    let _ = writeln!(
        text,
        "  source: {} ({})",
        tpm.source.as_str(),
        security_source_detail(tpm.source)
    );
    if tpm.state == TpmState::Present {
        let _ = writeln!(
            text,
            "  enabled: {}, activated: {}, owned: {}",
            yes_no_optional(tpm.enabled),
            yes_no_optional(tpm.activated),
            yes_no_optional(tpm.owned)
        );
        if let Some(version) = &tpm.spec_version {
            let _ = writeln!(text, "  spec version: {version}");
        }
        let _ = writeln!(text, "  manufacturer: {}", manufacturer_summary(tpm));
        let firmware = tpm.manufacturer_version.as_deref().unwrap_or("unknown");
        match tpm.manufacturer_version_full.as_deref() {
            Some(full) if Some(full) != tpm.manufacturer_version.as_deref() => {
                let _ = writeln!(text, "  firmware: {firmware} (TPM 2.0 full {full})");
            }
            _ => {
                let _ = writeln!(text, "  firmware: {firmware}");
            }
        }
        if let Some(version) = &tpm.physical_presence_version {
            let _ = writeln!(text, "  physical presence: {version}");
        }
    }
    if let Some(detail) = &tpm.detail {
        let _ = writeln!(text, "  detail: {detail}");
    }

    let bitlocker = &report.bitlocker;
    let _ = writeln!(text, "bitlocker: {}", bitlocker.state.as_str());
    let _ = writeln!(
        text,
        "  source: {} ({})",
        bitlocker.source.as_str(),
        security_source_detail(bitlocker.source)
    );
    if bitlocker.state == BitLockerState::Observed {
        let _ = writeln!(
            text,
            "  {} encryptable volume(s)",
            bitlocker.volumes.len()
        );
        for volume in &bitlocker.volumes {
            text.push_str(&format_bitlocker_volume(volume));
        }
    }
    if let Some(detail) = &bitlocker.detail {
        let _ = writeln!(text, "  detail: {detail}");
    }

    text
}

fn format_bitlocker_volume(volume: &BitLockerVolume) -> String {
    let mut text = String::new();
    let label = volume.drive_letter.as_deref().unwrap_or("(no drive letter)");
    let _ = writeln!(
        text,
        "  {label}: protection={} ({}) conversion={} ({}) method={} ({}) lock={} ({}) progress={}",
        optional_value(volume.protection_status.map(|status| status.as_str())),
        optional_raw(volume.protection_status_raw),
        optional_value(volume.conversion_status.map(|status| status.as_str())),
        optional_raw(volume.conversion_status_raw),
        optional_value(volume.encryption_method.map(|method| method.as_str())),
        optional_raw(volume.encryption_method_raw),
        optional_value(volume.lock_status.map(|status| status.as_str())),
        optional_raw(volume.lock_status_raw),
        optional_raw(volume.encryption_percentage),
    );
    let _ = writeln!(
        text,
        "    type={} initialized_for_protection={} device={}",
        optional_value(volume.volume_type.map(|kind| kind.as_str())),
        yes_no_optional(volume.initialized_for_protection),
        optional_value(volume.device_id.as_deref()),
    );
    if let Some(id) = &volume.persistent_volume_id {
        let _ = writeln!(text, "    persistent volume id: {id}");
    }
    if let Some(detail) = &volume.detail {
        let _ = writeln!(text, "    detail: {detail}");
    }
    text
}

const fn yes_no_optional(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

fn optional_value(value: Option<&str>) -> &str {
    value.unwrap_or("unknown")
}

fn optional_raw(value: Option<u32>) -> String {
    value.map_or_else(|| "unknown".into(), |value| value.to_string())
}

fn platform_key_summary(present: Option<bool>, bytes: Option<usize>) -> String {
    match present {
        Some(true) => bytes.map_or_else(
            || "present".into(),
            |bytes| format!("present ({bytes} bytes)"),
        ),
        Some(false) => "absent".into(),
        None => "unknown".into(),
    }
}

fn manufacturer_summary(tpm: &lctrl_hal::TpmReport) -> String {
    match (&tpm.manufacturer_id_text, tpm.manufacturer_id) {
        (Some(name), Some(id)) => format!("{name} (0x{id:08x})"),
        (Some(name), None) => name.clone(),
        (None, Some(id)) => format!("0x{id:08x}"),
        (None, None) => "unknown".into(),
    }
}

const fn security_source_detail(source: SecuritySource) -> &'static str {
    match source {
        SecuritySource::FirmwareType => "kernel32 GetFirmwareType",
        SecuritySource::UefiFirmwareVariable => {
            "GetFirmwareEnvironmentVariableExW {8be4df61-93ca-11d2-aa0d-00e098032b8c}:SecureBoot"
        }
        SecuritySource::SecureBootStateRegistry => {
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State!UEFISecureBootEnabled"
        }
        SecuritySource::Win32Tpm => "root\\CIMV2\\Security\\MicrosoftTpm:Win32_Tpm",
        SecuritySource::Win32EncryptableVolume => {
            "root\\CIMV2\\Security\\MicrosoftVolumeEncryption:Win32_EncryptableVolume"
        }
    }
}

fn execute_bluetooth(
    bluetooth: Option<&dyn BluetoothControl>,
    command: BluetoothCommand,
) -> CommandResult {
    let bluetooth = bluetooth.ok_or_else(|| LctrlError::Unsupported {
        feature: "bluetooth.list".into(),
    })?;
    // `list` is the only Bluetooth verb; the binding is a compile-time check
    // that a future verb cannot be dispatched through it by accident.
    let BluetoothCommand::List = command;
    let inventory = bluetooth.bluetooth_devices()?;
    let human = format_bluetooth_inventory(&inventory);
    structured_output(&inventory, human)
}

/// Read-only sensor access: the platform's own enumeration for `list`, one
/// report for `read`.
///
/// A bare `sensor` lists, because the listing is the only thing this machine
/// can answer without being told which sensor to read.
fn execute_sensor(
    sensor: Option<&dyn SensorControl>,
    command: Option<&SensorCommand>,
) -> CommandResult {
    let sensor = sensor.ok_or_else(|| LctrlError::Unsupported {
        feature: "sensor.list: no sensor channel is attached on this platform; the Windows \
                  backend enumerates and reads through the public Sensor API and no Linux \
                  sensor plane is bound (docs/evidence/re-sensors-20261105.md §1)"
            .into(),
    })?;
    match command {
        None | Some(SensorCommand::List) => {
            let inventory = sensor.list()?;
            let human = format_sensor_inventory(&inventory);
            structured_output(&inventory, human)
        }
        Some(SensorCommand::Read { target }) => {
            let reading = sensor.read(target)?;
            let human = format_sensor_read(&reading);
            structured_output(&reading, human)
        }
    }
}

/// Human rendering of `sensor list`.
///
/// Readable sensors and sensors that produced no report are two separate sets:
/// the second never carries a value, so "0 lx" cannot be confused with "no
/// reading".
fn format_sensor_inventory(inventory: &SensorInventory) -> String {
    let mut text = format!(
        "sensors (read-only): {} report(s), {} enumerated without a report\n",
        inventory.sensors.len(),
        inventory.unavailable.len(),
    );
    for reading in &inventory.sensors {
        text.push_str(&format_sensor_reading(reading));
        text.push('\n');
    }
    if !inventory.unavailable.is_empty() {
        text.push_str("unavailable (enumerated, no data report)\n");
        for descriptor in &inventory.unavailable {
            text.push_str(&format_sensor_descriptor(descriptor));
            let _ = writeln!(
                text,
                "  no report: {}",
                descriptor
                    .report_status
                    .as_deref()
                    .unwrap_or("the platform returned no report"),
            );
            text.push('\n');
        }
    }
    text
}

/// Human rendering of `sensor read`.
fn format_sensor_read(reading: &SensorReading) -> String {
    let mut text = String::from("sensor read (read-only)\n");
    text.push_str(&format_sensor_reading(reading));
    text
}

/// One sensor's identity, the report it produced, and when.
///
/// The timestamp is always printed: it is what separates a fresh reading of
/// zero from a reading that is stale or absent.
fn format_sensor_reading(reading: &SensorReading) -> String {
    let mut text = format_sensor_descriptor(&reading.descriptor);
    let freshness = &reading.freshness;
    let age = match freshness.age_ms {
        Some(age) => format!("{age} ms"),
        None => "unknown".into(),
    };
    let _ = writeln!(
        text,
        "  report {}  age {age}  {} (window {} ms)",
        reading
            .report_timestamp_utc
            .as_deref()
            .unwrap_or("no timestamp"),
        if freshness.fresh { "fresh" } else { "stale" },
        freshness.stale_after_ms,
    );
    if let Some(lux) = reading.illuminance_lux() {
        let _ = writeln!(
            text,
            "  illuminance {lux} lx ({})",
            if freshness.fresh {
                "fresh reading"
            } else {
                "not a current reading"
            },
        );
    }
    for field in &reading.fields {
        let _ = writeln!(
            text,
            "  {} = {}",
            field.label(),
            sensor_value_text(&field.value)
        );
    }
    text
}

/// One sensor's published identity. A property the platform rejected prints as
/// `unavailable` rather than as an empty string.
fn format_sensor_descriptor(descriptor: &SensorDescriptor) -> String {
    let mut text = format!("{} [#{}]\n", descriptor.label(), descriptor.index);
    let _ = writeln!(
        text,
        "  category {} / type {}",
        sensor_identifier(descriptor.category_name, descriptor.category),
        sensor_identifier(descriptor.sensor_type_name, descriptor.sensor_type),
    );
    let _ = writeln!(
        text,
        "  manufacturer {}  model {}  serial {}",
        descriptor.manufacturer.as_deref().unwrap_or("unavailable"),
        descriptor.model.as_deref().unwrap_or("unavailable"),
        descriptor.serial_number.as_deref().unwrap_or("unavailable"),
    );
    let _ = writeln!(
        text,
        "  device path {}",
        descriptor.device_path.as_deref().unwrap_or("unavailable"),
    );
    let interval = match descriptor.min_report_interval_ms {
        Some(interval) => format!("{interval} ms"),
        None => "unavailable".into(),
    };
    let _ = writeln!(
        text,
        "  state {}  min report interval {interval}  data fields {}",
        descriptor.state.label(),
        descriptor.data_field_count,
    );
    text
}

/// A documented category/type name with its identifier, or the raw identifier
/// when this build has no name for it.
fn sensor_identifier(name: Option<&'static str>, id: SensorGuid) -> String {
    match name {
        Some(name) => format!("{name} {id}"),
        None => format!("undocumented {id}"),
    }
}

/// One reported value as text. An undecoded vartype keeps its number and payload
/// instead of being coerced into a reading.
fn sensor_value_text(value: &SensorValue) -> String {
    match value {
        SensorValue::Real { value } => format!("{value}"),
        SensorValue::Integer { value } => value.to_string(),
        SensorValue::Unsigned { value } => value.to_string(),
        SensorValue::Boolean { value } => {
            if *value {
                "true".into()
            } else {
                "false".into()
            }
        }
        SensorValue::Text { value } => value.clone(),
        SensorValue::Timestamp { utc, filetime } => format!("{utc} (FILETIME {filetime})"),
        SensorValue::Guid { value } => value.to_canonical(),
        SensorValue::Empty => "empty (no value published)".into(),
        SensorValue::Unsupported { vartype, raw_hex } => {
            format!("unsupported vartype {vartype} ({raw_hex})")
        }
    }
}

/// Human rendering of `wifi status`. Absent observations print as `-` so a
/// missing field is visible instead of looking like a value.
fn format_wifi_status(report: &WifiStatusReport) -> String {
    let mut text = format!("{} WLAN interface(s)\n", report.interfaces.len());
    for interface in &report.interfaces {
        let state = match interface.connection_state {
            lctrl_hal::WifiConnectionState::Connected => "connected",
            lctrl_hal::WifiConnectionState::Disconnected => "disconnected",
            lctrl_hal::WifiConnectionState::Unknown => "unknown",
        };
        text.push_str(&format!(
            "{}  {}  {}\n",
            interface.interface_guid,
            interface.interface_description.as_deref().unwrap_or("-"),
            state,
        ));
        if interface.connection_state != lctrl_hal::WifiConnectionState::Connected {
            continue;
        }
        text.push_str(&format!(
            "  ssid={} bssid={} profile={}\n",
            interface.ssid.as_deref().unwrap_or("-"),
            interface.bssid.as_deref().unwrap_or("-"),
            interface.profile_name.as_deref().unwrap_or("-"),
        ));
        text.push_str(&format!(
            "  band={} channel={} center_khz={} signal={} rssi={} phy={}\n",
            interface.band.map_or("-", WifiBand::as_str),
            optional_number(interface.channel),
            optional_number(interface.center_frequency_khz),
            interface
                .signal_percent
                .map_or_else(|| "-".to_owned(), |percent| format!("{percent}%")),
            interface
                .rssi_dbm
                .map_or_else(|| "-".to_owned(), |dbm| format!("{dbm}dBm")),
            interface.phy_type.as_deref().unwrap_or("-"),
        ));
        text.push_str(&format!(
            "  rx_kbps={} tx_kbps={}\n",
            optional_number(interface.rx_rate_kbps),
            optional_number(interface.tx_rate_kbps),
        ));
        text.push_str(&format!(
            "  security={}/{} enabled={} 8021x={}\n",
            interface.authentication.as_deref().unwrap_or("-"),
            interface.cipher.as_deref().unwrap_or("-"),
            optional_flag(interface.security_enabled),
            optional_flag(interface.one_x_enabled),
        ));
    }
    text
}

/// Human rendering of `bluetooth list`.
fn format_bluetooth_inventory(inventory: &BluetoothInventory) -> String {
    let mut text = format!(
        "{} radio(s), {} known device(s)\n",
        inventory.adapters.len(),
        inventory.devices.len(),
    );
    for adapter in &inventory.adapters {
        text.push_str(&format!(
            "radio {}  {}  class={}\n",
            adapter.address,
            adapter.name.as_deref().unwrap_or("-"),
            adapter.major_class.map_or("-", BluetoothMajorClass::as_str),
        ));
    }
    for device in &inventory.devices {
        text.push_str(&format!(
            "device {}  {}  class={}  state={}  last_seen={}  last_used={}\n",
            device.address,
            device.name.as_deref().unwrap_or("-"),
            device.major_class.map_or("-", BluetoothMajorClass::as_str),
            bluetooth_state(device),
            optional_time(device.last_seen.as_ref()),
            optional_time(device.last_used.as_ref()),
        ));
    }
    if inventory.adapters.is_empty() {
        text.push_str("no Bluetooth radio is enumerated on this machine\n");
    }
    text
}

fn bluetooth_state(device: &lctrl_hal::BluetoothDeviceRecord) -> &'static str {
    if device.connected {
        "connected"
    } else if device.authenticated {
        "paired"
    } else if device.remembered {
        "remembered"
    } else {
        "known"
    }
}

fn optional_time(value: Option<&lctrl_hal::BluetoothTime>) -> String {
    value.map_or_else(
        || "-".to_owned(),
        |time| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                time.year, time.month, time.day, time.hour, time.minute, time.second
            )
        },
    )
}

fn optional_number(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn optional_flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "-",
    }
}

fn execute_radio(
    radio: Option<&dyn RadioControl>,
    command: RadioCommand,
    apply: ApplyMode,
) -> CommandResult {
    let radio = radio.ok_or_else(|| LctrlError::Unsupported {
        feature: "radio".into(),
    })?;
    match command {
        RadioCommand::Status => {
            let report = radio.radio_status()?;
            let mut human = String::new();
            for status in &report.radios {
                writeln!(
                    &mut human,
                    "{}: {} ({})",
                    status.kind.as_str(),
                    status.state.as_str(),
                    status.name.as_deref().unwrap_or("unnamed"),
                )
                .expect("writing to a string cannot fail");
                for detail in &status.wlan_interfaces {
                    writeln!(
                        &mut human,
                        "  wlan {} ({}): link {} [{}], {} physical layer(s)",
                        detail.interface_guid,
                        detail.interface_description.as_deref().unwrap_or("no description"),
                        wlan_link_state_name(detail.link_state),
                        detail.link_state_raw,
                        detail.phys.len(),
                    )
                    .expect("writing to a string cannot fail");
                    for phy in &detail.phys {
                        writeln!(
                            &mut human,
                            "    phy {}: software {}, hardware {}",
                            phy.phy_index,
                            phy.software.as_str(),
                            phy.hardware.as_str(),
                        )
                        .expect("writing to a string cannot fail");
                    }
                }
            }
            if !report.absent_kinds.is_empty() {
                let absent = report
                    .absent_kinds
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(&mut human, "absent (no radio enumerated): {absent}")
                    .expect("writing to a string cannot fail");
            }
            writeln!(&mut human, "radio access: {}", report.access.as_str())
                .expect("writing to a string cannot fail");
            structured_output(&report, human)
        }
        RadioCommand::Off { kind, force } => {
            let transition = radio.radio_set_state(kind.kind(), RadioState::Off, force, apply)?;
            structured_output(&transition, radio_transition_human(&transition))
        }
        RadioCommand::On { kind } => {
            let transition = radio.radio_set_state(kind.kind(), RadioState::On, false, apply)?;
            structured_output(&transition, radio_transition_human(&transition))
        }
    }
}

fn radio_transition_human(transition: &lctrl_hal::RadioTransition) -> String {
    let observed = match transition.observed {
        Some(observed) => observed.as_str(),
        None => "not written (dry run)",
    };
    let verb = match transition.mode {
        ApplyMode::DryRun => "would request",
        ApplyMode::Commit => "requested",
    };
    let mut human = format!(
        "{} radio: {} -> {verb} {}, observed {} (access {}, mode {})\n",
        transition.kind.as_str(),
        transition.previous.as_str(),
        transition.requested.as_str(),
        observed,
        transition.access.as_str(),
        match transition.mode {
            ApplyMode::DryRun => "dry_run",
            ApplyMode::Commit => "commit",
        },
    );
    if let Some(rollback) = transition.rollback_observed {
        human.push_str(&format!(
            "rollback: previous state restored, observed {}\n",
            rollback.as_str()
        ));
    }
    human
}

const fn wlan_link_state_name(state: lctrl_hal::WlanLinkState) -> &'static str {
    match state {
        lctrl_hal::WlanLinkState::NotReady => "not_ready",
        lctrl_hal::WlanLinkState::Connected => "connected",
        lctrl_hal::WlanLinkState::AdHocNetworkFormed => "ad_hoc_network_formed",
        lctrl_hal::WlanLinkState::Disconnecting => "disconnecting",
        lctrl_hal::WlanLinkState::Disconnected => "disconnected",
        lctrl_hal::WlanLinkState::Associating => "associating",
        lctrl_hal::WlanLinkState::Discovering => "discovering",
        lctrl_hal::WlanLinkState::Authenticating => "authenticating",
        lctrl_hal::WlanLinkState::Unknown => "unknown",
    }
}

fn execute_fix_rules(machine_fix: Option<&dyn MachineFixControl>) -> CommandResult {
    let machine_fix = machine_fix.ok_or_else(|| LctrlError::Unsupported {
        feature: "fix.rules".into(),
    })?;
    let rules = machine_fix.rules();
    let rows = rules
        .iter()
        .map(|rule| {
            serde_json::json!({
                "id": rule.issue.id,
                "title": rule.issue.title,
                "repair": rule.repair,
                "rollback": rule.rollback,
            })
        })
        .collect::<Vec<_>>();
    structured_output(
        &serde_json::json!({ "rules": rows }),
        format!("{} declared rule(s)\n", rules.len()),
    )
}

fn execute_fix_scan(machine_fix: Option<&dyn MachineFixControl>) -> CommandResult {
    let machine_fix = machine_fix.ok_or_else(|| LctrlError::Unsupported {
        feature: "fix.scan".into(),
    })?;
    let observations = machine_fix.scan()?;
    let present = observations.iter().filter(|o| o.present).count();
    structured_output(
        &observations,
        format!("{} rule(s), {} present\n", observations.len(), present),
    )
}

fn execute_fix_run(
    machine_fix: Option<&dyn MachineFixControl>,
    issue: &str,
    mode: ApplyMode,
) -> CommandResult {
    let machine_fix = machine_fix.ok_or_else(|| LctrlError::Unsupported {
        feature: "fix.run".into(),
    })?;
    let report = machine_fix.fix(issue, mode)?;
    let verb = if mode == ApplyMode::DryRun {
        "plan"
    } else {
        "applied"
    };
    structured_output(&report, format!("fix {} for {}\n", verb, report.issue_id))
}

fn execute_fix_track(machine_fix: Option<&dyn MachineFixControl>) -> CommandResult {
    let machine_fix = machine_fix.ok_or_else(|| LctrlError::Unsupported {
        feature: "fix.track".into(),
    })?;
    let ledger = machine_fix.track();
    structured_output(
        &ledger,
        format!("{} applied fix(es) on record\n", ledger.entries.len()),
    )
}

fn scene_action_kind(action: &SceneAction) -> String {
    match action {
        SceneAction::SetPerformanceMode { mode } => format!("set_performance_mode({mode})"),
        SceneAction::SetFanMode { mode } => format!("set_fan_mode({mode})"),
        SceneAction::SetPanelRefresh { hz } => format!("set_panel_refresh({hz} Hz)"),
        SceneAction::SetPowerScheme { scheme } => format!("set_power_scheme({scheme})"),
    }
}

fn execute_scene_list(scene: Option<&dyn ScenePolicyControl>) -> CommandResult {
    let scene = scene.ok_or_else(|| LctrlError::Unsupported {
        feature: "scene.list".into(),
    })?;
    let rules = scene.rules();
    let rows = rules
        .iter()
        .map(|rule| {
            serde_json::json!({
                "id": rule.id,
                "title": rule.title,
                "event_class": rule.event_class,
                "action": rule.action,
                "rollback": rule.rollback,
            })
        })
        .collect::<Vec<_>>();
    structured_output(
        &serde_json::json!({ "rules": rows }),
        format!("{} declared scene rule(s)\n", rules.len()),
    )
}

fn execute_scene_test(
    scene: Option<&dyn ScenePolicyControl>,
    event: &str,
    rule: Option<&str>,
    mode: ApplyMode,
) -> CommandResult {
    let scene = scene.ok_or_else(|| LctrlError::Unsupported {
        feature: "scene.test".into(),
    })?;
    if let Some(rule_id) = rule {
        // Test a specific rule by id: apply it directly against the named event class.
        let report = scene.apply(rule_id, event, mode)?;
        let human = format!(
            "scene rule {rule_id} on {event}: {}",
            scene_action_kind(&report.requested)
        );
        return structured_output(&report, human);
    }
    let matches = scene.match_event(event);
    let firing = matches.iter().filter(|m| m.matched).count();
    if firing == 0 {
        return structured_output(
            &serde_json::json!({
                "event": event,
                "matched": 0,
                "reason": "no declared scene rule fires on this event class",
            }),
            format!("no scene fires on {event}\n"),
        );
    }
    let first = matches
        .iter()
        .find(|m| m.matched)
        .expect("a firing rule exists when firing > 0");
    let report = scene.apply(&first.rule.id, event, mode)?;
    let verb = if mode == ApplyMode::DryRun {
        "plan"
    } else {
        "applied"
    };
    structured_output(
        &serde_json::json!({
            "event": event,
            "matched": firing,
            "rule": first.rule.id,
            "report": report,
        }),
        format!(
            "scene {verb} for {event}: rule {} ({} of {} rule(s) fire)\n",
            first.rule.id,
            firing,
            matches.len(),
        ),
    )
}

fn execute_scene_track(scene: Option<&dyn ScenePolicyControl>) -> CommandResult {
    let scene = scene.ok_or_else(|| LctrlError::Unsupported {
        feature: "scene.track".into(),
    })?;
    let ledger = scene.track();
    structured_output(
        &ledger,
        format!(
            "{} applied scene action(s) on record\n",
            ledger.entries.len()
        ),
    )
}

/// Execute an on-screen-display command.
///
/// `enable` / `disable` drive the Lenovo Fn OSD preference (the
/// `LenovoFnAndFunctionKeys` user-defined control codes) and report what the
/// registry read-back observed; `test` / `show` paint the replacement overlay.
fn execute_osd(
    osd: Option<&dyn OsdControl>,
    command: Option<OsdCommand>,
    apply: ApplyMode,
) -> CommandResult {
    let osd = osd.ok_or_else(|| LctrlError::Unsupported {
        feature: "osd".into(),
    })?;
    let dry_run = matches!(apply, ApplyMode::DryRun);
    let command = command.unwrap_or(OsdCommand::Test);
    match command {
        OsdCommand::Enable => toggle_fn_osd(osd, true, dry_run),
        OsdCommand::Disable => toggle_fn_osd(osd, false, dry_run),
        OsdCommand::Test => {
            let outcome = osd.show(&["Vantage replacement", "osd test"], dry_run)?;
            structured_output(
                &outcome,
                format!("osd test {}: {}\n", outcome.state, outcome.detail),
            )
        }
        OsdCommand::Show { lines } => {
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            let outcome = osd.show(&refs, dry_run)?;
            structured_output(
                &outcome,
                format!("osd show {}: {}\n", outcome.state, outcome.detail),
            )
        }
    }
}

/// Drive the Lenovo Fn OSD preference. The backend reports the observed
/// before/after status; `osd disable` additionally hides any replacement
/// overlay this process is still showing.
fn toggle_fn_osd(osd: &dyn OsdControl, enabled: bool, dry_run: bool) -> CommandResult {
    let change = osd.set_fn_osd(enabled, dry_run)?;
    // Enabling never leaves an overlay behind, and a dry-run paints nothing.
    let overlay_hidden = if enabled || dry_run { false } else { osd.hide()? };
    let mut human = String::new();
    if dry_run {
        human.push_str("osd dry-run: no control code sent to LenovoFnAndFunctionKeys\n");
    }
    for indicator in [&change.after.capslock, &change.after.numlock] {
        human.push_str(&format!(
            "osd {} {}: {}\n",
            indicator.indicator, indicator.state, indicator.detail
        ));
    }
    if overlay_hidden {
        human.push_str("osd overlay hidden\n");
    }
    structured_output(
        &serde_json::json!({ "fn_osd": change, "overlay_hidden": overlay_hidden }),
        human,
    )
}

/// Execute a global-hotkey command against the native RegisterHotKey backend.
fn execute_hotkey(
    hotkey: Option<&dyn HotkeyControl>,
    command: Option<HotkeyCommand>,
    apply: ApplyMode,
) -> CommandResult {
    let hotkey = hotkey.ok_or_else(|| LctrlError::Unsupported {
        feature: "hotkey".into(),
    })?;
    let dry_run = matches!(apply, ApplyMode::DryRun);
    let command = command.unwrap_or(HotkeyCommand::List);
    match command {
        HotkeyCommand::List => {
            let ids = hotkey.registered();
            structured_output(
                &serde_json::json!({ "registered": ids }),
                format!("{} live global hotkey(s)\n", ids.len()),
            )
        }
        HotkeyCommand::Status => {
            let status = hotkey.status()?;
            let human = render_hotkey_status(&status);
            structured_output(&status, human)
        }
        HotkeyCommand::Register { id, vk } => {
            let outcome = hotkey.register(id, vk, dry_run)?;
            structured_output(
                &outcome,
                format!(
                    "hotkey {id} (vk {vk:#x}) {}: {}\n",
                    outcome.state, outcome.detail
                ),
            )
        }
        HotkeyCommand::Trigger { id } => {
            // A trigger is a dry observation: report whether the id is live.
            let live = hotkey.registered().contains(&id);
            structured_output(
                &serde_json::json!({ "id": id, "live": live }),
                if live {
                    format!("hotkey {id} is registered and live\n")
                } else {
                    format!("hotkey {id} is not registered; register it first\n")
                },
            )
        }
        HotkeyCommand::Unregister { id } => {
            let removed = hotkey.unregister(id)?;
            structured_output(
                &serde_json::json!({ "id": id, "removed": removed }),
                if removed {
                    format!("hotkey {id} released\n")
                } else {
                    format!("hotkey {id} was not registered\n")
                },
            )
        }
    }
}

/// Render the read-only Lenovo Fn hotkey package state for a human reader.
fn render_hotkey_status(status: &lctrl_hal::HotkeyStatus) -> String {
    use std::fmt::Write as _;

    let mut text = String::new();
    let _ = writeln!(
        text,
        "hotkey package {}: {} ({})",
        status.service,
        if status.service_running {
            "running"
        } else {
            "not running"
        },
        status.utility_version.as_deref().unwrap_or("version unknown"),
    );
    let _ = writeln!(
        text,
        "renderers: FnHotkeyUtility.exe {}, FnHotkeyCapsLKNumLK.exe {}",
        if status.utility_running {
            "running"
        } else {
            "not running"
        },
        if status.caps_led_running {
            "running"
        } else {
            "not running"
        },
    );
    let _ = writeln!(
        text,
        "capabilities: fn-lock={} keyboard-layout={} numlock-key={} caps-numlock-user-choice={}",
        status.capabilities.support_fn_lock,
        status.capabilities.support_keyboard_layout,
        status.capabilities.support_numlock_key,
        status.capabilities.user_changed_caps_numlock,
    );
    let _ = writeln!(
        text,
        "fn lock: {} ({})",
        status.fn_lock.state, status.fn_lock.detail
    );
    let _ = writeln!(
        text,
        "fn/ctrl swap: {} ({})",
        status.fn_ctrl_swap.state, status.fn_ctrl_swap.detail
    );
    let current_user = status
        .tables
        .iter()
        .filter(|table| table.scope == "current_user")
        .count();
    let _ = writeln!(
        text,
        "hotkey tables: {current_user} current-user, {} machine",
        status.tables.len() - current_user
    );
    for table in &status.tables {
        let _ = writeln!(
            text,
            "  {} {} ({} mappings)",
            table.scope, table.device, table.mappings
        );
    }
    for note in &status.notes {
        let _ = writeln!(text, "note: {note}");
    }
    text
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
    auto_restore: bool,
) -> CommandResult {
    let keyboard = keyboard.ok_or_else(|| LctrlError::Unsupported {
        feature: "kbd.backlight".into(),
    })?;
    let effect = match effect.unwrap_or(BacklightEffect::Static) {
        BacklightEffect::Static => lctrl_core::LightingEffect::Static,
        BacklightEffect::Breath => lctrl_core::LightingEffect::Breathing,
    };
    let report = keyboard.set_backlight(level, effect, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit {
        let before = report.previous();
        let restored = keyboard.set_backlight(before.level, before.effect, ApplyMode::Commit)?;
        Some(format!(
            "restored keyboard backlight: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        format!(
            "keyboard backlight: requested={} actual={} (previous={})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), ToString::to_string),
            report.previous()
        ),
        restore.as_deref(),
    )
}
fn execute_touchpad(
    touchpad: Option<&dyn TouchpadControl>,
    state: TouchpadAction,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let touchpad = touchpad.ok_or_else(|| LctrlError::Unsupported {
        feature: match state {
            TouchpadAction::Status => "touchpad.status",
            TouchpadAction::Caps => "touchpad.caps",
            TouchpadAction::Recover => "touchpad.recover",
            _ => "touchpad",
        }
        .into(),
    })?;
    let requested = match state {
        TouchpadAction::On => DeviceState::Enabled,
        TouchpadAction::Off => DeviceState::Disabled,
        TouchpadAction::Status => {
            let actual = touchpad.touchpad_state()?;
            return structured_output(
                &serde_json::json!({"actual": actual}),
                format!("touchpad: {actual}\n"),
            );
        }
        TouchpadAction::Caps => {
            let caps = touchpad.touchpad_caps()?;
            return structured_output(&caps, format_touchpad_caps(&caps));
        }
        TouchpadAction::Recover => {
            let report = touchpad.recover_touchpad(apply)?;
            let message = match (apply, report.strategy) {
                (
                    ApplyMode::DryRun,
                    lctrl_core::TouchpadRecoveryStrategy::EnableDisabledLeaves,
                ) => "touchpad recovery dry run; disabled input leaves would be enabled\n",
                (
                    ApplyMode::DryRun,
                    lctrl_core::TouchpadRecoveryStrategy::CycleInputLeaves,
                ) => "touchpad recovery dry run; input leaves would be restarted\n",
                (_, lctrl_core::TouchpadRecoveryStrategy::EnableDisabledLeaves) => {
                    "touchpad input leaves enabled and independently read back\n"
                }
                (_, lctrl_core::TouchpadRecoveryStrategy::CycleInputLeaves) => {
                    "touchpad input leaves restarted and independently read back; readback proves enumeration, not press or haptic function\n"
                }
            };
            return structured_output(&report, message.to_string());
        }
    };
    let report = touchpad.set_touchpad(requested, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit && *report.previous() != requested {
        let restored = touchpad.set_touchpad(*report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored touchpad: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        format!(
            "touchpad: requested={:?} actual={} (previous={:?})\n",
            report.requested(),
            report
                .actual()
                .map_or("dry-run".into(), |value| format!("{value:?}")),
            report.previous(),
        ),
        restore.as_deref(),
    )
}

/// Human form for `touchpad caps`. Every number is either declared by the
/// device or arithmetic over a declared range; the millimetres line says so.
fn format_touchpad_caps(caps: &lctrl_core::TouchpadCapabilities) -> String {
    let mut output = format!(
        "touchpad caps (HID-declared, read-only): {}\n",
        caps.instance_id
    );
    output.push_str(&format!(
        "  collection: 0x{:04X}/0x{:04X} (digitizer touchpad)\n",
        caps.usage.page, caps.usage.usage
    ));
    output.push_str(&format!(
        "  device: vid=0x{:04X} pid=0x{:04X} version=0x{:04X}",
        caps.identity.vendor_id, caps.identity.product_id, caps.identity.version
    ));
    if let Some(product) = &caps.identity.product {
        output.push_str(&format!(" product={product:?}"));
    }
    output.push('\n');
    output.push_str(&format!(
        "  reports: input={}B output={}B feature={}B link-collections={}\n",
        caps.reports.input_bytes,
        caps.reports.output_bytes,
        caps.reports.feature_bytes,
        caps.reports.link_collections
    ));
    output.push_str(&format!(
        "  contacts: declared-collections={} maximum={} identifier-max={} count-field-max={}\n",
        caps.contacts.declared_collections,
        caps.contacts
            .count_maximum
            .map_or_else(|| "unavailable".to_owned(), |value| value.to_string()),
        caps.contacts.identifier_max,
        caps.contacts.count_field_max
    ));
    for (label, axis) in [("x", caps.surface.x), ("y", caps.surface.y)] {
        output.push_str(&format!(
            "  surface {label}: logical {}..{} physical {}..{} unit=0x{:04X} exponent={}\n",
            axis.logical_min,
            axis.logical_max,
            axis.physical_min,
            axis.physical_max,
            axis.unit,
            axis.unit_exponent
        ));
    }
    match caps.surface.derived_size_mm {
        Some(size) => output.push_str(&format!(
            "  surface size: {:.1} mm x {:.1} mm (derived from the declared physical range)\n",
            size.width_mm, size.height_mm
        )),
        None => output.push_str(
            "  surface size: not derived (declared unit is not a length unit this build converts)\n",
        ),
    }
    let button_type = caps.buttons.button_type.map_or_else(
        || "unavailable".to_owned(),
        |kind| {
            let raw = match kind {
                lctrl_core::TouchpadButtonType::Depressible => "0".to_owned(),
                lctrl_core::TouchpadButtonType::NonDepressible => "1".to_owned(),
                lctrl_core::TouchpadButtonType::NonClickable => "2".to_owned(),
                lctrl_core::TouchpadButtonType::Other(raw) => raw.to_string(),
            };
            format!("{kind:?} ({raw})")
        },
    );
    output.push_str(&format!(
        "  buttons: type={button_type} integrated-button={}\n",
        if caps.buttons.integrated_button {
            "declared"
        } else {
            "absent"
        }
    ));
    let mut bits: Vec<&str> = Vec::new();
    for (declared, label) in [
        (caps.support.two_finger, "two-finger"),
        (caps.support.three_finger, "three-finger"),
        (caps.support.four_finger, "four-finger"),
        (caps.support.five_finger, "five-finger"),
        (caps.support.pressure, "pressure"),
        (caps.support.palm_confidence, "palm-confidence"),
        (caps.support.haptic_intensity, "haptic-intensity"),
        (caps.support.button_press_threshold, "button-press-threshold"),
    ] {
        if declared {
            bits.push(label);
        }
    }
    output.push_str(&format!(
        "  support: {}\n",
        if bits.is_empty() {
            "no gesture input declared".to_owned()
        } else {
            bits.join(" ")
        }
    ));
    if let Some(error) = &caps.capability_report_error {
        output.push_str(&format!("  capability report: unreadable ({error})\n"));
    }
    output.push_str(
        "  note: declared capabilities, not a measurement; Windows synthesizes gestures from the declared contacts\n",
    );
    output
}

fn execute_fnlock(
    bios: Option<&dyn BiosControl>,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
    auto_restore: bool,
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
            previous.clone(),
            requested,
            actual,
        ));
    }
    let restore = if auto_restore {
        // Re-stage every captured previous value in one transaction; each is
        // independently verified by restore_bios_change's readback.
        let mut texts = Vec::with_capacity(previous_for_recovery.len());
        for previous in &previous_for_recovery {
            texts.push(restore_bios_change(bios, previous)?);
        }
        Some(texts.concat())
    } else {
        None
    };
    output_with_restore(
        &reports,
        "FnLock BIOS settings saved and read back\n".into(),
        restore.as_deref(),
    )
}

fn execute_fn_ctrl_swap(
    bios: Option<&dyn BiosControl>,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
    auto_restore: bool,
) -> CommandResult {
    execute_persistent_bios_toggle(
        bios,
        "FoolProofFnCtrl",
        state,
        apply,
        confirmed,
        interactive,
        auto_restore,
    )
}

fn execute_privacy(
    privacy: Option<&dyn PrivacyControl>,
    bios: Option<&dyn BiosControl>,
    command: PrivacyCommand,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
    auto_restore: bool,
) -> CommandResult {
    match command {
        PrivacyCommand::Cam {
            action,
            runtime,
            persistent,
        } => {
            // The consent-store read is not a device state; it never takes a
            // layer flag and never shares the toggle path.
            if action == PrivacyCamAction::InUse {
                return execute_camera_inuse(privacy, runtime, persistent);
            }
            match action.toggle() {
                Some(state) => match privacy_layer(runtime, persistent)? {
                    PrivacyLayer::Runtime => {
                        execute_runtime_camera(privacy, state, apply, auto_restore)
                    }
                    PrivacyLayer::Persistent => execute_persistent_bios_toggle(
                        bios,
                        "IntegratedCamera",
                        state,
                        apply,
                        confirmed,
                        interactive,
                        auto_restore,
                    ),
                },
                None => execute_camera_status(privacy, bios, runtime, persistent),
            }
        }
        PrivacyCommand::Mic {
            state,
            runtime,
            persistent,
        } => match privacy_layer(runtime, persistent)? {
            PrivacyLayer::Runtime => {
                execute_runtime_microphone(privacy, state, apply, auto_restore)
            }
            PrivacyLayer::Persistent => execute_persistent_bios_toggle(
                bios,
                "Microphone",
                state,
                apply,
                confirmed,
                interactive,
                auto_restore,
            ),
        },
        PrivacyCommand::Fingerprint { state } => execute_persistent_bios_toggle(
            bios,
            "FingerprintReader",
            state,
            apply,
            confirmed,
            interactive,
            auto_restore,
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
            let (runtime_microphone, runtime_microphone_error) = privacy
                .map_or((None, None), |privacy| {
                    capture_optional(privacy.microphone_state())
                });
            let mut bios_items = Vec::new();
            let mut unpublished = Vec::new();
            if let Some(bios) = bios {
                for name in ["IntegratedCamera", "Microphone", "FingerprintReader"] {
                    if let Some(item) = bios.get(name)? {
                        bios_items.push(item);
                    } else {
                        unpublished.push(name);
                    }
                }
            }
            let payload = PrivacyStatusPayload {
                runtime_camera,
                runtime_camera_error,
                runtime_microphone,
                runtime_microphone_error,
                bios: bios_items,
            };
            // Text mode renders the state the JSON carries. A camera or
            // microphone read that failed keeps its captured error instead of
            // collapsing into a bare "collected" line, and a switch the
            // firmware does not publish is named rather than silently omitted.
            let mut human = String::from("privacy status:\n");
            for (layer, state, error) in [
                (
                    "runtime camera",
                    &payload.runtime_camera,
                    &payload.runtime_camera_error,
                ),
                (
                    "runtime microphone",
                    &payload.runtime_microphone,
                    &payload.runtime_microphone_error,
                ),
            ] {
                let detail = error
                    .as_ref()
                    .map_or_else(|| "service not attached".to_string(), ToString::to_string);
                let _ = match state {
                    Some(state) => writeln!(human, "  {layer}: {state}"),
                    None => writeln!(human, "  {layer}: unavailable ({detail})"),
                };
            }
            for item in &payload.bios {
                let _ = writeln!(human, "  persistent {}: {}", item.name, item.value);
            }
            if !unpublished.is_empty() {
                let _ = writeln!(
                    human,
                    "  firmware publishes no {} switch",
                    unpublished.join(", ")
                );
            }
            structured_output(&payload, human)
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

/// Read-only camera status: the aggregate runtime answer, the per-devnode
/// attribution behind it, the persistent BIOS switch, and the honest state of
/// the occlusion channel.
fn execute_camera_status(
    privacy: Option<&dyn PrivacyControl>,
    bios: Option<&dyn BiosControl>,
    runtime: bool,
    persistent: bool,
) -> CommandResult {
    if runtime || persistent {
        // A layer flag scopes a write; the status read always reports both
        // layers so the operator can compare them.
        return Err(LctrlError::InvalidArgument {
            detail: "privacy cam status reports both layers; drop --runtime/--persistent".into(),
        });
    }
    if privacy.is_none() && bios.is_none() {
        return Err(LctrlError::Unsupported {
            feature: "privacy.cam.status".into(),
        });
    }
    let (runtime_state, runtime_error) =
        privacy.map_or((None, None), |service| capture_optional(service.camera_state()));
    let (devices, devices_error) = privacy.map_or((Vec::new(), None), |service| {
        let (devices, error) = capture_optional(service.camera_devices());
        (devices.unwrap_or_default(), error)
    });
    let persistent_item = match bios {
        Some(bios) => bios.get("IntegratedCamera")?,
        None => None,
    };
    let payload = CameraStatusPayload {
        runtime: runtime_state,
        runtime_error,
        devices,
        devices_error,
        persistent: persistent_item,
        occlusion: CameraOcclusionAvailability {
            supported: false,
            reason: CAMERA_OCCLUSION_REASON,
        },
    };
    structured_output(&payload, "privacy camera status collected\n".into())
}

/// Read-only camera consent / last-use records.
///
/// The answer comes from the Capability Access Manager registry store, so it
/// describes what Windows recorded, not what a camera is doing now. The
/// rendered text and the JSON both carry that caveat and the in-use rule, and
/// no store is merged into another: a `deny` in the machine store and an
/// `allow` in the user store are reported side by side.
fn execute_camera_inuse(
    privacy: Option<&dyn PrivacyControl>,
    runtime: bool,
    persistent: bool,
) -> CommandResult {
    if runtime || persistent {
        // The consent store spans every layer; a layer flag would suggest a
        // write this command never performs.
        return Err(LctrlError::InvalidArgument {
            detail: "privacy cam inuse reads all stores; drop --runtime/--persistent".into(),
        });
    }
    let privacy = privacy.ok_or_else(|| LctrlError::Unsupported {
        feature: "privacy.cam.inuse".into(),
    })?;
    let report = privacy.camera_consent()?;
    let human = render_camera_consent(&report);
    structured_output(&report, human)
}

fn render_camera_consent(report: &lctrl_core::CameraConsentReport) -> String {
    use std::fmt::Write as _;

    let mut text = String::new();
    let _ = writeln!(text, "{}", report.as_recorded);
    let _ = writeln!(text, "in-use rule: {}", report.in_use_rule);
    for scope in &report.scopes {
        if !scope.present {
            let _ = writeln!(text, "{}: no consent-store key", scope_name(scope.scope));
            continue;
        }
        let consent = match (&scope.consent, &scope.consent_raw) {
            (Some(consent), _) => consent_name(*consent),
            (None, Some(raw)) => raw.as_str(),
            (None, None) => "no consent text recorded",
        };
        let _ = write!(
            text,
            "{}: {consent} ({} record(s))",
            scope_name(scope.scope),
            scope.entries.len()
        );
        if let Some(last_set) = &scope.last_set {
            let _ = write!(
                text,
                ", last set {}",
                last_set.utc.as_deref().unwrap_or("undecodable FILETIME")
            );
        }
        text.push('\n');
        for entry in &scope.entries {
            let identity = entry.path.as_deref().unwrap_or(&entry.key);
            let consent = match (&entry.consent, &entry.consent_raw) {
                (Some(consent), _) => consent_name(*consent),
                (None, Some(raw)) => raw.as_str(),
                (None, None) => "no consent text recorded",
            };
            let marker = if entry.in_use { "*" } else { " " };
            let _ = write!(text, "  {marker} {identity}: {consent}");
            if let (Some(start), Some(stop)) = (&entry.last_used_start, &entry.last_used_stop) {
                let _ = write!(
                    text,
                    ", last use {} → {}",
                    start.utc.as_deref().unwrap_or("undecodable FILETIME"),
                    stop.utc.as_deref().unwrap_or("undecodable FILETIME"),
                );
            }
            text.push('\n');
        }
        for note in &scope.notes {
            let _ = writeln!(text, "  ! {note}");
        }
    }
    if report.policy.present {
        let value = match (
            &report.policy.let_apps_access_camera,
            report.policy.let_apps_access_camera_value,
        ) {
            (Some(policy), _) => policy_name(*policy).to_string(),
            (None, Some(raw)) => format!("unrecognised value {raw}"),
            (None, None) => "not configured".to_string(),
        };
        let _ = writeln!(
            text,
            "policy LetAppsAccessCamera: {value} (this is policy, not consent)"
        );
        for (label, list) in [
            ("force-allow", &report.policy.force_allow),
            ("force-deny", &report.policy.force_deny),
        ] {
            if let Some(list) = list {
                let _ = writeln!(text, "policy {label}: {}", list.join(", "));
            }
        }
        for note in &report.policy.notes {
            let _ = writeln!(text, "policy note: {note}");
        }
    } else {
        let _ = writeln!(
            text,
            "policy: no AppPrivacy policy key; no policy value is recorded (this is not an allow)"
        );
    }
    let in_use = report.in_use().count();
    let _ = write!(
        text,
        "{in_use} record(s) satisfy the in-use rule; that is a recorded stamp pair, not a live stream\n"
    );
    text
}

const fn scope_name(scope: lctrl_core::CameraConsentScope) -> &'static str {
    match scope {
        lctrl_core::CameraConsentScope::CurrentUser => "current_user",
        lctrl_core::CameraConsentScope::CurrentUserNonPackaged => "current_user_non_packaged",
        lctrl_core::CameraConsentScope::Machine => "machine",
        lctrl_core::CameraConsentScope::MachineNonPackaged => "machine_non_packaged",
    }
}

const fn consent_name(consent: lctrl_core::CameraConsent) -> &'static str {
    match consent {
        lctrl_core::CameraConsent::Allow => "Allow",
        lctrl_core::CameraConsent::Deny => "Deny",
        lctrl_core::CameraConsent::PromptOnFirstUse => "PromptOnFirstUse",
    }
}

const fn policy_name(policy: lctrl_core::CameraAppPrivacyPolicy) -> &'static str {
    match policy {
        lctrl_core::CameraAppPrivacyPolicy::UserInControl => "0 (user in control)",
        lctrl_core::CameraAppPrivacyPolicy::ForceAllow => "1 (force allow)",
        lctrl_core::CameraAppPrivacyPolicy::ForceDeny => "2 (force deny)",
    }
}

fn execute_runtime_camera(
    privacy: Option<&dyn PrivacyControl>,
    state: Toggle,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let privacy = privacy.ok_or_else(|| LctrlError::Unsupported {
        feature: "privacy.cam.runtime".into(),
    })?;
    let requested = match state {
        Toggle::On => DeviceState::Enabled,
        Toggle::Off => DeviceState::Disabled,
    };
    let report = privacy.set_camera(requested, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit && *report.previous() != requested {
        // The PnP transaction already verified the requested state; restore
        // re-runs the same guarded path for the captured previous value.
        let restored = privacy.set_camera(*report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored runtime camera: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        "runtime camera state requested\n".into(),
        restore.as_deref(),
    )
}

fn execute_runtime_microphone(
    privacy: Option<&dyn PrivacyControl>,
    state: Toggle,
    apply: ApplyMode,
    auto_restore: bool,
) -> CommandResult {
    let privacy = privacy.ok_or_else(|| LctrlError::Unsupported {
        feature: "privacy.mic.runtime".into(),
    })?;
    let requested = match state {
        Toggle::On => DeviceState::Enabled,
        Toggle::Off => DeviceState::Disabled,
    };
    let report = privacy.set_microphone(requested, apply)?;
    let restore = if auto_restore && apply == ApplyMode::Commit && *report.previous() != requested {
        // Same guarded PnP transaction as the write: the backend locates the
        // admitted microphone devnode again and verifies the state.
        let restored = privacy.set_microphone(*report.previous(), ApplyMode::Commit)?;
        Some(format!(
            "restored runtime microphone: {:?} (verified)\n",
            restored.actual()
        ))
    } else {
        None
    };
    output_with_restore(
        &report,
        "runtime microphone state requested\n".into(),
        restore.as_deref(),
    )
}

fn execute_persistent_bios_toggle(
    bios: Option<&dyn BiosControl>,
    name: &str,
    state: Toggle,
    apply: ApplyMode,
    confirmed: bool,
    interactive: bool,
    auto_restore: bool,
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
    let report = lctrl_core::ChangeReport::committed(previous.clone(), requested, actual);
    let restore = if auto_restore {
        Some(restore_bios_change(bios, &previous)?)
    } else {
        None
    };
    output_with_restore(
        &report,
        format!("BIOS {name} saved and read back\n"),
        restore.as_deref(),
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

/// Re-apply `previous` after a verified write and confirm the mirror shows
/// the original value again. Used by `--auto-restore`; a failure is reported
/// as a firmware rejection so the caller never pretends the restore ran.
fn restore_bios_change(
    bios: &dyn BiosControl,
    previous: &BiosChange,
) -> lctrl_core::Result<String> {
    stage_and_save(bios, previous.clone()).map_err(|error| LctrlError::FirmwareRejected {
        detail: format!(
            "BIOS auto-restore failed to re-stage {}: {error}",
            previous.name
        ),
    })?;
    let actual = bios
        .get(previous.name.as_str())?
        .ok_or_else(|| LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS auto-restore: {} vanished after restore",
                previous.name
            ),
        })?;
    if actual.value != previous.value.as_str() {
        return Err(LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS auto-restore: {} reads {} after restore, expected {}",
                previous.name, actual.value, previous.value
            ),
        });
    }
    Ok(format!(
        "restored {}={} (verified)\n",
        previous.name, previous.value
    ))
}

fn stage_and_save_many(bios: &dyn BiosControl, changes: &[BiosChange]) -> lctrl_core::Result<()> {
    bios.require_staging_contract()?;
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

/// Report the Lenovo records the OS event log holds.
///
/// The HAL validates the bound and does all of the reading, so this handler adds
/// no second interpretation: it reports the providers it discovered, the
/// channels that accepted the query and the raw records, newest first. A record
/// whose message resource is empty keeps only its raw `EventData` strings — no
/// message text is invented from them.
fn execute_events_log(event_log: Option<&dyn EventLogControl>, limit: usize) -> CommandResult {
    let event_log = event_log.ok_or_else(|| LctrlError::Unsupported {
        feature: "events.log: no event-log channel is attached on this platform".into(),
    })?;
    let report = event_log.lenovo_events(limit)?;
    let human = format_event_log(&report);
    structured_output(&report, human)
}

fn format_event_log(report: &lctrl_hal::LenovoEventLogReport) -> String {
    let mut text = String::from("Lenovo event records (read-only)\n");
    text.push_str(&format!(
        "  records: {} of at most {}\n",
        report.events.len(),
        report.limit
    ));
    if report.providers.is_empty() {
        text.push_str("  providers: none discoverable on this machine\n");
    } else {
        text.push_str(&format!("  providers: {}\n", report.providers.join(", ")));
    }
    if report.channels.is_empty() {
        text.push_str("  channels: none accepted the query\n");
    } else {
        text.push_str(&format!("  channels: {}\n", report.channels.join(", ")));
    }
    if report.events.is_empty() {
        if report.providers.is_empty() {
            text.push_str("  no Lenovo provider or event source was discoverable\n");
        } else {
            text.push_str("  the discovered providers hold no record on the queried channels\n");
        }
        return text;
    }
    for record in &report.events {
        text.push_str(&format!(
            "  {}  {}  {}  id {}  {}  record {}\n",
            record.time_utc,
            record.channel,
            record.provider,
            record.event_id,
            record.level.name,
            record.record_id,
        ));
        if record.qualifiers != 0 {
            text.push_str(&format!("    qualifiers: {}\n", record.qualifiers));
        }
        // These records carry no message resource, so the raw insertion strings
        // are the whole content; an empty list stays empty.
        if record.data.is_empty() {
            text.push_str("    data: none\n");
        } else {
            for (index, value) in record.data.iter().enumerate() {
                text.push_str(&format!("    data[{index}]: {value}\n"));
            }
        }
    }
    text
}


/// Execute `storage status`: enumerate the physical drives read-only.
///
/// The report is the observation itself, so a drive that refused a field shows
/// `not reported` plus the recorded issue instead of a zero.
fn execute_storage(storage: Option<&dyn StorageStatusControl>) -> CommandResult {
    let storage = storage.ok_or_else(|| LctrlError::Unsupported {
        feature: "storage.status: no read-only storage channel is attached on this platform".into(),
    })?;
    let status = storage.storage_status()?;
    let human = format_storage_status(&status);
    structured_output(&status, human)
}

fn format_storage_status(status: &lctrl_core::StorageStatus) -> String {
    let mut text = String::new();
    if status.devices.is_empty() {
        text.push_str("no physical disk reported\n");
    }
    for device in &status.devices {
        text.push_str(&format!("{}\n", device.device_path));
        if let Some(interface) = &device.interface_path {
            text.push_str(&format!("  interface: {interface}\n"));
        }
        text.push_str(&format!(
            "  model: {}\n",
            device.model.as_deref().unwrap_or("not reported")
        ));
        text.push_str(&format!(
            "  serial ({}): {}\n",
            device.fields.serial_number,
            device.serial_number.as_deref().unwrap_or("not reported")
        ));
        text.push_str(&format!(
            "  device serial ({}): {}\n",
            device.fields.device_serial,
            device.device_serial.as_deref().unwrap_or("not reported")
        ));
        text.push_str(&format!(
            "  firmware: {}\n",
            device.firmware_version.as_deref().unwrap_or("not reported")
        ));
        let bus = match (device.bus_type, device.bus_type_raw) {
            (Some(bus), Some(raw)) => format!("{} (raw {raw})", bus.as_str()),
            (Some(bus), None) => bus.as_str().to_string(),
            (None, Some(raw)) => format!("raw {raw}"),
            (None, None) => "not reported".to_string(),
        };
        text.push_str(&format!("  bus: {bus} ({})\n", device.fields.bus_type));
        text.push_str(&format!(
            "  media: {} ({})\n",
            device
                .media_type
                .map_or("not reported", lctrl_core::StorageMediaType::as_str),
            device.fields.media_type
        ));
        match (device.size_bytes, device.logical_sector_size) {
            (Some(size), Some(sector)) => {
                let gib = size / (1 << 30);
                let tenths = (size % (1 << 30)) * 10 / (1 << 30);
                text.push_str(&format!(
                    "  size: {size} bytes ({gib}.{tenths} GiB), {sector}-byte logical sectors\n"
                ));
            }
            (Some(size), None) => {
                let gib = size / (1 << 30);
                let tenths = (size % (1 << 30)) * 10 / (1 << 30);
                text.push_str(&format!(
                    "  size: {size} bytes ({gib}.{tenths} GiB), logical sector size not reported\n"
                ));
            }
            (None, _) => text.push_str("  size: not reported\n"),
        }
        text.push_str(&format!(
            "  health: {} ({})\n",
            device
                .health
                .map_or("not reported", lctrl_core::StorageHealth::as_str),
            device.fields.health
        ));
        if let Some(warning) = device.critical_warning {
            let asserted: Vec<&str> = [
                (warning.available_spare_below_threshold, "spare below threshold"),
                (warning.temperature_above_threshold, "temperature above threshold"),
                (warning.reliability_degraded, "reliability degraded"),
                (warning.media_read_only, "media read-only"),
                (warning.volatile_memory_backup_failed, "volatile-memory backup failed"),
                (
                    warning.persistent_memory_region_read_only,
                    "persistent-memory region read-only",
                ),
            ]
            .into_iter()
            .filter_map(|(set, label)| set.then_some(label))
            .collect();
            text.push_str(&format!(
                "  critical warning: raw {:#04x}, {}\n",
                warning.raw,
                if asserted.is_empty() {
                    "none asserted".to_string()
                } else {
                    asserted.join(", ")
                }
            ));
        }
        match device.temperature_celsius {
            Some(celsius) => {
                text.push_str(&format!(
                    "  temperature: {celsius} C ({})",
                    device.fields.temperature
                ));
                if let Some(warning) = device.temperature_warning_celsius {
                    text.push_str(&format!(", warns at {warning} C"));
                }
                if let Some(critical) = device.temperature_critical_celsius {
                    text.push_str(&format!(", critical at {critical} C"));
                }
                text.push('\n');
            }
            None => text.push_str("  temperature: not reported\n"),
        }
        for (label, value, source) in [
            (
                "percentage used",
                device.percentage_used.map(|value| format!("{value}%")),
                device.fields.percentage_used,
            ),
            (
                "available spare",
                device
                    .available_spare_percent
                    .map(|spare| match device.available_spare_threshold_percent {
                        Some(threshold) => format!("{spare}% (threshold {threshold}%)"),
                        None => format!("{spare}%"),
                    }),
                device.fields.percentage_used,
            ),
            (
                "power-on hours",
                device.power_on_hours.map(|hours| hours.to_string()),
                device.fields.power_on_hours,
            ),
        ] {
            match value {
                Some(value) => text.push_str(&format!("  {label}: {value} ({source})\n")),
                None => text.push_str(&format!("  {label}: not reported\n")),
            }
        }
        for (label, value) in [
            ("power cycles", device.power_cycles),
            ("unsafe shutdowns", device.unsafe_shutdowns),
            ("media errors", device.media_errors),
        ] {
            match value {
                Some(value) => text.push_str(&format!("  {label}: {value}\n")),
                None => text.push_str(&format!("  {label}: not reported\n")),
            }
        }
        for issue in &device.issues {
            text.push_str(&format!("  issue: {} — {}\n", issue.source, issue.detail));
        }
    }
    for issue in &status.issues {
        text.push_str(&format!("issue: {} — {}\n", issue.source, issue.detail));
    }
    text
}

fn execute_firmware(
    firmware: Option<&dyn FirmwareControl>,
    command: Option<FirmwareCommand>,
) -> CommandResult {
    let firmware = firmware.ok_or_else(|| LctrlError::Unsupported {
        feature: "firmware.status: no firmware inventory channel is attached on this platform"
            .into(),
    })?;
    match command {
        Some(FirmwareCommand::Status) | None => execute_firmware_status(firmware),
    }
}

/// Report the firmware identity, the ESRT entries, and the recorded update
/// attempts. Every value is an observation of the platform's own projection;
/// nothing is normalised into a verdict.
fn execute_firmware_status(firmware: &dyn FirmwareControl) -> CommandResult {
    let status = firmware.firmware_status()?;
    let identity = &status.identity;
    let vendor = identity.vendor.as_deref().unwrap_or("vendor not reported");
    let version = identity.version.as_deref().unwrap_or("version not reported");
    let mut human = format!("{} firmware: {vendor} {version}", identity.firmware_type);
    if let Some(date) = &identity.release_date {
        human.push_str(&format!(", dated {date}"));
    }
    human.push('\n');
    for (label, value) in [
        ("BIOS release", identity.bios_release()),
        ("EC firmware", identity.ec_firmware()),
        ("SMBIOS", identity.smbios_version()),
    ] {
        if let Some(value) = value {
            human.push_str(&format!("  {label}: {value}\n"));
        }
    }
    if !identity.version_strings.is_empty() {
        human.push_str(&format!("  version strings: {}\n", identity.version_strings.join(", ")));
    }
    human.push_str(&format!("ESRT entries: {}\n", status.resources.len()));
    for resource in &status.resources {
        let name = resource
            .description
            .as_deref()
            .unwrap_or("no device description");
        let state = resource.status.as_deref().unwrap_or("status not reported");
        let version = match resource.reported_version() {
            Some(version) => lctrl_core::format_firmware_version(version),
            None => "version absent".into(),
        };
        human.push_str(&format!(
            "  {}  {name}  version {version}  [{state}]\n",
            resource.firmware_class
        ));
        // The store value is printed only when the documented derivation also
        // answered; otherwise it is the fallback already shown above.
        if let (Some(_), Some(stored)) = (resource.current_version, resource.stored_version) {
            human.push_str(&format!(
                "    property store: {}\n",
                lctrl_core::format_firmware_version(stored)
            ));
        }
        for issue in &resource.issues {
            human.push_str(&format!("    issue: {issue}\n"));
        }
    }
    if status.attempts.is_empty() {
        human.push_str("update attempts: none recorded\n");
    } else {
        human.push_str(&format!("update attempts: {}\n", status.attempts.len()));
    }
    for attempt in &status.attempts {
        let version = attempt
            .version
            .map(lctrl_core::format_firmware_version)
            .unwrap_or_else(|| "version absent".into());
        let last_attempt = attempt
            .last_attempt_version
            .map(lctrl_core::format_firmware_version)
            .unwrap_or_else(|| "version absent".into());
        // An unnamed status is printed as its raw number; only the documented
        // values get a word.
        let outcome = match (&attempt.last_attempt_outcome, attempt.last_attempt_status) {
            (Some(meaning), Some(status)) => format!("{status} ({meaning})"),
            (None, Some(status)) => format!("{status} (unnamed)"),
            (_, None) => "not recorded".into(),
        };
        human.push_str(&format!(
            "  {}  version {version}, last attempt {last_attempt}, status {outcome}\n",
            attempt.firmware_class
        ));
        if let Some(phase) = attempt.phase {
            human.push_str(&format!("    phase: {phase}\n"));
        }
        if let Some(filename) = &attempt.filename {
            human.push_str(&format!("    image: {filename}\n"));
        }
        if let Some(catalog) = &attempt.catalog {
            human.push_str(&format!("    catalog: {catalog}\n"));
        }
    }
    for issue in &status.issues {
        human.push_str(&format!("issue: {issue}\n"));
    }
    structured_output(&status, human)
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
    auto_restore: bool,
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
                    "BIOS write: key={name} value={value}; the save commits the machine-global staging buffer (any pending changes from other tools are committed too); effect timing may require reboot; recovery: vantage bios set {name} {} --save --yes",
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
            let report = lctrl_core::ChangeReport::committed(previous.clone(), requested, actual);
            let restore = if auto_restore {
                Some(restore_bios_change(bios, &previous)?)
            } else {
                None
            };
            output_with_restore(
                &report,
                "BIOS setting saved and read back\n".into(),
                restore.as_deref(),
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
                confirm_risky_change(
                    confirmed,
                    interactive,
                    "BIOS discard clears the machine-global staging buffer: it drops every pending SetBiosSetting, including any staged by other tools",
                )?;
                bios.require_staging_contract()?;
                bios.discard()?;
                structured_output(&(), "BIOS staged changes discarded\n".into())
            }
        }
        BiosCommand::Password {
            command: Some(PasswordCommand::Status),
        } => {
            let status = bios.password_status()?;
            // Text mode carries the observed state: the previous line reported
            // only that a read happened while JSON held the answer.
            let human = format!(
                "BIOS supervisor switch status (set: {}; length {}..={}):\n{}\n",
                status.supervisor_set,
                status.min_length,
                status.max_length,
                serde_json::to_string_pretty(&status)
                    .map_err(|error| LctrlError::Io(io::Error::other(error)))?
            );
            structured_output(&status, human)
        }
        BiosCommand::Defaults { password } => {
            if apply == ApplyMode::DryRun {
                structured_output(
                    &serde_json::json!({ "operation": "bios_defaults", "mode": "dry_run" }),
                    "BIOS defaults dry run validated; no settings were reset\n".into(),
                )
            } else {
                confirm_risky_change(
                    confirmed,
                    interactive,
                    "BIOS defaults resets every setup option to factory values: it discards staged changes and overwrites persistent settings such as Fn-lock, USB behaviour, and boot order",
                )?;
                bios.load_defaults(password.as_deref())?;
                structured_output(&(), "BIOS setup reset to factory defaults\n".into())
            }
        }
        BiosCommand::Password {
            command: Some(PasswordCommand::Set { .. } | PasswordCommand::Clear { .. }),
        } => Err(LctrlError::Unsupported {
            feature: "bios.password.write".into(),
        }),
        BiosCommand::Password {
            command: Some(PasswordCommand::Verify { value }),
        } => {
            let verified = bios.verify_password(value.as_str())?;
            structured_output(
                &serde_json::json!({ "verified": verified }),
                if verified {
                    "BIOS supervisor password verified\n".into()
                } else {
                    "BIOS supervisor password rejected\n".into()
                },
            )
        }
        BiosCommand::Password { command: None } => Err(LctrlError::InvalidArgument {
            detail: "a password subcommand is required".into(),
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
    let identity = hal.system_identity()?;
    let identity = (!identity.is_empty()).then_some(identity);
    let human = format_info(&platform, &hardware, identity.as_ref(), &capabilities);
    let payload = InfoPayload {
        platform,
        hardware,
        identity,
        features: capabilities.features,
    };
    let json = serde_json::to_value(payload)
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;

    Ok(CommandOutput { human, json })
}

fn format_info(
    platform: &Platform,
    hardware: &HardwareInfo,
    identity: Option<&SystemIdentity>,
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
    if let Some(identity) = identity {
        let ec_version = identity.bios.embedded_controller_version();
        output.push_str("identity:\n");
        for (label, value) in [
            // Machine type model: SMBIOS Product Name (Win32_ComputerSystem.Model).
            ("model (mtm)", identity.product.model.as_deref()),
            // Marketing product version (Win32_ComputerSystemProduct.Version).
            ("product version", identity.product.version.as_deref()),
            (
                "serial",
                identity.product.identifying_number.as_deref(),
            ),
            // Win32_BaseBoard.Product: Lenovo's internal board id, not the MTM.
            ("baseboard", identity.motherboard.product.as_deref()),
            ("baseboard version", identity.motherboard.version.as_deref()),
            // DMTF datetime text exactly as Win32_BIOS.ReleaseDate delivered it.
            ("bios release date", identity.bios.release_date.as_deref()),
            // SMBIOS Type 0 embedded-controller firmware revision.
            ("ec firmware", ec_version.as_deref()),
        ] {
            output.push_str("  ");
            output.push_str(label);
            output.push_str(": ");
            output.push_str(value.unwrap_or("unknown"));
            output.push('\n');
        }
    }
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

/// Accessory presence as observed by the detect service.
///
/// The registry gates the accessory-only rows on this observation: without an
/// attached accessory they are `not_applicable` rather than a failed probe, and
/// a failed inventory stays `unknown` instead of being read as absence.
fn magicbay_hardware(magicbay: Option<&dyn MagicBayControl>) -> lctrl_hal::HardwareObservation {
    let Some(magicbay) = magicbay else {
        return lctrl_hal::HardwareObservation::default();
    };
    let magicbay = match magicbay.detect_magicbay() {
        Ok(inventory) => {
            if inventory.devices.is_empty() && inventory.acpi_devices.is_empty() {
                lctrl_core::HardwarePresence::Absent
            } else {
                lctrl_core::HardwarePresence::Present
            }
        }
        Err(_) => lctrl_core::HardwarePresence::Unknown,
    };
    lctrl_hal::HardwareObservation { magicbay }
}

fn execute_capability(services: &CommandServices<'_>) -> CommandResult {
    let capabilities = services.hal.capabilities()?;
    let rows = capability::evaluate_commands_with_hardware(
        services.hal,
        &capabilities,
        magicbay_hardware(services.magicbay),
    );
    let human = format_capabilities(&rows);
    structured_output(&rows, human)
}

fn format_capabilities(rows: &[capability::ObservedCommandCapability]) -> String {
    let mut counts = std::collections::BTreeMap::new();
    let mut output = String::new();
    for row in rows {
        output.push_str(row.command.as_str());
        output.push('\t');
        output.push_str(&json_label(&row.layer));
        output.push('\t');
        let status = json_label(&row.status);
        output.push_str(&status);
        if let Some(detail) = &row.detail {
            output.push('\t');
            output.push_str(detail);
        }
        output.push('\n');
        *counts.entry(status).or_insert(0usize) += 1;
    }
    for (status, count) in counts {
        output.push_str(&format!("{status}: {count}\n"));
    }
    output
}

fn json_label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// Read-only processor status: the platform observation plus the processor
/// policies stored in the active power scheme.
#[derive(Debug, Serialize)]
struct CpuStatusPayload {
    #[serde(flatten)]
    cpu: lctrl_core::CpuStatus,
    /// `PERFEPP` percentage per power source, as
    /// `PowerReadACValueIndex`/`PowerReadDCValueIndex` return it.
    epp_percent: PowerSourceValues<u32>,
    /// What `PowerReadValue{Min,Max,Increment}` publishes for `PERFEPP`.
    epp_range: PowerRangeObservation,
    /// `PERFBOOSTMODE` index per power source. An index rather than a boolean:
    /// the target stores `2` (aggressive), which no boolean request selects.
    boost_mode: PowerSourceValues<ProcessorBoostMode>,
    /// What `PowerReadValue{Min,Max,Increment}` publishes for `PERFBOOSTMODE`.
    /// `Unavailable` on the target: all three calls return
    /// `ERROR_FILE_NOT_FOUND`, so no bound is claimed.
    boost_range: PowerRangeObservation,
}

/// `SUB_PROCESSOR`: the subgroup the processor policies live in.
const PROCESSOR_SETTINGS_SUBGROUP: &str = "54533251-82be-4824-96c1-47b60b740d00";
/// `PERFEPP`.
const PROCESSOR_EPP_SETTING: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6863";
/// `PERFBOOSTMODE`.
const PROCESSOR_BOOST_SETTING: &str = "be337238-0d82-4146-a960-4f3749d470c7";

fn execute_cpu_status(
    hal: &dyn Hal,
    power: Option<&dyn PowerControl>,
    processor_power: Option<&dyn ProcessorPowerControl>,
) -> CommandResult {
    // The processor observation is reported first so a platform without a
    // clock source fails with its own honest reason.
    let cpu = hal.cpu_status()?;
    let processor = processor_power.ok_or_else(|| LctrlError::Unsupported {
        feature: "cpu.status: no processor power service".into(),
    })?;
    // Both values come from the documented power API on the active scheme. The
    // ranges are read separately: a setting whose bounds
    // `PowerReadValue{Min,Max,Increment}` refuses to publish (PERFBOOSTMODE
    // returns ERROR_FILE_NOT_FOUND on the target) is reported as an unavailable
    // range, never as a guessed 0..=4.
    let epp_percent = processor.epp_percent_ac_dc()?;
    let boost_mode = processor.boost_ac_dc()?;
    let epp_range = processor_setting_range(power, PROCESSOR_EPP_SETTING)?;
    let boost_range = processor_setting_range(power, PROCESSOR_BOOST_SETTING)?;
    let human = format_cpu_status(&cpu, epp_percent, &epp_range, boost_mode, &boost_range);
    structured_output(
        &CpuStatusPayload {
            cpu,
            epp_percent,
            epp_range,
            boost_mode,
            boost_range,
        },
        human,
    )
}

/// Read one processor setting's advertised range from the active scheme.
///
/// A range the API refuses to publish becomes
/// [`PowerRangeObservation::Unavailable`] with the API's own error text; any
/// other failure is returned rather than flattened into "no range", matching
/// `PowerControl::read_power_setting`.
fn processor_setting_range(
    power: Option<&dyn PowerControl>,
    setting: &str,
) -> lctrl_core::Result<PowerRangeObservation> {
    let Some(power) = power else {
        return Ok(PowerRangeObservation::Unavailable {
            detail: "no power control service".into(),
        });
    };
    let key = PowerSettingKey {
        subgroup: PowerGuid::new(PROCESSOR_SETTINGS_SUBGROUP)?,
        setting: PowerGuid::new(setting)?,
    };
    match power.power_value_range(&key) {
        Ok(range) => Ok(PowerRangeObservation::Available { range }),
        Err(
            error @ (LctrlError::Unsupported { .. } | LctrlError::ChannelUnavailable { .. }),
        ) => Ok(PowerRangeObservation::Unavailable {
            detail: format!("{setting}: {error}"),
        }),
        Err(error) => Err(error),
    }
}

fn format_cpu_status(
    cpu: &lctrl_core::CpuStatus,
    epp_percent: PowerSourceValues<u32>,
    epp_range: &PowerRangeObservation,
    boost_mode: PowerSourceValues<ProcessorBoostMode>,
    boost_range: &PowerRangeObservation,
) -> String {
    let mut text = String::new();
    text.push_str("processor identity (read-only)\n");
    push_labelled(&mut text, "name", cpu.name.as_deref(), "");
    push_labelled(&mut text, "vendor", cpu.vendor.as_deref(), "");
    push_labelled(&mut text, "identifier", cpu.identifier.as_deref(), "");
    push_labelled(&mut text, "cores", cpu.cores, "");
    push_labelled(&mut text, "threads", cpu.threads, "");
    push_labelled(&mut text, "L2 cache", cpu.l2_cache_kb, " KB");
    push_labelled(&mut text, "L3 cache", cpu.l3_cache_kb, " KB");

    text.push_str("rated clocks (firmware-declared; not a running frequency)\n");
    push_labelled(
        &mut text,
        "Win32_Processor.MaxClockSpeed",
        cpu.firmware_max_clock_mhz,
        " MHz",
    );
    push_labelled(
        &mut text,
        "registry ~MHz",
        cpu.firmware_approximate_clock_mhz,
        " MHz",
    );

    text.push_str("current clock\n");
    match cpu.current_clock_mhz {
        Some(mhz) => {
            let _ = writeln!(
                text,
                "derived: {mhz} MHz (highest logical processor; rated MHz × percent processor \
                 performance / 100)"
            );
        }
        None => {
            let detail = cpu
                .current_clock_detail
                .as_deref()
                .unwrap_or("no processor performance sample");
            let _ = writeln!(text, "derived: unavailable ({detail})");
        }
    }
    let _ = writeln!(
        text,
        "source: Win32_PerfFormattedData_Counters_ProcessorInformation \
         (ProcessorFrequency × PercentProcessorPerformance / 100)"
    );
    for cluster in &cpu.clusters {
        let processors = cluster
            .logical_processors
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let percent = cluster
            .percent_processor_performance
            .map(|percent| format!("{percent}%"))
            .unwrap_or_else(|| "no sample".into());
        match cluster.current_clock_mhz {
            Some(mhz) => {
                let _ = writeln!(
                    text,
                    "  domain {} MHz rated (logical processors {processors}): {percent} → {mhz} MHz",
                    cluster.rated_clock_mhz
                );
            }
            None => {
                let _ = writeln!(
                    text,
                    "  domain {} MHz rated (logical processors {processors}): {percent} → no \
                     frequency (0 percent carries no sample)",
                    cluster.rated_clock_mhz
                );
            }
        }
    }

    text.push_str("processor power policy (active scheme, PowerRead{AC,DC}ValueIndex)\n");
    let _ = writeln!(
        text,
        "  EPP: AC {}% / DC {}% ({})",
        epp_percent.ac,
        epp_percent.dc,
        format_power_range(epp_range)
    );
    let _ = writeln!(
        text,
        "  boost mode: AC {} ({}) / DC {} ({}) ({})",
        boost_mode.ac.get(),
        boost_mode_label(boost_mode.ac),
        boost_mode.dc.get(),
        boost_mode_label(boost_mode.dc),
        format_power_range(boost_range),
    );
    text
}

/// Render an advertised range, or the API's own reason for not publishing one.
fn format_power_range(range: &PowerRangeObservation) -> String {
    match range {
        PowerRangeObservation::Available { range } => {
            format!("range: {}..={} (increment: {})", range.min, range.max, range.increment)
        }
        PowerRangeObservation::Unavailable { detail } => format!("range: unavailable ({detail})"),
    }
}

/// Render one optional observation, keeping an absent value visible.
fn push_labelled<T: std::fmt::Display>(
    text: &mut String,
    label: &str,
    value: Option<T>,
    suffix: &str,
) {
    match value {
        Some(value) => {
            let _ = writeln!(text, "{label}: {value}{suffix}");
        }
        None => {
            let _ = writeln!(text, "{label}: unavailable");
        }
    }
}

/// The documented `PERFBOOSTMODE` index labels; an unpublished index is
/// reported as an unrecognized index rather than folded into a boolean.
fn boost_mode_label(mode: ProcessorBoostMode) -> &'static str {
    match mode.get() {
        0 => "disabled",
        1 => "enabled",
        2 => "aggressive",
        3 => "efficient-enabled",
        4 => "efficient-aggressive",
        _ => "unrecognized index",
    }
}

/// The parent command a bare invocation left without a subcommand, if any.
///
/// Only the "nothing was asked" case is listed. Subcommands that default to a
/// read (`battery`, `bios`, `hotkey`, `fix`, `scene`, `power scheme`) are not
/// usage errors, and every `Unsupported` reason still describes a feature the
/// code refuses rather than an argument the caller forgot.
fn missing_subcommand(command: &Command) -> Option<&'static str> {
    match command {
        Command::Power { command: None } => Some("power"),
        Command::Perf { command: None } => Some("perf"),
        Command::Tune { command: None } => Some("tune"),
        Command::Tune {
            command: Some(TuneCommand::Profile { command: None }),
        } => Some("tune profile"),
        Command::Kbd { command: None } => Some("kbd"),
        Command::Panel { command: None } => Some("panel"),
        Command::Display { command: None } => Some("display"),
        Command::Privacy { command: None } => Some("privacy"),
        Command::Magicbay { command: None } => Some("magicbay"),
        Command::Magicbay {
            command: Some(MagicbayCommand::Lte { command: None }),
        } => Some("magicbay lte"),
        Command::Update { command: None } => Some("update"),
        Command::Scan { command: None } => Some("scan"),
        Command::Snapshot { command: None } => Some("snapshot"),
        Command::Wifi { command: None } => Some("wifi"),
        Command::Bluetooth { command: None } => Some("bluetooth"),
        Command::Radio { command: None } => Some("radio"),
        Command::Security { command: None } => Some("security"),
        Command::Daemon { command: None } => Some("daemon"),
        _ => None,
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
        } => {
            "custom charge thresholds have no verified native write and readback contract; fixed conservation mode is not a custom threshold interface (docs/02-power-battery.md §4.2)"
        }
        Command::Battery {
            command: Some(BatteryCommand::ExtremeLife { .. }),
        } => {
            "full extreme-life equivalence requires an owned signed kernel Job lifecycle implementation and the target UI toggle mapping; a partial power-plan or EcoQoS substitute is not implemented (docs/evidence/extreme-life-re-20261102.md)"
        }
        Command::Power {
            command: Some(PowerCommand::SaverOnce),
        } => "composite saver workflow has no transactional executor",
        Command::Perf {
            command: Some(PerfCommand::Fan { command: None }),
        } => "a fan subcommand is required",
        Command::Perf {
            command: Some(PerfCommand::Temp { command: None }),
        } => "a temperature subcommand is required",
        Command::Tune {
            command: Some(TuneCommand::Turbo { .. }),
        } => "no verified turbo mutator is attached",
        Command::Kbd {
            command: Some(KbdCommand::Winlock { .. }),
        } => {
            "firmware rejects the Win-key methods: LENOVO_GAMEZONE_DATA.GetWinKeyStatus/SetWinKeyStatus return WBEM_E_INVALID_OBJECT on this SKU (docs/evidence/wmi-capability-exhaustion-20261104.md §4)"
        }
        Command::Panel {
            command: Some(PanelCommand::Color { .. }),
        } => {
            "firmware rejects the gamut plane: every LENOVO_PANEL_METHOD method including Panel_Set_Gamut_Switch returns WBEM_E_NOT_SUPPORTED on this SKU, so no user-mode write path exists (docs/evidence/wmi-capability-exhaustion-20261104.md §2)"
        }
        Command::Panel {
            command: Some(PanelCommand::Overdrive { .. }),
        } => {
            "firmware advertises no overdrive feature: LENOVO_CAPABILITY_DATA_00 lists no 0x1A family, the GAMEZONE/panel MPRT plane rejects, and the TCON device (\\\\.\\SPBTESTTOOL) is absent (docs/evidence/re-panel-20261105.md §3)"
        }
        Command::Sense {
            command: Some(SenseCommand::KbdLightAuto { .. }),
        } => {
            "automatic keyboard backlight has no consuming service on this model: ThinkSmartSenseAddin 1.0.0.77 targets LPlatSvc/LITSSvc registry parameters, and both services are absent on 21VG (docs/interfaces.md §13.3)"
        }
        Command::Sense { .. } => "a sense subcommand is required",
        Command::Audio { .. } => "an audio subcommand is required",
        Command::Magicbay {
            command: Some(MagicbayCommand::Detect),
        } => "MagicBay detection service is unavailable",
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Connect | LteCommand::Disconnect | LteCommand::Apn { .. }),
                }),
        } => {
            "modem session control has no bindable API on this build: mbnapi.dll is absent and the WWAN Service API is COM-only, with its interface contract (WDK wwanapi.h) unavailable and no module attached to exercise it (docs/evidence/re-magicbay-20261105.md §3.2)"
        }
        Command::Magicbay {
            command: Some(MagicbayCommand::Cam),
        } => {
            "MagicBay cameras enumerate as plain UVC devices with no Lenovo identifier, so a camera cannot be attributed to the accessory without the 17EF enclosure (docs/06-magicbay.md §5.3)"
        }
        Command::Magicbay {
            command: Some(MagicbayCommand::Display),
        } => {
            "the display bridge is an ACPI device (ACPI\\QCOM2488 / ACPI\\QCOM24B7) already reported by `magicbay detect`; EDID and mode switching are a display-configuration surface, not an accessory control (docs/06-magicbay.md §6)"
        }
        Command::Magicbay {
            command: Some(MagicbayCommand::Watch),
        } => {
            "hot-plug streaming belongs to the daemon event source; the recovered contract is WM_DEVICECHANGE on GUID_DEVINTERFACE_USB_DEVICE (docs/evidence/re-magicbay-20261105.md §2.3)"
        }
        Command::Daemon { .. } => "daemon commands are handled by the platform binary",
        Command::Wifi {
            command: Some(WifiCommand::Status),
        } => {
            "no WLAN query channel is attached on this platform: the Windows backend reads the \
             live association and cached BSS list through wlanapi, and no Linux WLAN plane is \
             bound (docs/evidence/re-network-20261105.md §1)"
        }
        Command::Bluetooth { .. } => {
            "no Bluetooth channel is attached on this platform: the Windows backend enumerates \
             radios and known devices through bluetoothapis, and no Linux Bluetooth plane is \
             bound (docs/evidence/re-network-20261105.md §1)"
        }
        Command::Radio { .. } => {
            "no radio channel is attached on this platform: the Windows backend drives the WinRT \
             Windows.Devices.Radios contract over the OS radio-management plane, and no Linux \
             radio plane is bound (docs/evidence/re-radio-20261105.md §1)"
        }
        Command::Sensor { .. } => {
            "no sensor channel is attached on this platform: the Windows backend enumerates and \
             reads through the public Sensor API (SensorsApi), and no Linux sensor plane is bound \
             (docs/evidence/re-sensors-20261105.md §1)"
        }
        Command::Security { .. } => {
            "no security posture channel is attached on this platform: the Windows backend reads \
             the UEFI SecureBoot/SetupMode/PK variables, Win32_Tpm, and Win32_EncryptableVolume \
             read-only, and no Linux equivalent is bound \
             (docs/evidence/re-security-20261105.md §6)"
        }
        _ => "no verified implementation is attached on this platform",
    }
}

fn command_feature(command: &Command) -> String {
    match command {
        Command::Info => "info".into(),
        Command::Doctor => "doctor".into(),
        Command::Capability => "capability".into(),
        Command::Events { .. } => "events.log".into(),
        Command::Cpu { command } => prefixed("cpu", command.as_ref().map(CpuCommand::feature)),
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
        Command::Touchpad {
            state: TouchpadAction::Status,
        } => "touchpad.status".into(),
        Command::Touchpad {
            state: TouchpadAction::Caps,
        } => "touchpad.caps".into(),
        Command::Touchpad {
            state: TouchpadAction::Recover,
        } => "touchpad.recover".into(),
        Command::Touchpad { .. } => "touchpad".into(),
        Command::Panel { command } => {
            prefixed("panel", command.as_ref().map(PanelCommand::feature))
        }
        Command::Display { command } => {
            prefixed("display", command.as_ref().map(DisplayCommand::feature))
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
        Command::Firmware { command } => {
            prefixed("firmware", command.as_ref().map(FirmwareCommand::feature))
        }
        Command::Storage { command } => {
            prefixed("storage", command.as_ref().map(StorageCommand::feature))
        }
        Command::Magicbay { command } => {
            prefixed("magicbay", command.as_ref().map(MagicbayCommand::feature))
        }
        Command::Update { command } => {
            prefixed("update", command.as_ref().map(UpdateCommand::feature))
        }
        Command::Scan { command } => prefixed("scan", command.as_ref().map(ScanCommand::feature)),
        Command::Wifi { command } => prefixed("wifi", command.as_ref().map(WifiCommand::feature)),
        Command::Bluetooth { command } => {
            prefixed("bluetooth", command.as_ref().map(BluetoothCommand::feature))
        }
        Command::Radio { command } => prefixed("radio", command.as_ref().map(RadioCommand::feature)),
        Command::Sensor { command } => {
            prefixed("sensor", command.as_ref().map(SensorCommand::feature))
        }
        Command::Security { command } => {
            prefixed("security", command.as_ref().map(SecurityCommand::feature))
        }
        Command::Fix { command } => prefixed("fix", command.as_ref().map(FixCommand::feature)),
        Command::Scene { command } => {
            prefixed("scene", command.as_ref().map(SceneCommand::feature))
        }
        Command::Snapshot { command } => {
            prefixed("snapshot", command.as_ref().map(SnapshotCommand::feature))
        }
        Command::Osd { command } => prefixed("osd", command.as_ref().map(OsdCommand::feature)),
        Command::Hotkey { command } => {
            prefixed("hotkey", command.as_ref().map(HotkeyCommand::feature))
        }
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

impl CpuCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
        }
    }
}

impl StorageCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
        }
    }
}

impl BatteryCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Health => "health",
            Self::Adapter => "adapter",
            Self::ChargeMode { .. } => "charge-mode",
            Self::Thresholds { .. } => "thresholds",
            Self::ExtremeLife { .. } => "extreme-life",
            Self::NightCharge { .. } => "night-charge",
            Self::TemporaryMode => "temporary-mode",
            Self::Watch { .. } => "watch",
        }
    }
}

impl UsbCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::AlwaysOn { .. } => "always-on",
            Self::ChargeOnBattery { .. } => "charge-on-battery",
            Self::Ports => "ports",
        }
    }
}

impl PowerCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Scheme { .. } => "scheme",
            Self::SaverOnce => "saver-once",
            Self::Ac => "ac",
            Self::Draw => "draw",
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
            Self::Dbdc { .. } => "dbdc",
            Self::Policy => "policy",
            Self::Gpu { .. } => "gpu",
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
            Self::Watch { .. } => "watch",
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
            Self::Info => "info",
            Self::Brightness { .. } => "brightness",
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

impl DisplayCommand {
    const fn feature(&self) -> &'static str {
        match self {
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
            Self::CameraAssist { .. } => "camera-assist",
            Self::ApproachAutoAdjust { .. } => "approach-auto-adjust",
            Self::LeaveAutoAdjust { .. } => "leave-auto-adjust",
            Self::AttentionTracking { .. } => "attention-tracking",
            Self::KbdLightAuto { .. } => "kbd-light-auto",
            Self::Reset => "reset",
        }
    }
}

impl AudioCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Dolby { .. } => "dolby",
            Self::NoiseCancel { .. } => "noise-cancel",
            Self::Endpoints { .. } => "endpoints",
            Self::Volume { .. } => "volume",
            Self::Mute { .. } => "mute",
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
            Self::Defaults { .. } => "defaults",
            Self::Password { .. } => "password",
        }
    }
}

impl FirmwareCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
        }
    }
}

impl UpdateCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Check { .. } => "check",
            Self::LocalDriver { command } => LocalDriverCommand::feature(command),
            Self::Download { .. } => "download",
            Self::Install { .. } => "install",
            Self::History => "history",
            Self::LocalInstall { .. } => "local-install",
            Self::Ignore { .. } => "ignore",
            Self::Rollback { .. } => "rollback",
            Self::Schedule { .. } => "schedule",
        }
    }
}

impl FixCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Run { .. } => "run",
            Self::Track => "track",
        }
    }
}

impl SceneCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Test { .. } => "test",
            Self::Track => "track",
        }
    }
}

impl LocalDriverCommand {
    const fn feature(command: &Self) -> &'static str {
        match command {
            Self::Apply { .. } => "local-driver.apply",
            Self::Rollback { .. } => "local-driver.rollback",
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

impl WifiCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Security => "security",
            Self::History => "history",
            Self::ClearHistory => "clear_history",
        }
    }
}

impl BluetoothCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
        }
    }
}

impl RadioCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Off { .. } => "off",
            Self::On { .. } => "on",
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
            Self::Show { .. } => "show",
        }
    }
}

impl HotkeyCommand {
    const fn feature(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Status => "status",
            Self::Register { .. } => "register",
            Self::Trigger { .. } => "trigger",
            Self::Unregister { .. } => "unregister",
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
