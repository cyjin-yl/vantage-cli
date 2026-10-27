use lctrl_core::{
    ApplyMode, DriverRollbackReport, DriverUpdateReport, LocalDriverRollbackRequest,
    LocalDriverUpdateRequest, Result,
};

/// Explicit user-triggered local driver updates. This contract has no catalog,
/// download, firmware, automatic-install, or reboot operation.
pub trait LocalUpdateControl: Send + Sync {
    fn apply_local_driver_update(
        &self,
        request: &LocalDriverUpdateRequest,
        mode: ApplyMode,
    ) -> Result<DriverUpdateReport>;

    fn rollback_local_driver(
        &self,
        request: &LocalDriverRollbackRequest,
        mode: ApplyMode,
    ) -> Result<DriverRollbackReport>;
}
