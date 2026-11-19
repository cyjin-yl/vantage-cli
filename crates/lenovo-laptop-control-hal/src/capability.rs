pub trait Hal: Send + Sync {
    fn platform(&self) -> lenovo_laptop_control_core::Platform;
    fn hardware_info(
        &self,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::HardwareInfo>;
    fn capabilities(
        &self,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::CapabilitySet>;

    /// Read the machine's identity modules (product, baseboard, BIOS).
    ///
    /// The default is an all-absent identity for platforms that have no
    /// implemented identity source; callers report it as absent rather than
    /// inventing values.
    fn system_identity(
        &self,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::SystemIdentity> {
        Ok(lenovo_laptop_control_core::SystemIdentity::default())
    }

    /// Read processor identity, firmware-declared rated clocks and the derived
    /// current clock (read-only).
    ///
    /// The default reports the channel unsupported: a platform without an
    /// implemented clock source must not answer with a rated frequency dressed
    /// up as a live one (`docs/evidence/re-cpu-20261105.md` §4, §5).
    fn cpu_status(
        &self,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::CpuStatus> {
        Err(
            lenovo_laptop_control_core::LenovoLaptopControlError::Unsupported {
                feature: "cpu.status: no processor clock source".into(),
            },
        )
    }
}
