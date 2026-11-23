use lenovo_laptop_control_core::{
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

/// The machine's present device hardware IDs.
///
/// Catalog packages declare the hardware they bind to (`PCI\VEN_8086&DEV_…`,
/// `USB\VID_…&PID_…`); comparing those against the IDs actually present on
/// this machine is what separates "Lenovo publishes this for the model" from
/// "this package matches a device this machine has". The trait is read-only
/// and never installs anything.
pub trait HardwareInventoryControl: Send + Sync {
    /// Every present device's hardware IDs, verbatim and uppercase as the
    /// platform reports them.
    fn present_hardware_ids(&self) -> Result<Vec<String>>;
}
