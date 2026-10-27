use lctrl_core::{
    ApplyMode, ChangeReport, DispatcherVersion, LctrlError, PerformanceCapabilities,
    PerformanceMode, PerformanceState, Result,
};
use lctrl_hal::PerformanceControl;

/// Reads the only verified performance readback surface on the target: the
/// Dispatcher PowerSlider registry values. Writing those values is not a
/// hardware-control substitute and is intentionally not supported here.
pub trait PerformanceRegistryReader: Send + Sync {
    fn read_dword(&self, value: &str) -> Result<u32>;
}

#[derive(Debug)]
pub struct WindowsPerformanceP0<R> {
    registry: R,
}

impl<R> WindowsPerformanceP0<R> {
    #[must_use]
    pub const fn new(registry: R) -> Self {
        Self { registry }
    }

    #[must_use]
    pub const fn registry(&self) -> &R {
        &self.registry
    }
}

impl<R> PerformanceControl for WindowsPerformanceP0<R>
where
    R: PerformanceRegistryReader,
{
    fn performance_state(&self) -> Result<PerformanceState> {
        let version = DispatcherVersion::from_raw(self.registry.read_dword("VERSION")?);
        let requested = decode_mode(self.registry.read_dword("CURRENT_SETTING")?);
        let active = decode_mode(self.registry.read_dword("CURRENT_STATE")?);
        let automatic = self.registry.read_dword("AUTOMATIC_MODE_SETTING")? != 0;
        let capabilities = PerformanceCapabilities::new(self.registry.read_dword("POWER_SLIDER")?);
        Ok(PerformanceState {
            requested,
            active,
            automatic,
            version,
            capabilities,
        })
    }

    fn set_performance_mode(
        &self,
        _mode: PerformanceMode,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>> {
        // SCM accepts unknown 0x80..0x8f controls without an observable
        // effect. No versioned semantic mapping is in the clean-room spec,
        // so refusing prevents fake success and avoids a blind code sweep.
        Err(LctrlError::Unsupported {
            feature: "perf.mode.set".into(),
        })
    }
}

fn decode_mode(raw: u32) -> Option<PerformanceMode> {
    PerformanceMode::from_its(raw).ok()
}
