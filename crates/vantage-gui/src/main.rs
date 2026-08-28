use lctrl_core::{LctrlError, Platform};
use vantage_gui::{DashboardSnapshot, run};

#[cfg(target_os = "linux")]
fn main() {
    let hal = lctrl_hal_linux::LinuxHal::new();
    launch(snapshot_or_unavailable(&hal, Platform::Linux));
}

#[cfg(windows)]
fn main() {
    let hal =
        lctrl_hal_win::NativeWindowsHal::new(lctrl_hal_win::NativeWmi, lctrl_hal_win::NativeIoctl);
    launch(snapshot_or_unavailable(&hal, Platform::Windows));
}

#[cfg(not(any(target_os = "linux", windows)))]
fn main() {
    launch(DashboardSnapshot::unavailable(
        Platform::Linux,
        "No supported platform HAL is attached; controls remain unavailable",
    ));
}

fn snapshot_or_unavailable(hal: &dyn lctrl_hal::Hal, platform: Platform) -> DashboardSnapshot {
    match DashboardSnapshot::from_hal(hal) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            DashboardSnapshot::unavailable(platform, format!("Hardware probe failed: {error}"))
        }
    }
}

fn launch(snapshot: DashboardSnapshot) {
    if let Err(error) = run(snapshot) {
        exit_error(error);
    }
}

fn exit_error(error: LctrlError) -> ! {
    eprintln!("{error}");
    std::process::exit(i32::from(error.exit_code()));
}
