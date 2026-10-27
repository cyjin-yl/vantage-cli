pub trait Hal: Send + Sync {
    fn platform(&self) -> lctrl_core::Platform;
    fn hardware_info(&self) -> lctrl_core::Result<lctrl_core::HardwareInfo>;
    fn capabilities(&self) -> lctrl_core::Result<lctrl_core::CapabilitySet>;
}
