use lctrl_core::{
    ApplyMode, ChangeReport, DispatcherVersion, ItsPowerSlider, LctrlError,
    PerformanceCapabilities, PerformanceMode, PerformanceState, Result,
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

    /// Persist a PowerSlider `REG_DWORD`. Used only when the ITS dispatcher
    /// does not own the mode (automatic mode off, ModernITS not asserted).
    /// Implemented on `NativePerformanceRegistry`; the default refuses so a
    /// read-only test fixture stays honest.
    fn write_dword(&self, value: &str, data: u32) -> Result<()> {
        let _ = data;
        Err(LctrlError::Unsupported {
            feature: format!("PowerSlider registry write {value:?}"),
        })
    }
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
        // Each value is read independently. A field the installed dispatcher
        // does not publish must not abort the report, and the older schema
        // (VERSION / CURRENT_SETTING / CURRENT_STATE / POWER_SLIDER) and the
        // ITS-era schema (ITS_*) are both accepted.
        let read = |name: &str| self.registry.read_dword(name).ok();

        let version_raw = read("VERSION");
        // `Legacy(0)` records "the dispatcher published no version", which is
        // the truth for the ITS-era key; it is not a decoded version.
        let version = version_raw.map_or(DispatcherVersion::Legacy(0), DispatcherVersion::from_raw);

        let requested_raw = read("ITS_CurrentSetting").or_else(|| read("CURRENT_SETTING"));
        let active_raw = read("ITS_CurrentSettingV").or_else(|| read("CURRENT_STATE"));
        let automatic_raw =
            read("ITS_AutomaticModeSetting").or_else(|| read("AUTOMATIC_MODE_SETTING"));
        // `POWER_SLIDER` is the legacy capability mask. The ITS-era masks
        // (`ITS_HW_Capability`, `ITS_FN_Capability`, `ITS_SE_AutoModeCapability`)
        // are carried raw below but are deliberately not attributed to modes:
        // the three disagree and their bit assignment is unproven.
        let capabilities = PerformanceCapabilities::new(read("POWER_SLIDER").unwrap_or(0));

        let its = ItsPowerSlider {
            current_setting: read("ITS_CurrentSetting"),
            current_setting_v: read("ITS_CurrentSettingV"),
            automatic_mode_setting: read("ITS_AutomaticModeSetting"),
            hw_capability: read("ITS_HW_Capability"),
            fn_capability: read("ITS_FN_Capability"),
            fan_mode: read("ITS_FanMode"),
        };
        let its = (its != ItsPowerSlider::default()).then_some(its);

        // Mode decoding uses `PerformanceMode::from_its`, whose mapping is
        // corroborated by the dispatcher's own `(1:BSM, 3:EPM)` status string:
        // 1 = Balanced (BSM), 3 = Performance (EPM). An out-of-range value is
        // reported as unknown rather than coerced to a neighbouring mode.
        Ok(PerformanceState {
            requested: requested_raw.and_then(decode_mode),
            active: active_raw.and_then(decode_mode),
            automatic: automatic_raw.is_some_and(|value| value != 0),
            version,
            capabilities,
            its,
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
        let previous =
            mode_for_overlay(&previous_guid).ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!(
                    "current power overlay {previous_guid} is not a scheme this build owns; \
                     refusing a change that could not be rolled back"
                ),
            })?;

        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }

        self.overlay.set_active(&target)?;

        let observed_guid = self.overlay.actual()?;
        match mode_for_overlay(&observed_guid) {
            Some(actual) if actual == mode => {
                // Also drive the vendor ITS tier when the dispatcher does not
                // own it. `ITS_AutomaticModeSetting == 7` means the dispatcher
                // chooses ITS_CurrentSetting itself; `Policy_ModernITS`
                // asserts an active policy owner. Writing then would fight the
                // firmware/service, so we report the overlay change and leave
                // the tier alone.
                self.sync_its_tier(mode, previous, &target, &previous_guid, actual)
            }
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

impl<R: PerformanceRegistryReader, O: PowerOverlayControl> WindowsPerformanceP0<R, O> {
    /// Best-effort ITS-tier write; honest on failure. Only runs after the
    /// overlay already committed to `mode`. Rolls back the ITS value and the
    /// overlay if the ITS readback does not confirm the requested tier.
    fn sync_its_tier(
        &self,
        mode: PerformanceMode,
        previous: PerformanceMode,
        target: &OverlayGuid,
        previous_guid: &OverlayGuid,
        actual: PerformanceMode,
    ) -> Result<ChangeReport<PerformanceMode>> {
        const AUTO_MODE_SENTINEL: u32 = 7;
        let read = |value: &str| self.registry.read_dword(value).ok();

        // Vendor-only modes have no documented ITS tier; overlay-only change.
        let its_tier = mode.its_value();
        if its_tier == 0 {
            return Ok(ChangeReport::committed(previous, mode, actual));
        }

        // Only drive the tier when the modern `ITS_*` schema is actually
        // present; a legacy-only surface (CURRENT_SETTING/CURRENT_STATE) is a
        // different dispatcher and we must not fabricate modern keys on it.
        if read("ITS_CurrentSetting").is_none() {
            return Ok(ChangeReport::committed(previous, mode, actual));
        }

        let auto_mode = read("ITS_AutomaticModeSetting").or_else(|| read("AUTOMATIC_MODE_SETTING"));
        let modern_its = read("Policy_ModernITS").or_else(|| read("POLICY_MODERNITS"));
        if auto_mode == Some(AUTO_MODE_SENTINEL) || modern_its == Some(1) {
            // Dispatcher/policy owns the tier; overlay change stands on its own.
            return Ok(ChangeReport::committed(previous, mode, actual));
        }

        let prior_its = read("ITS_CurrentSetting").or_else(|| read("CURRENT_SETTING"));
        self.registry
            .write_dword("ITS_CurrentSetting", its_tier)
            .map_err(|e| LctrlError::FirmwareRejected {
                detail: format!("ITS tier write failed: {e}"),
            })?;

        // Read back the value we wrote first — that proves our write landed.
        // `ITS_CurrentSettingV` is the dispatcher's own confirmed echo and lags
        // a manual write, so prefer the direct setting mirror.
        let observed = read("ITS_CurrentSetting")
            .or_else(|| read("CURRENT_SETTING"))
            .or_else(|| read("ITS_CurrentSettingV"));
        if observed == Some(its_tier) {
            return Ok(ChangeReport::committed(previous, mode, actual));
        }

        // Readback mismatch: restore ITS and the overlay.
        if let Some(prior) = prior_its {
            let _ = self.registry.write_dword("ITS_CurrentSetting", prior);
        }
        let _ = self.overlay.set_active(previous_guid);
        Err(LctrlError::VerifyMismatch {
            requested: format!("ITS tier {its_tier} ({mode}, overlay {target})"),
            actual: format!("ITS readback {observed:?}"),
        })
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
