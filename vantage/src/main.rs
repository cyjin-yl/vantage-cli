use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;

use clap::error::ErrorKind;
use vantage_cli::{
    Cli, Command as CliCommand, CommandOutput, CommandResult, CommandServices, DaemonCommand,
    StreamSpec, execute_with_services, render_error, render_success, sample_stream, stream_spec,
};

fn finish(result: CommandResult, json: bool) {
    match result {
        Ok(output) => println!("{}", render_success(&output, json)),
        Err(error) => {
            eprintln!("{}", render_error(&error, json));
            std::process::exit(i32::from(error.exit_code()));
        }
    }
}

fn write_output(output: &CommandOutput, json: bool) -> lctrl_core::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{}", render_success(output, json))?;
    stdout.flush()?;
    Ok(())
}

fn finish_io(result: lctrl_core::Result<()>, json: bool) {
    if let Err(error) = result {
        // Preserve the original failure's exit code even if stderr is closed.
        let _ = writeln!(std::io::stderr().lock(), "{}", render_error(&error, json));
        std::process::exit(i32::from(error.exit_code()));
    }
}

fn run(cli: Cli, services: CommandServices<'_>) {
    let json = cli.json;
    match stream_spec(&cli) {
        Ok(Some(spec)) => finish_io(run_stream(spec, &services, json), json),
        Ok(None) => finish(execute_with_services(cli, services), json),
        Err(error) => finish(Err(error), json),
    }
}

fn run_stream(
    spec: StreamSpec,
    services: &CommandServices<'_>,
    json: bool,
) -> lctrl_core::Result<()> {
    loop {
        write_output(&sample_stream(spec.kind, services)?, json)?;

        // Bound each OS sleep so even very large valid intervals do not overflow
        // platform-specific sleep representations.
        const MAX_SLEEP: Duration = Duration::from_secs(24 * 60 * 60);
        let mut remaining = spec.interval;
        while remaining > MAX_SLEEP {
            thread::sleep(MAX_SLEEP);
            remaining -= MAX_SLEEP;
        }
        thread::sleep(remaining);
    }
}

fn parse_cli() -> Cli {
    let args: Vec<_> = std::env::args_os().collect();
    let json = args.iter().any(|arg| arg == "--json");
    match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            std::process::exit(0);
        }
        Err(error) if json => {
            let error = lctrl_core::LctrlError::InvalidArgument {
                detail: error.to_string(),
            };
            eprintln!("{}", render_error(&error, true));
            std::process::exit(i32::from(error.exit_code()));
        }
        Err(error) => error.exit(),
    }
}

fn handle_daemon_command(cli: &Cli, json: bool) -> bool {
    let CliCommand::Daemon {
        command: Some(command),
    } = &cli.command
    else {
        return false;
    };
    finish(execute_daemon_command(command, cli.dry_run), json);
    true
}

fn execute_daemon_command(command: &DaemonCommand, dry_run: bool) -> CommandResult {
    match command {
        DaemonCommand::Status => daemon_status(),
        DaemonCommand::Stop => {
            daemon_request(vantage_daemon::DaemonRequest::Stop, "daemon stopped")
        }
        DaemonCommand::Start => start_daemon(dry_run),
        DaemonCommand::Install => install_daemon(dry_run),
    }
}

fn daemon_request(request: vantage_daemon::DaemonRequest, human: &str) -> CommandResult {
    let response = vantage_daemon::request(&request)?;
    if !response.ok {
        return Err(lctrl_core::LctrlError::ChannelUnavailable {
            channel: response
                .error
                .unwrap_or_else(|| "daemon rejected request".into()),
        });
    }
    Ok(CommandOutput {
        human: format!("{human}\n"),
        json: response.data.unwrap_or(serde_json::Value::Null),
    })
}

