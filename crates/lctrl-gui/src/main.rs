use lctrl_core::Platform;
use lctrl_gui::{DashboardSnapshot, run};

#[cfg(target_os = "linux")]
fn main() {
    let hal = lctrl_hal_linux::LinuxHal::new();
    run(snapshot_or_unavailable(&hal, Platform::Linux));
}

#[cfg(windows)]
fn main() {
    let hal =
        lctrl_hal_win::NativeWindowsHal::new(lctrl_hal_win::NativeWmi, lctrl_hal_win::NativeIoctl);
    run(snapshot_or_unavailable(&hal, Platform::Windows));
}

#[cfg(not(any(target_os = "linux", windows)))]
fn main() {
    run(DashboardSnapshot::unavailable(
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
