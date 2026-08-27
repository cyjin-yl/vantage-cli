use clap::Parser;
use lctrl_cli::{Cli, execute, render_error, render_success};
use lctrl_core::{CapabilitySet, HardwareInfo, LctrlError, Platform};
use lctrl_hal::Hal;

/// Placeholder HAL used until the controller integrates concrete host
/// backends.  It never claims that hardware operations succeeded.
#[derive(Debug, Default)]
struct UnavailableHal;

impl Hal for UnavailableHal {
    fn platform(&self) -> Platform {
        // `Hal` currently has no platform-independent `Unknown` variant.  The
        // value is only a placeholder: both info methods below remain explicit
        // unsupported errors until a real backend is wired in.
        Platform::Linux
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

fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let result = execute(cli, &UnavailableHal);

    match result {
        Ok(output) => println!("{}", render_success(&output, json)),
        Err(error) => {
            eprintln!("{}", render_error(&error, json));
            std::process::exit(i32::from(error.exit_code()));
        }
    }
}
