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

pub trait PanelControl: Send + Sync {
    fn refresh_capability(&self) -> Result<PanelRefreshCapability>;
    fn refresh_mode(&self) -> Result<RefreshMode>;
}
