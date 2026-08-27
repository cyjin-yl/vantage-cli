use lctrl_core::{ApplyMode, ChangeReport, PowerMutation, PowerScheme, Result};

pub trait PowerControl: Send + Sync {
    fn power_schemes(&self) -> Result<Vec<PowerScheme>>;
    fn active_power_scheme(&self) -> Result<PowerScheme>;
    fn power_value_range(
        &self,
        key: &lctrl_core::PowerSettingKey,
    ) -> Result<lctrl_core::PowerValueRange>;
    fn apply_power_mutation(
        &self,
        mutation: PowerMutation,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>>;
}
