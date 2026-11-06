use lctrl_core::{
    ApplyMode, DriverRollbackReport, DriverUpdateReport, LocalDriverRollbackRequest,
    LocalDriverUpdateRequest, LocalPackageInstallReport, LocalPackageInstallRequest, Result,
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

/// Explicit user-triggered installation of an update package the Lenovo UDC
/// service already downloaded, using the package's own signed `install.ps1`
/// contract. Deliberately separate from [`LocalUpdateControl`]: that contract
/// installs one vendored `.inf` through SetupAPI, while this one hands a whole
/// vendor package directory to its own installer. Neither downloads, schedules,
/// flips firmware, nor reboots.
pub trait LocalPackageInstallControl: Send + Sync {
    fn apply_local_package(
        &self,
        request: &LocalPackageInstallRequest,
        mode: ApplyMode,
    ) -> Result<LocalPackageInstallReport>;
}
