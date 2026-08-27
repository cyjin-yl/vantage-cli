use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, PowerMutation, PowerScheme, PowerSchemeId,
    PowerSettingValue, PowerValueRange, Result, validate_power_write,
};
use lctrl_hal::PowerControl;

pub trait PowerApi: Send + Sync {
    fn schemes(&self) -> Result<Vec<PowerScheme>>;
    fn active_scheme(&self) -> Result<PowerScheme>;
    fn activate(&self, id: &PowerSchemeId) -> Result<()>;
    fn read_value(
        &self,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
    ) -> Result<u32>;
    fn range(&self, key: &lctrl_core::PowerSettingKey) -> Result<PowerValueRange>;
    fn write_value(
        &self,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: u32,
    ) -> Result<()>;
}

#[derive(Debug)]
pub struct WindowsPowerP0<A> {
    api: A,
}

impl<A> WindowsPowerP0<A> {
    #[must_use]
    pub const fn new(api: A) -> Self {
        Self { api }
    }

    #[must_use]
    pub const fn api(&self) -> &A {
        &self.api
    }
}

impl<A> PowerControl for WindowsPowerP0<A>
where
    A: PowerApi,
{
    fn power_schemes(&self) -> Result<Vec<PowerScheme>> {
        self.api.schemes()
    }

    fn active_power_scheme(&self) -> Result<PowerScheme> {
        self.api.active_scheme()
    }

    fn apply_power_mutation(
        &self,
        mutation: PowerMutation,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>> {
        match mutation {
            PowerMutation::Activate(target) => self.activate(target, apply),
            PowerMutation::SetValue { key, source, value } => {
                self.set_value(key, source, value, apply)
            }
        }
    }
}

impl<A> WindowsPowerP0<A>
where
    A: PowerApi,
{
    fn activate(
        &self,
        target: PowerSchemeId,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>> {
        let active = self.api.active_scheme()?;
        if !self.api.schemes()?.iter().any(|scheme| scheme.id == target) {
            return Err(LctrlError::InvalidArgument {
                detail: format!("power scheme {target} is not enumerated"),
            });
        }
        let previous = PowerMutation::Activate(active.id);
        let requested = PowerMutation::Activate(target.clone());
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.api.activate(&target)?;
        let actual = PowerMutation::Activate(self.api.active_scheme()?.id);
        if actual != requested {
            return Err(verify_mismatch(&requested, &actual));
        }
        Ok(ChangeReport::committed(previous, requested, actual))
    }

    fn set_value(
        &self,
        key: lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: PowerSettingValue,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>> {
        let previous_raw = self.api.read_value(&key, source)?;
        let range = self.api.range(&key)?;
        let previous = PowerMutation::SetValue {
            key: key.clone(),
            source,
            value: PowerSettingValue::new(previous_raw, &range)?,
        };
        validate_power_write(value.get(), &range)?;
        let requested = PowerMutation::SetValue { key, source, value };
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let PowerMutation::SetValue { key, source, value } = &requested else {
            unreachable!("constructed above")
        };
        self.api.write_value(key, *source, value.get())?;
        let actual_raw = self.api.read_value(key, *source)?;
        let actual = PowerMutation::SetValue {
            key: key.clone(),
            source: *source,
            value: PowerSettingValue::new(actual_raw, &range)?,
        };
        if actual != requested {
            return Err(verify_mismatch(&requested, &actual));
        }
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

fn verify_mismatch(requested: &PowerMutation, actual: &PowerMutation) -> LctrlError {
    LctrlError::VerifyMismatch {
        requested: format!("{requested:?}"),
        actual: format!("{actual:?}"),
    }
}
