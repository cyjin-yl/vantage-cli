use lctrl_core::Result;
use lctrl_hal::ControlConflictDetection;

use crate::{WmiTransport, WmiValue};

const ROOT_CIMV2: &str = r"ROOT\CIMV2";

/// Read-only process detector for vendor applications that race hardware writes.
pub struct WindowsControlConflictDetector<W> {
    transport: W,
}

impl<W> WindowsControlConflictDetector<W> {
    pub const fn new(transport: W) -> Self {
        Self { transport }
    }
}

impl<W> ControlConflictDetection for WindowsControlConflictDetector<W>
where
    W: WmiTransport,
{
    fn active_vendor_controllers(&self) -> Result<Vec<String>> {
        let objects = self
            .transport
            .query(ROOT_CIMV2, "SELECT Name FROM Win32_Process")?;
        let mut controllers = objects
            .iter()
            .filter_map(|object| match object.get("Name") {
                Some(WmiValue::String(name)) if vendor_controller_name(name) => Some(name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        controllers.sort();
        controllers.dedup();
        Ok(controllers)
    }
}

fn vendor_controller_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "lenovovantage",
        "vantageservice",
        "imcontroller",
        "legionzone",
        "pcmanager",
        "magicenter",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}
