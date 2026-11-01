use lctrl_core::{
    ApplyMode, BacklightState, ChangeReport, EyeCareLevel, PanelMode, PanelRefreshCapability,
    PanelRefreshRequest, RefreshMode, Result,
};

pub trait KeyboardControl: Send + Sync {
    fn backlight_state(&self) -> Result<BacklightState>;
    fn set_backlight(
        &self,
        level: u8,
        effect: lctrl_core::LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>>;
}

pub trait TouchpadControl: Send + Sync {
    fn touchpad_state(&self) -> Result<lctrl_core::DeviceState>;
    fn set_touchpad(
        &self,
        state: lctrl_core::DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<lctrl_core::DeviceState>>;

    /// Enable observed input leaves after an interrupted split-state transition.
    fn recover_touchpad(&self, _apply: ApplyMode) -> Result<lctrl_core::TouchpadRecoveryReport> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "touchpad.recover".into(),
        })
    }
}

pub trait PrivacyControl: Send + Sync {
    fn camera_state(&self) -> Result<lctrl_core::DeviceState>;
    fn set_camera(
        &self,
        state: lctrl_core::DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<lctrl_core::DeviceState>>;
}

pub trait PanelControl: Send + Sync {
    fn refresh_capability(&self) -> Result<PanelRefreshCapability>;
    fn refresh_mode(&self) -> Result<RefreshMode>;
    fn panel_mode(&self) -> Result<PanelMode>;
    fn set_refresh_rate(
        &self,
        request: PanelRefreshRequest,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PanelMode>>;
}

/// Brightness of a uniquely identified active internal panel. Implementations
/// must not redirect a transition to an external or newly attached monitor.
pub trait PanelBrightnessControl: Send + Sync {
    fn brightness(&self) -> Result<lctrl_core::PanelBrightness>;
    /// If provided, `expected_current` must match the freshly observed value
    /// before any write is attempted; it is not an atomic cross-process CAS.
    fn set_brightness(
        &self,
        percent: u8,
        expected_current: Option<u8>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<u8>>;
}

/// Panel image-quality features driven through the graphics driver control
/// library (Intel IGCL on the reference target). Gamut switching is not part
/// of this trait: on the reference target the original vendor path requires a
/// Lenovo-delivered monitor-control DLL, which the replacement must not
/// depend on (docs/RE-interfaces.md §15).
pub trait PanelFeaturesControl: Send + Sync {
    /// Current eye-care level as observed from the active 1D LUT.
    /// `Custom` reports a non-eye-care custom LUT; when the driver exposes no
    /// stable get-config the last process-local write is reported instead.
    fn eye_care(&self) -> Result<EyeCareLevel>;
    fn set_eye_care(
        &self,
        level: EyeCareLevel,
        apply: ApplyMode,
    ) -> Result<ChangeReport<EyeCareLevel>>;
    fn super_resolution(&self) -> Result<bool>;
    fn set_super_resolution(&self, enabled: bool, apply: ApplyMode) -> Result<ChangeReport<bool>>;
}
