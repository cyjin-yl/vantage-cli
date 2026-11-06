//! Read-only firmware inventory surface.

use lctrl_core::{FirmwareStatus, Result};

/// Read firmware identity and the platform firmware-resource table.
///
/// This trait has exactly one method and it is a read: no implementation may
/// stage, flash or clear a firmware capsule. A platform that cannot answer
/// returns `Unsupported` rather than an empty, plausible-looking status.
pub trait FirmwareControl: Send + Sync {
    fn firmware_status(&self) -> Result<FirmwareStatus>;
}
