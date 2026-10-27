use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, PowerMutation, PowerScheme, PowerSchemeId,
    PowerSettingValue, PowerValueRange, Result, validate_power_write,
};
use lctrl_hal::{PowerControl, poll_readback};

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
    /// Write one stored AC/DC index without reapplying the active scheme.
    fn write_value(
        &self,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: u32,
    ) -> Result<()>;
    /// Reapply the active scheme so prior writes take effect.
    ///
    /// This is deliberately a separate phase: if it fails, the transaction
    /// knows a value write may already have happened and must roll it back.
    fn apply_active_scheme(&self) -> Result<()>;
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
    fn power_value_range(&self, key: &lctrl_core::PowerSettingKey) -> Result<PowerValueRange> {
        self.api.range(key)
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
                self.set_value(key, source, value, apply, None)
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
        let previous_id = active.id.clone();
        let previous = PowerMutation::Activate(previous_id.clone());
        let requested = PowerMutation::Activate(target.clone());
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        if let Err(error) = self.api.activate(&target) {
            return if Self::preserve_prewrite_error(&error) {
                Err(error)
            } else {
                Err(self.rollback_activation(&previous_id, error))
            };
        }
        match poll_readback(&requested, 10, Duration::from_millis(50), || {
            Ok(PowerMutation::Activate(self.api.active_scheme()?.id))
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => Err(self.rollback_activation(&previous_id, error)),
        }
    }

    pub(crate) fn set_value(
        &self,
        key: lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: PowerSettingValue,
        apply: ApplyMode,
        range: Option<PowerValueRange>,
    ) -> Result<ChangeReport<PowerMutation>> {
        let previous_raw = self.api.read_value(&key, source)?;
        let range = match range {
            Some(range) => range,
            None => self.api.range(&key)?,
        };
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
            return Err(LctrlError::InvalidArgument {
                detail: "internal power mutation shape mismatch".into(),
            });
        };
        if let Err(error) = self.api.write_value(key, *source, value.get()) {
            return if Self::preserve_prewrite_error(&error) {
                Err(error)
            } else {
                Err(self.rollback_value(key, *source, previous_raw, &previous, &range, error))
            };
        }
        if let Err(error) = self.api.apply_active_scheme() {
            return Err(self.rollback_value(key, *source, previous_raw, &previous, &range, error));
        }
        match poll_readback(&requested, 10, Duration::from_millis(50), || {
            let actual_raw = self.api.read_value(key, *source)?;
            Ok(PowerMutation::SetValue {
                key: key.clone(),
                source: *source,
                value: PowerSettingValue::new(actual_raw, &range)?,
            })
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => {
                Err(self.rollback_value(key, *source, previous_raw, &previous, &range, error))
            }
        }
    }

    fn preserve_prewrite_error(error: &LctrlError) -> bool {
        matches!(
            error,
            LctrlError::PermissionDenied { .. } | LctrlError::ChannelUnavailable { .. }
        )
    }

    fn rollback_activation(&self, previous: &PowerSchemeId, error: LctrlError) -> LctrlError {
        let requested = PowerMutation::Activate(previous.clone());
        match self.api.activate(previous).and_then(|()| {
            poll_readback(&requested, 10, Duration::from_millis(50), || {
                Ok(PowerMutation::Activate(self.api.active_scheme()?.id))
            })
        }) {
            Ok(_) => error,
            Err(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "power scheme activation failed ({error}); rollback also failed ({rollback})"
                ),
            },
        }
    }

    fn rollback_value(
        &self,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        previous_raw: u32,
        previous: &PowerMutation,
        range: &PowerValueRange,
        error: LctrlError,
    ) -> LctrlError {
        match self
            .api
            .write_value(key, source, previous_raw)
            .and_then(|()| self.api.apply_active_scheme())
            .and_then(|()| {
                poll_readback(previous, 10, Duration::from_millis(50), || {
                    let actual_raw = self.api.read_value(key, source)?;
                    Ok(PowerMutation::SetValue {
                        key: key.clone(),
                        source,
                        value: PowerSettingValue::new(actual_raw, range)?,
                    })
                })
            }) {
            Ok(_) => error,
            Err(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "power value write failed ({error}); rollback also failed ({rollback})"
                ),
            },
        }
    }
}
