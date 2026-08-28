use lctrl_core::{
    ApplyMode, BacklightState, ChangeReport, PanelRefreshCapability, RefreshMode, Result,
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
}
