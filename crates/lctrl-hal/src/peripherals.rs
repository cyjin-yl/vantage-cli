use lctrl_core::{
    ApplyMode, BacklightState, ChangeReport, PanelMode, PanelRefreshCapability,
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
    fn set_brightness(&self, percent: u8, apply: ApplyMode) -> Result<ChangeReport<u8>>;
}