/// `daemon status` reports only what can be observed: one IPC probe plus the
/// on-disk autostart record. A daemon that is not running is a normal answer
/// (exit 0 with `running: false`); an endpoint that exists but cannot complete
/// the exchange is a fault and stays an error.
///
/// The daemon keeps no log, so `observed_error` is strictly what this
/// invocation saw; a failure from an earlier daemon lifetime is not
/// recoverable here.
fn daemon_status() -> CommandResult {
    use vantage_daemon::StatusProbe;

    let endpoint = vantage_daemon::endpoint_text();
    let autostart = daemon_autostart();
    let mut running = false;
    let mut daemon = None;
    let mut uptime_ms = None;
    let mut event_count = None;
    let mut last_event = None;
    let mut protocol_matches = None;
    let mut observed_error = None;
    let mut human = String::new();

    match vantage_daemon::probe() {
        StatusProbe::Running(status) => {
            running = true;
            let uptime = unix_time_ms().saturating_sub(status.started_at_unix_ms);
            let version = if status.version.is_empty() {
                "unreported"
            } else {
                status.version.as_str()
            };
            human.push_str(&format!(
                "daemon: running\n  endpoint: {endpoint}\n  version: {version} (protocol {})\n  pid: {}\n  uptime: {}\n  subscribers: {}\n  buffered events: {}/{}\n",
                status.protocol_version,
                status.pid,
                format_uptime(uptime),
                status.subscribers,
                status.last_events.len(),
                vantage_daemon::MAX_EVENTS,
            ));
            if let Some(event) = status.last_events.last() {
                human.push_str(&format!(
                    "  last event: {} at {} (unix ms)\n",
                    event.kind,
                    event.ts_unix_ms
                ));
                last_event = Some(serde_json::to_value(event).map_err(serialization_error)?);
            }
            if status.protocol_version != vantage_daemon::PROTOCOL_VERSION {
                human.push_str(&format!(
                    "  warning: the daemon speaks protocol {}, this CLI speaks {}\n",
                    status.protocol_version,
                    vantage_daemon::PROTOCOL_VERSION
                ));
            }
            uptime_ms = Some(uptime);
            event_count = Some(status.last_events.len());
            protocol_matches = Some(status.protocol_version == vantage_daemon::PROTOCOL_VERSION);
            daemon = Some(serde_json::to_value(&status).map_err(serialization_error)?);
        }
        StatusProbe::Absent { detail } => {
            human.push_str(&format!(
                "daemon: not running\n  endpoint: {endpoint}\n  last error: {detail} (observed by this status call; the daemon keeps no log)\n"
            ));
            observed_error = Some(detail);
        }
        StatusProbe::Fault { detail } => {
            return Err(lctrl_core::LctrlError::ChannelUnavailable {
                channel: format!("daemon status: {detail}"),
            });
        }
    }
    human.push_str(&autostart.human());

    Ok(CommandOutput {
        human,
        json: serde_json::json!({
            "running": running,
            "endpoint": endpoint,
            "cli_version": env!("CARGO_PKG_VERSION"),
            "cli_protocol_version": vantage_daemon::PROTOCOL_VERSION,
            "protocol_matches": protocol_matches,
            "daemon": daemon,
            "uptime_ms": uptime_ms,
            "event_count": event_count,
            "last_event": last_event,
            "autostart": autostart.json(),
            "observed_error": observed_error,
            "observed_error_scope": "this status invocation only; the daemon keeps no log",
        }),
    })
}

fn serialization_error(error: serde_json::Error) -> lctrl_core::LctrlError {
    lctrl_core::LctrlError::Io(std::io::Error::other(error))
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn format_uptime(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// The installed-autostart record, read from disk. Nothing here starts, stops
/// or modifies a service.
struct DaemonAutostart {
    kind: &'static str,
    path: Option<PathBuf>,
    installed: bool,
    /// `None` when the platform record does not expose an inspected state.
    enabled: Option<bool>,
    note: &'static str,
}

impl DaemonAutostart {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind,
            "path": self.path.as_ref().map(|path| path.display().to_string()),
            "installed": self.installed,
            "enabled": self.enabled,
            "note": self.note,
        })
    }

    fn human(&self) -> String {
        let Some(path) = &self.path else {
            return format!("  autostart: {}\n", self.note);
        };
        let state = match self.enabled {
            Some(true) => ", enabled",
            Some(false) => ", not enabled",
            None => "",
        };
        format!(
            "  autostart: {} at {}{state} ({})\n",
            self.kind,
            path.display(),
            self.note
        )
    }
}

