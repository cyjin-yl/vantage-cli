use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;

use clap::error::ErrorKind;
use vantage_cli::{
    Cli, Command as CliCommand, CommandOutput, CommandResult, DaemonCommand, execute_with_services,
    render_error, render_success,
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
        DaemonCommand::Status => {
            daemon_request(vantage_daemon::DaemonRequest::Status, "daemon status")
        }
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
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| lctrl_core::LctrlError::ChannelUnavailable {
            channel: "HOME or XDG_CONFIG_HOME".into(),
        })?;
    let unit_path = config_root.join("systemd/user/vantaged.service");
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
        run_checked("systemctl", &["--user", "enable", "vantaged.service"])?;
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
                "vantage-daemon",
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
            "task": "vantage-daemon",
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

#[cfg(target_os = "linux")]
fn main() {
    let cli = parse_cli();
    let json = cli.json;
    if handle_daemon_command(&cli, json) {
        return;
    }
    let hal = lctrl_hal_linux::LinuxHal::new();
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
        .with_touchpad(&hal)
        .with_tuning(&hal)
        .with_temperature(&hal)
        .with_update(&hal);
    finish(execute_with_services(cli, services), json);
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
    let battery = lctrl_hal_win::WindowsBatteryP0::new(
        lctrl_hal_win::NativeIoctl,
        lctrl_hal_win::UnverifiedChargeModeReader,
    );
    let conflicts = lctrl_hal_win::WindowsControlConflictDetector::new(lctrl_hal_win::NativeWmi);
    let bios = lctrl_hal_win::WindowsBiosController::new(lctrl_hal_win::NativeWmi);
    let performance =
        lctrl_hal_win::WindowsPerformanceP0::new(lctrl_hal_win::NativePerformanceRegistry);
    let system_inventory = lctrl_hal_win::WindowsSystemInventory::new(lctrl_hal_win::NativeWmi);
    let magicbay = lctrl_hal_win::NativeMagicBay;
    let peripherals = lctrl_hal_win::WindowsPeripheralController::new(lctrl_hal_win::NativeWmi);
    let power = lctrl_hal_win::WindowsPowerP0::new(lctrl_hal_win::NativePowerApi);
    let services = vantage_cli::CommandServices::new(&hal)
        .with_battery(&battery)
        .with_conflict_detection(&conflicts)
        .with_bios(&bios)
        .with_diagnostics(&system_inventory)
        .with_keyboard(&peripherals)
        .with_magicbay(&magicbay)
        .with_performance(&performance)
        .with_power(&power)
        .with_panel(&peripherals)
        .with_update(&system_inventory);
    finish(execute_with_services(cli, services), json);
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
    finish(vantage_cli::execute(cli, &UnavailableHal), json);
}
