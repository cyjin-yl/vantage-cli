use lctrl_core::{Result, StorageStatus};

/// Read-only storage identity and health.
///
/// This contract has no write verb: a drive's self-test trigger, sanitize /
/// secure-erase operation and firmware-slot writes are outside the recovered
/// channel, so no method exists that could reach them
/// (docs/evidence/re-storage-20261105.md).
pub trait StorageStatusControl {
    /// Enumerate the physical drives the platform exposes and report the
    /// identity and health fields each one actually answered.
    ///
    /// A refused device request is recorded as a per-device issue; a
    /// transport-level failure is returned as an error instead of an empty
    /// device list, which would read as "no drives installed".
    fn storage_status(&self) -> Result<StorageStatus>;
}
