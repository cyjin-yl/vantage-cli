use lctrl_core::Result;

/// Detects vendor applications that can race lctrl on the same hardware channels.
pub trait ControlConflictDetection: Send + Sync {
    /// Return the process names of active vendor control applications.
    fn active_vendor_controllers(&self) -> Result<Vec<String>>;
}
