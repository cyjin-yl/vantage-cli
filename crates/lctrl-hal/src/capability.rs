pub trait Hal: Send + Sync {
    fn platform(&self) -> lctrl_core::Platform;
    fn hardware_info(&self) -> lctrl_core::Result<lctrl_core::HardwareInfo>;
    fn capabilities(&self) -> lctrl_core::Result<lctrl_core::CapabilitySet>;

    /// Read the machine's identity modules (product, baseboard, BIOS).
    ///
    /// The default is an all-absent identity for platforms that have no
    /// implemented identity source; callers report it as absent rather than
    /// inventing values.
    fn system_identity(&self) -> lctrl_core::Result<lctrl_core::SystemIdentity> {
        Ok(lctrl_core::SystemIdentity::default())
    }

    /// Read processor identity, firmware-declared rated clocks and the derived
    /// current clock (read-only).
    ///
    /// The default reports the channel unsupported: a platform without an
    /// implemented clock source must not answer with a rated frequency dressed
    /// up as a live one (`docs/evidence/re-cpu-20261105.md` §4, §5).
    fn cpu_status(&self) -> lctrl_core::Result<lctrl_core::CpuStatus> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "cpu.status: no processor clock source".into(),
        })
    }
}
