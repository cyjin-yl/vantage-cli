use lctrl_core::{
    ApplyMode, ChangeReport, DispatcherVersion, LctrlError, PerformanceCapabilities,
    PerformanceMode, PerformanceState, Result,
};
use lctrl_hal::PerformanceControl;

use crate::power_overlay::{
    NativePowerOverlay, OverlayGuid, PowerOverlayControl, mode_for_overlay, overlay_for_mode,
};

/// Reads the vendor's own performance readback surface: the Dispatcher
/// PowerSlider registry values. This is a *read* of the vendor's published
/// state and never drives hardware.
pub trait PerformanceRegistryReader: Send + Sync {
    fn read_dword(&self, value: &str) -> Result<u32>;
}

/// Windows performance control.
///
/// Hardware changes go through the OS power overlay scheme
/// (`powrprof!PowerSetActiveOverlayScheme`); nothing here calls Lenovo
/// services, DLLs, or drivers. The registry reader only supplies the vendor's
/// requested/active view for reporting.
#[derive(Debug)]
pub struct WindowsPerformanceP0<R, O = NativePowerOverlay> {
    registry: R,
    overlay: O,
}

impl<R> WindowsPerformanceP0<R, NativePowerOverlay> {
    #[must_use]
    pub const fn new(registry: R) -> Self {
        Self {
            registry,
            overlay: NativePowerOverlay,
        }
    }
}

impl<R, O> WindowsPerformanceP0<R, O> {
    #[must_use]
    pub const fn with_overlay(registry: R, overlay: O) -> Self {
        Self { registry, overlay }
    }

    #[must_use]
    pub const fn registry(&self) -> &R {
        &self.registry
    }

    #[must_use]
    pub const fn overlay(&self) -> &O {
        &self.overlay
    }
}

impl<R, O> PerformanceControl for WindowsPerformanceP0<R, O>
where
    R: PerformanceRegistryReader,
    O: PowerOverlayControl,
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
        mode: PerformanceMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>> {
        // The OS defines exactly three overlay schemes. Vendor-only modes have
        // no documented Windows equivalent and are refused instead of being
        // approximated by a different behaviour.
        let target = overlay_for_mode(mode)?;

        let previous_guid = self.overlay.actual()?;
        let previous = mode_for_overlay(&previous_guid).ok_or_else(|| {
            LctrlError::ChannelUnavailable {
                channel: format!(
                    "current power overlay {previous_guid} is not a scheme this build owns; \
                     refusing a change that could not be rolled back"
                ),
            }
        })?;

        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }

        self.overlay.set_active(&target)?;

        let observed_guid = self.overlay.actual()?;
        match mode_for_overlay(&observed_guid) {
            Some(actual) if actual == mode => Ok(ChangeReport::committed(previous, mode, actual)),
            _ => {
                let restore = self.overlay.set_active(&previous_guid);
                match restore {
                    Ok(()) => Err(LctrlError::VerifyMismatch {
                        requested: format!("{mode} ({target})"),
                        actual: format!("{observed_guid}"),
                    }),
                    Err(restore_error) => Err(LctrlError::ChannelUnavailable {
                        channel: format!(
                            "overlay readback was {observed_guid} instead of {target}, and \
                             restoring {previous_guid} also failed: {restore_error}"
                        ),
                    }),
                }
            }
        }
    }
}

fn decode_mode(raw: u32) -> Option<PerformanceMode> {
    PerformanceMode::from_its(raw).ok()
}

/// Convenience for callers that only need the OS overlay view.
pub fn read_effective_mode<O: PowerOverlayControl>(overlay: &O) -> Result<Option<PerformanceMode>> {
    let guid: OverlayGuid = overlay.effective()?;
    Ok(mode_for_overlay(&guid))
}
