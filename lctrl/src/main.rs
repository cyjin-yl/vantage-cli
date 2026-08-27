use clap::Parser;
use lctrl_cli::{Cli, CommandResult, execute_with_services, render_error, render_success};

fn finish(result: CommandResult, json: bool) {
    match result {
        Ok(output) => println!("{}", render_success(&output, json)),
        Err(error) => {
            eprintln!("{}", render_error(&error, json));
            std::process::exit(i32::from(error.exit_code()));
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let hal = lctrl_hal_linux::LinuxHal::new();
    let services = lctrl_cli::CommandServices::new(&hal).with_battery(&hal);
    finish(execute_with_services(cli, services), json);
}

#[cfg(not(target_os = "linux"))]
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

    let cli = Cli::parse();
    let json = cli.json;
    finish(lctrl_cli::execute(cli, &UnavailableHal), json);
}