#[cfg(target_os = "linux")]
fn daemon_autostart() -> DaemonAutostart {
    let unit = match daemon_unit_path() {
        Ok(path) => path,
        Err(_) => {
            return DaemonAutostart {
                kind: "systemd-user-unit",
                path: None,
                installed: false,
                enabled: None,
                note: "HOME and XDG_CONFIG_HOME are unset, so no user unit path resolves",
            };
        }
    };
    let installed = unit.is_file();
    // `systemctl --user enable` with `WantedBy=default.target` writes this
    // symlink, so its presence is the enable evidence; a status query does not
    // call the service manager.
    let mut enabled = None;
    if installed {
        enabled = Some(daemon_unit_enable_link().is_ok_and(|link| link.is_symlink()));
    }
    DaemonAutostart {
        kind: "systemd-user-unit",
        path: Some(unit),
        installed,
        enabled,
        note: match enabled {
            Some(true) => "enabled for default.target",
            Some(false) => "not enabled for default.target",
            None => "the unit file is absent; run `vantage daemon install`",
        },
    }
}

#[cfg(windows)]
fn daemon_autostart() -> DaemonAutostart {
    let task = daemon_task_path();
    let installed = task.is_file();
    let note = if installed {
        "the task record exists; its enabled state is not inspected"
    } else {
        "the task record is absent; run `vantage daemon install`"
    };
    DaemonAutostart {
        kind: "logon-task",
        path: Some(task),
        installed,
        enabled: None,
        note,
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn daemon_autostart() -> DaemonAutostart {
    DaemonAutostart {
        kind: "unsupported",
        path: None,
        installed: false,
        enabled: None,
        note: "autostart is not implemented on this platform",
    }
}

/// The user config root that both `daemon install` and `daemon status` resolve.
#[cfg(target_os = "linux")]
fn daemon_config_root() -> lctrl_core::Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| lctrl_core::LctrlError::ChannelUnavailable {
            channel: "HOME or XDG_CONFIG_HOME".into(),
        })
}

/// Unit file name shared by `daemon install` and `daemon status`.
#[cfg(target_os = "linux")]
const DAEMON_UNIT_NAME: &str = "vantaged.service";

/// The systemd user-unit directory under the shared config root.
#[cfg(target_os = "linux")]
fn daemon_unit_dir() -> lctrl_core::Result<PathBuf> {
    Ok(daemon_config_root()?.join("systemd/user"))
}

/// The user unit `daemon install` writes and `daemon status` reports.
#[cfg(target_os = "linux")]
fn daemon_unit_path() -> lctrl_core::Result<PathBuf> {
    Ok(daemon_unit_dir()?.join(DAEMON_UNIT_NAME))
}

/// The `systemctl --user enable` symlink the unit's `WantedBy=` creates.
#[cfg(target_os = "linux")]
fn daemon_unit_enable_link() -> lctrl_core::Result<PathBuf> {
    Ok(daemon_unit_dir()?.join("default.target.wants").join(DAEMON_UNIT_NAME))
}

/// Task name registered by `schtasks` and reported by `daemon status`.
#[cfg(windows)]
const DAEMON_TASK_NAME: &str = "vantage-daemon";

/// The task record `daemon install` registers and `daemon status` reports.
#[cfg(windows)]
fn daemon_task_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("Tasks")
        .join(DAEMON_TASK_NAME)
}

