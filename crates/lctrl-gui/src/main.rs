use lctrl_core::Platform;
use lctrl_gui::{DashboardSnapshot, run};

fn main() {
    let platform = if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
    run(DashboardSnapshot::unavailable(
        platform,
        "No platform HAL attached; controls remain unavailable",
    ));
}