fn daemon_binary() -> lctrl_core::Result<PathBuf> {
    if let Some(path) = std::env::var_os("VANTAGE_DAEMON_BINARY") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe().map_err(lctrl_core::LctrlError::Io)?;
    let name = if cfg!(windows) {
        "vantaged.exe"
    } else {
        "vantaged"
    };
    let path = executable
        .parent()
        .map(|parent| parent.join(name))
        .ok_or_else(|| lctrl_core::LctrlError::ChannelUnavailable {
            channel: "current executable has no parent directory".into(),
        })?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(lctrl_core::LctrlError::ChannelUnavailable {
            channel: format!("daemon binary not found at {}", path.display()),
        })
    }
}

fn start_daemon(dry_run: bool) -> CommandResult {
    if let Ok(response) = vantage_daemon::request(&vantage_daemon::DaemonRequest::Status)
        && response.ok
    {
        return Ok(CommandOutput {
            human: "daemon is already running\n".into(),
            json: response.data.unwrap_or(serde_json::Value::Null),
        });
    }
    let binary = daemon_binary()?;
    if dry_run {
        return Ok(CommandOutput {
            human: format!("would start {}\n", binary.display()),
            json: serde_json::json!({"mode": "dry_run", "binary": binary}),
        });
    }
    let mut process = ProcessCommand::new(&binary);
    process
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached(&mut process);
    process.spawn().map_err(lctrl_core::LctrlError::Io)?;

    let mut last_error = None;
    for _ in 0..40 {
        match vantage_daemon::request(&vantage_daemon::DaemonRequest::Status) {
            Ok(response) if response.ok => {
                return Ok(CommandOutput {
                    human: "daemon started\n".into(),
                    json: response.data.unwrap_or(serde_json::Value::Null),
                });
            }
            Ok(response) => last_error = response.error,
            Err(error) => last_error = Some(error.to_string()),
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(lctrl_core::LctrlError::ChannelUnavailable {
        channel: format!(
            "daemon did not become ready: {}",
            last_error.unwrap_or_else(|| "no status response".into())
        ),
    })
}

#[cfg(unix)]
fn configure_detached(_process: &mut ProcessCommand) {}

#[cfg(windows)]
fn configure_detached(process: &mut ProcessCommand) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    process.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached(_process: &mut ProcessCommand) {}

fn install_daemon(dry_run: bool) -> CommandResult {
    let binary = daemon_binary()?;
    install_daemon_platform(&binary, dry_run)
}

#[cfg(target_os = "linux")]
fn install_daemon_platform(binary: &Path, dry_run: bool) -> CommandResult {
    let unit_path = daemon_unit_path()?;
    let binary_text = binary.to_string_lossy();
    if binary_text.contains(['\n', '\r', '"']) {
        return Err(lctrl_core::LctrlError::InvalidArgument {
            detail: "daemon binary path contains unsupported systemd quoting characters".into(),
        });
    }
    let unit = format!(
        "[Unit]\nDescription=vantage hardware event daemon\n\n[Service]\nType=simple\nExecStart=\"{binary_text}\"\nRestart=on-failure\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n"
    );
    if !dry_run {
        if let Some(parent) = unit_path.parent() {
            std::fs::create_dir_all(parent).map_err(lctrl_core::LctrlError::Io)?;
        }
        write_atomic(&unit_path, unit.as_bytes())?;
        run_checked("systemctl", &["--user", "daemon-reload"])?;
        run_checked("systemctl", &["--user", "enable", DAEMON_UNIT_NAME])?;
    }
    Ok(CommandOutput {
        human: format!(
            "{} daemon user service at {}\n",
            if dry_run {
                "would install"
            } else {
                "installed"
            },
            unit_path.display()
        ),
        json: serde_json::json!({
            "mode": if dry_run { "dry_run" } else { "commit" },
            "service": unit_path,
            "binary": binary,
        }),
    })
}

#[cfg(windows)]
fn install_daemon_platform(binary: &Path, dry_run: bool) -> CommandResult {
    if !dry_run {
        let task = format!("\"{}\"", binary.display());
        let output = ProcessCommand::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                DAEMON_TASK_NAME,
                "/SC",
                "ONLOGON",
                "/TR",
                &task,
                "/RL",
                "HIGHEST",
                "/F",
            ])
            .output()
            .map_err(lctrl_core::LctrlError::Io)?;
        if !output.status.success() {
            return Err(lctrl_core::LctrlError::ChannelUnavailable {
                channel: format!(
                    "schtasks.exe exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
    }
    Ok(CommandOutput {
        human: format!(
            "{} daemon logon task\n",
            if dry_run {
                "would install"
            } else {
                "installed"
            }
        ),
        json: serde_json::json!({
            "mode": if dry_run { "dry_run" } else { "commit" },
            "task": DAEMON_TASK_NAME,
            "binary": binary,
        }),
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
fn install_daemon_platform(_binary: &Path, _dry_run: bool) -> CommandResult {
    Err(lctrl_core::LctrlError::Unsupported {
        feature: "daemon.install".into(),
    })
}

#[cfg(target_os = "linux")]
fn write_atomic(path: &Path, contents: &[u8]) -> lctrl_core::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, contents).map_err(lctrl_core::LctrlError::Io)?;
    std::fs::rename(temporary, path).map_err(lctrl_core::LctrlError::Io)
}

#[cfg(target_os = "linux")]
fn run_checked(program: &str, args: &[&str]) -> lctrl_core::Result<()> {
    let output = ProcessCommand::new(program)
        .args(args)
        .output()
        .map_err(lctrl_core::LctrlError::Io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(lctrl_core::LctrlError::ChannelUnavailable {
            channel: format!(
                "{program} exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        })
    }
}

/// Channel diagnostics are opt-in: `VANTAGE_DEBUG=1` prints why an optional
/// channel stayed unattached, so normal runs stay quiet.
#[cfg(windows)]
fn debug_note(message: &str) {
    if std::env::var_os("VANTAGE_DEBUG").is_some() {
        eprintln!("{message}");
    }
}

#[cfg(target_os = "linux")]
fn main() {
    let cli = parse_cli();
    let json = cli.json;
    if handle_daemon_command(&cli, json) {
        return;
    }
    let hal = lctrl_hal_linux::LinuxHal::new();
    let process = lctrl_hal_linux::LinuxProcessController::new();
    // No Linux radio plane is bound yet, so the service is attached and answers
    // `Unsupported` instead of leaving `radio` commands looking unimplemented.
    let radio = lctrl_hal_linux::LinuxRadio::new();
    let services = vantage_cli::CommandServices::new(&hal)
        .with_battery(&hal)
        .with_conflict_detection(&hal)
        .with_diagnostics(&hal)
        .with_fan(&hal)
        .with_keyboard(&hal)
        .with_magicbay(&hal)
        .with_privacy(&hal)
        .with_performance(&hal)
        .with_power(&hal)
        .with_process(&process)
        .with_radio(&radio)
        .with_touchpad(&hal)
        .with_tuning(&hal)
        .with_temperature(&hal)
        .with_update(&hal);
    run(cli, services);
}

#[cfg(windows)]
fn main() {
    let cli = parse_cli();
    let json = cli.json;
    if handle_daemon_command(&cli, json) {
        return;
    }
    let hal =
        lctrl_hal_win::NativeWindowsHal::new(lctrl_hal_win::NativeWmi, lctrl_hal_win::NativeIoctl);
    // The charge mode is read back through the GBMD status word, so a
    // write is verified against the firmware instead of being assumed.
    let vendor_battery = lctrl_hal_win::WindowsBatteryP0::new(
        lctrl_hal_win::NativeIoctl,
        lctrl_hal_win::GbmdChargeModeReader::new(lctrl_hal_win::NativeIoctl),
    );
    let os_battery = lctrl_hal_win::WindowsOsBattery::new(
        lctrl_hal_win::NativeWmi,
        lctrl_hal_win::NativeSystemPowerStatus,
    );
    let battery = lctrl_hal_win::WindowsBatterySources::new(
        vendor_battery,
        os_battery,
        lctrl_hal_win::NativeBatteryElectrical,
    );
    let conflicts = lctrl_hal_win::WindowsControlConflictDetector::new(lctrl_hal_win::NativeWmi);
    let bios = lctrl_hal_win::WindowsBiosController::new(lctrl_hal_win::NativeWmi);
    // Firmware inventory is a read of the platform's own ESRT projection and
    // firmware-update record; nothing here can flash a capsule.
    let firmware = lctrl_hal_win::WindowsFirmware::new(lctrl_hal_win::NativeWmi);
    let performance =
        lctrl_hal_win::WindowsPerformanceP0::new(lctrl_hal_win::NativePerformanceRegistry);
    let system_inventory = lctrl_hal_win::WindowsSystemInventory::new(lctrl_hal_win::NativeWmi);
    let magicbay = lctrl_hal_win::NativeMagicBay;
    let mobile_broadband = lctrl_hal_win::WindowsMobileBroadband::new(lctrl_hal_win::NativeWmi);
    // The keyboard backlight follows the ACPI `KBLC` method the Lenovo virtual
    // power controller re-exposes on `\\.\EnergyDrv`; the WMI lighting plane
    // stays attached for models that publish it instead.
    let keyboard = lctrl_hal_win::WindowsKeyboardSources::new(
        lctrl_hal_win::WindowsKeyboardBacklight::new(lctrl_hal_win::NativeIoctl),
        lctrl_hal_win::WindowsPeripheralController::new(lctrl_hal_win::NativeWmi),
    );
    let panel = lctrl_hal_win::WindowsPanelController::new(
        lctrl_hal_win::NativeDisplayApi,
        lctrl_hal_win::NativeWmi,
    );
    let panel_brightness = lctrl_hal_win::WindowsPanelBrightness::new(lctrl_hal_win::NativeWmi);
    // Panel identity joins the OS monitor-connection classification with the
    // per-instance EDID registry value; both are read-only.
    let panel_identity = lctrl_hal_win::WindowsPanelIdentity::new(
        lctrl_hal_win::NativeWmi,
        lctrl_hal_win::NativePanelRegistry,
    );
    // Display state is read-only: the live session's CCD/DPI/advanced-colour
    // state plus the display driver's machine-wide per-monitor colour store.
    let display_status = lctrl_hal_win::WindowsDisplayStatus::new(
        lctrl_hal_win::NativeDisplayApi,
        lctrl_hal_win::NativeDisplayState,
        lctrl_hal_win::NativeWmi,
        lctrl_hal_win::NativeMonitorColorStore,
    );
    let power = lctrl_hal_win::WindowsPowerP0::new(lctrl_hal_win::NativePowerApi);
    let thermal = lctrl_hal_win::WindowsThermal::new(lctrl_hal_win::NativeWmi, &battery);
    let gpu_oc = lctrl_hal_win::WindowsGpuOc::new(lctrl_hal_win::NativeWmi);
    let runtime_devices = lctrl_hal_win::NativeWindowsRuntimeDevices::default();
    let wifi_security = lctrl_hal_win::WindowsWifiSecurity::default();
    // The link-state verb binds the WLAN API directly (no scan), and the
    // Bluetooth verb enumerates the OS device cache (no inquiry).
    let wifi_status = lctrl_hal_win::WindowsWifiStatus;
    let bluetooth = lctrl_hal_win::WindowsBluetooth;
    // The radio channel holds no state: each command opens and releases its own
    // WinRT apartment, so it is attached unconditionally and reports per-call.
    let radio = lctrl_hal_win::NativeRadio::new();
    // The sensor channel is the public Sensor API: each call opens its own COM
    // apartment and enumerates from the platform, so it is attached
    // unconditionally and reports per-call. Read-only; no report interval or
    // event sink is written (docs/evidence/re-sensors-20261105.md §1).
    let sensor = lctrl_hal_win::WindowsSensors::new();
    // Secure Boot / TPM / BitLocker: the UEFI variable set, `Win32_Tpm`, and
    // `Win32_EncryptableVolume`, all read-only. Attached unconditionally; each
    // component reports its own state or its own reason for being unavailable.
    let security = lctrl_hal_win::WindowsSecurity::new(
        lctrl_hal_win::NativeWmi,
        lctrl_hal_win::NativeFirmwareEnvironment,
    );
    let local_update =
        lctrl_hal_win::WindowsLocalUpdateController::new(lctrl_hal_win::NativeLocalDriverUpdateApi);
    // A package the UDC service already downloaded is installed by its own
    // Lenovo-signed `install.ps1`; nothing here downloads, schedules, flashes
    // firmware, or reboots.
    let local_package = lctrl_hal_win::WindowsLocalPackageInstaller::new(
        lctrl_hal_win::NativeLocalPackageApi,
    );
    let machine_fix = lctrl_hal_win::WindowsMachineFix::new();
    let scene = lctrl_hal_win::NativeScenePolicy::new();
    let osd = lctrl_hal_win::NativeOsd::new();
    let hotkey = lctrl_hal_win::NativeHotkey::new();
    // The event-log channel holds no state: the query opens and releases its
    // own wevtapi handles per call, so it is attached unconditionally and
    // reports ChannelUnavailable if the log itself refuses the query.
    let event_log = lctrl_hal_win::WindowsEventLog;
    let usb_power = lctrl_hal_win::WindowsUsbPower::new(lctrl_hal_win::NativeIoctl);
    let usb_ports = lctrl_hal_win::WindowsUsbPorts::new();
    // The storage service holds no state: each call enumerates the disk device
    // interface class and queries the devices on a zero-access handle.
    let storage = lctrl_hal_win::WindowsStorage::new(lctrl_hal_win::NativeStorageProbe);
    // The Dolby client speaks the DAX contract over rpcrt4 (no Lenovo
    // payload dependency). `load()` only composes the binding; a missing
    // DAX3API surfaces when the first command runs, so the service is
    // attached unconditionally and reports ChannelUnavailable per call.
    let dolby = match lctrl_hal_win::NativeDolbyRpc::load() {
        Ok(rpc) => Some(lctrl_hal_win::WindowsDolbyAudio::new(rpc)),
        Err(error) => {
            debug_note(&format!("dolby audio channel unavailable: {error}"));
            None
        }
    };
    let noise_cancel = match lctrl_hal_win::NativeElevocRpc::load() {
        Ok(rpc) => Some(lctrl_hal_win::WindowsNoiseCancel::new(rpc)),
        Err(error) => {
            debug_note(&format!("noise-cancel channel unavailable: {error}"));
            None
        }
    };
    // The OS endpoint surface is the MMDevice API, so it is attached
    // unconditionally: a missing vendor audio stack cannot disable it and the
    // per-call path reports ChannelUnavailable if the audio services are down.
    let audio_endpoints =
        lctrl_hal_win::WindowsAudioEndpoints::new(lctrl_hal_win::NativeMmDeviceApi);
    let services = vantage_cli::CommandServices::new(&hal)
        .with_battery(&battery)
        .with_conflict_detection(&conflicts)
        .with_bios(&bios)
        .with_firmware(&firmware)
        .with_diagnostics(&system_inventory)
        .with_dbdc(&thermal)
        .with_gpu_oc(&gpu_oc)
        .with_keyboard(&keyboard)
        .with_its_policy(&performance)
        .with_magicbay(&magicbay)
        .with_mobile_broadband(&mobile_broadband)
        .with_performance(&performance)
        .with_power(&power)
        .with_privacy(&runtime_devices)
        .with_touchpad(&runtime_devices)
        .with_tuning(&power)
        .with_usb(&usb_power)
        .with_usb_ports(&usb_ports)
        .with_processor_power(&power)
        .with_panel(&panel)
        .with_panel_brightness(&panel_brightness)
        .with_panel_identity(&panel_identity)
        .with_display_status(&display_status)
        .with_temperature(&thermal)
        .with_fan(&thermal)
        .with_update(&system_inventory)
        .with_local_update(&local_update)
        .with_local_package(&local_package)
        .with_wifi(&wifi_security)
        .with_wifi_status(&wifi_status)
        .with_bluetooth(&bluetooth)
        .with_radio(&radio)
        .with_sensor(&sensor)
        .with_security(&security)
        .with_machine_fix(&machine_fix)
        .with_scene(&scene)
        .with_osd(&osd)
        .with_hotkey(&hotkey)
        .with_storage(&storage)
        .with_event_log(&event_log);
    let services = match &noise_cancel {
        Some(noise_cancel) => services.with_noise_cancel(noise_cancel),
        None => services,
    };
    let services = match &dolby {
        Some(dolby) => services.with_audio(dolby),
        None => services,
    };
    let services = services.with_audio_endpoints(&audio_endpoints);
    // Panel image-quality features ride the graphics driver's Intel Control
    // Library (IGCL). The library ships with the Intel driver, so a missing
    // package only disables super-resolution. Eye-care is applied through the
    // desktop gamma ramp (the mechanism the vendor's own EyeCareMode uses),
    // so it is attached independently of IGCL.
    let panel_features = match lctrl_hal_win::IntelControlLib::load() {
        Ok(ctl) => Some(
            lctrl_hal_win::WindowsPanelFeatures::new(ctl).with_gamma_ramp(Box::new(
                lctrl_hal_win::NativeGammaRamp,
            )),
        ),
        Err(error) => {
            debug_note(&format!("panel features channel unavailable: {error}"));
            None
        }
    };
    let services = match &panel_features {
        Some(panel_features) => services.with_panel_features(panel_features),
        None => services,
    };
    // The HPD RPC transport is live on the target: the NDR descriptor is
    // accepted by the runtime and reads/writes through the SmartSense HPD
    // endpoint. The service still falls back to the registry projection
    // when the endpoint cannot be reached.
    let sense = lctrl_hal_win::WindowsSenseControl::new(
        lctrl_hal_win::NativeHpdRpc::new(),
        lctrl_hal_win::NativeSensePipe,
        lctrl_hal_win::NativeHpdStore,
    );
    // Process telemetry and per-process policy control use the public Win32
    // process APIs; every mutation re-verifies the process identity and is
    // read back independently before it is reported as applied.
    let process =
        lctrl_hal_win::WindowsProcessController::new(lctrl_hal_win::NativeProcessApi::default());
    let services = services.with_sense(&sense).with_process(&process);
    run(cli, services);
}

#[cfg(not(any(target_os = "linux", windows)))]
fn main() {
    use lctrl_core::{CapabilitySet, HardwareInfo, LctrlError, Platform};
    use lctrl_hal::Hal;

    #[derive(Debug, Default)]
    struct UnavailableHal;

    impl Hal for UnavailableHal {
        fn platform(&self) -> Platform {
            Platform::Windows
        }

        fn hardware_info(&self) -> lctrl_core::Result<HardwareInfo> {
            Err(LctrlError::Unsupported {
                feature: "hal.hardware-info".into(),
            })
        }

        fn capabilities(&self) -> lctrl_core::Result<CapabilitySet> {
            Err(LctrlError::Unsupported {
                feature: "hal.capabilities".into(),
            })
        }
    }

    let cli = parse_cli();
    let json = cli.json;
    if handle_daemon_command(&cli, json) {
        return;
    }
    let hal = UnavailableHal;
    run(cli, CommandServices::new(&hal));
}
