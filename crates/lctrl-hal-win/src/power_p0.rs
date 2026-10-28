use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, PowerMutation, PowerRangeObservation, PowerScheme,
    PowerSchemeId, PowerSettingKey, PowerSettingObservation, PowerSettingValue, PowerSource,
    PowerSourceValues, PowerValueRange, Result, validate_power_write,
};
use lctrl_hal::{PowerControl, poll_readback};

#[cfg(any(windows, test))]
pub(crate) fn policy_override_from_status(status: u32) -> Result<bool> {
    const ERROR_ACCESS_DISABLED_BY_POLICY: u32 = 1260;
    match status {
        0 => Ok(false),
        ERROR_ACCESS_DISABLED_BY_POLICY => Ok(true),
        error => Err(crate::map_win_error(error, "PowerSettingAccessCheck")),
    }
}

pub trait PowerApi: Send + Sync {
    fn schemes(&self) -> Result<Vec<PowerScheme>>;
    fn active_scheme(&self) -> Result<PowerScheme>;
    fn activate(&self, id: &PowerSchemeId) -> Result<()>;
    fn read_value(
        &self,
        scheme: &PowerSchemeId,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
    ) -> Result<u32>;
    fn range(&self, key: &lctrl_core::PowerSettingKey) -> Result<PowerValueRange>;
    /// Report a group-policy override for the setting and source. This is not
    /// a general writability or permission check and is independent of scheme.
    fn policy_overridden(&self, key: &PowerSettingKey, source: PowerSource) -> Result<bool>;
    /// Write one stored AC/DC index without reapplying the active scheme.
    fn write_value(
        &self,
        scheme: &PowerSchemeId,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: u32,
    ) -> Result<()>;
    /// Reapply `scheme` only when observed active immediately before the call.
    /// Returns `false` without applying when a different scheme is active.
    /// The check and Windows API call are not atomic with other processes.
    fn apply_if_active(&self, scheme: &PowerSchemeId) -> Result<bool>;
}

#[cfg(test)]
mod policy_status_tests {
    use super::policy_override_from_status;
    use lctrl_core::LctrlError;

    #[test]
    fn only_policy_disabled_is_an_override() {
        assert!(!policy_override_from_status(0).unwrap());
        assert!(policy_override_from_status(1260).unwrap());
        assert!(matches!(
            policy_override_from_status(5),
            Err(LctrlError::PermissionDenied { .. })
        ));
        assert!(matches!(
            policy_override_from_status(87),
            Err(LctrlError::Unsupported { .. })
        ));
        let error = policy_override_from_status(1640).unwrap_err();
        assert!(matches!(&error, LctrlError::Io(_)));
        assert!(error.to_string().contains("1640"));
    }
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

    fn read_power_setting(
        &self,
        scheme: &PowerSchemeId,
        key: &PowerSettingKey,
    ) -> Result<PowerSettingObservation> {
        let ac = self.api.read_value(scheme, key, PowerSource::Ac)?;
        let dc = self.api.read_value(scheme, key, PowerSource::Dc)?;
        let range = match self.api.range(key) {
            Ok(range) => PowerRangeObservation::Available { range },
            Err(
                error @ (LctrlError::Unsupported { .. } | LctrlError::ChannelUnavailable { .. }),
            ) => PowerRangeObservation::Unavailable {
                detail: error.to_string(),
            },
            Err(error) => return Err(error),
        };
        let ac_policy = self.api.policy_overridden(key, PowerSource::Ac)?;
        let dc_policy = self.api.policy_overridden(key, PowerSource::Dc)?;
        Ok(PowerSettingObservation {
            scheme: scheme.clone(),
            key: key.clone(),
            values: PowerSourceValues::new(ac, dc),
            range,
            policy_overrides: PowerSourceValues::new(ac_policy, dc_policy),
        })
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
        self.require_active_scheme(&previous_id)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        if previous_id == target {
            return Ok(ChangeReport::committed(
                previous,
                requested.clone(),
                requested,
            ));
        }
        if let Err(error) = self.api.activate(&target) {
            return Err(self.rollback_activation(&previous_id, &target, error));
        }
        match poll_readback(&requested, 10, Duration::from_millis(50), || {
            Ok(PowerMutation::Activate(self.api.active_scheme()?.id))
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => Err(self.rollback_activation(&previous_id, &target, error)),
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
        let scheme = self.active_scheme_id()?;
        self.set_value_for_scheme(&scheme, key, source, value, apply, range)
    }

    pub(crate) fn active_scheme_id(&self) -> Result<PowerSchemeId> {
        Ok(self.api.active_scheme()?.id)
    }

    pub(crate) fn require_active_scheme(&self, expected: &PowerSchemeId) -> Result<()> {
        let actual = self.active_scheme_id()?;
        if &actual != expected {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "active power scheme changed during transaction: expected {expected}, actual {actual}"
                ),
            });
        }
        Ok(())
    }

    pub(crate) fn set_value_for_scheme(
        &self,
        scheme: &PowerSchemeId,
        key: lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        value: PowerSettingValue,
        apply: ApplyMode,
        range: Option<PowerValueRange>,
    ) -> Result<ChangeReport<PowerMutation>> {
        let previous_raw = self.api.read_value(scheme, &key, source)?;
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
        self.require_active_scheme(scheme)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let PowerMutation::SetValue { key, source, value } = &requested else {
            return Err(LctrlError::InvalidArgument {
                detail: "internal power mutation shape mismatch".into(),
            });
        };
        if let Err(error) = self.api.write_value(scheme, key, *source, value.get()) {
            return Err(self.rollback_value(
                scheme,
                key,
                *source,
                previous_raw,
                value.get(),
                error,
            ));
        }
        match self.api.apply_if_active(scheme) {
            Ok(true) => {}
            Ok(false) => {
                let error = LctrlError::ChannelUnavailable {
                    channel: format!("power scheme {scheme} became inactive after value write"),
                };
                return Err(self.rollback_value(
                    scheme,
                    key,
                    *source,
                    previous_raw,
                    value.get(),
                    error,
                ));
            }
            Err(error) => {
                return Err(self.rollback_value(
                    scheme,
                    key,
                    *source,
                    previous_raw,
                    value.get(),
                    error,
                ));
            }
        }
        match poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.require_active_scheme(scheme)?;
            let actual_raw = self.api.read_value(scheme, key, *source)?;
            Ok(PowerMutation::SetValue {
                key: key.clone(),
                source: *source,
                value: PowerSettingValue::new(actual_raw, &range)?,
            })
        }) {
            Ok(actual) => match self.require_active_scheme(scheme) {
                Ok(()) => Ok(ChangeReport::committed(previous, requested, actual)),
                Err(error) => {
                    Err(self.rollback_value(scheme, key, *source, previous_raw, value.get(), error))
                }
            },
            Err(error) => {
                Err(self.rollback_value(scheme, key, *source, previous_raw, value.get(), error))
            }
        }
    }

    fn rollback_activation(
        &self,
        previous: &PowerSchemeId,
        target: &PowerSchemeId,
        error: LctrlError,
    ) -> LctrlError {
        let current = match self.active_scheme_id() {
            Ok(current) => current,
            Err(readback) => {
                return LctrlError::FirmwareRejected {
                    detail: format!(
                        "power scheme activation failed ({error}); current scheme could not be read ({readback}); rollback was not attempted and state is unverified"
                    ),
                };
            }
        };
        if &current == previous {
            return error;
        }
        if &current != target {
            return LctrlError::FirmwareRejected {
                detail: format!(
                    "power scheme activation failed ({error}); current scheme {current} differs from previous {previous} and target {target}; rollback was not attempted"
                ),
            };
        }
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
        scheme: &PowerSchemeId,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        previous_raw: u32,
        attempted_raw: u32,
        error: LctrlError,
    ) -> LctrlError {
        match self.restore_value_for_scheme(scheme, key, source, previous_raw, attempted_raw) {
            Ok(_) => error,
            Err(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "power value write failed ({error}); rollback for scheme {scheme} also failed ({rollback}); state is unverified"
                ),
            },
        }
    }

    pub(crate) fn restore_value_for_scheme(
        &self,
        scheme: &PowerSchemeId,
        key: &lctrl_core::PowerSettingKey,
        source: lctrl_core::PowerSource,
        previous_raw: u32,
        attempted_raw: u32,
    ) -> Result<()> {
        let observed = self.api.read_value(scheme, key, source)?;
        if observed == previous_raw {
            return Ok(());
        }
        if observed != attempted_raw {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "scheme {scheme} value is {observed}, neither previous {previous_raw} nor attempted {attempted_raw}; recovery did not overwrite it"
                ),
            });
        }
        self.api.write_value(scheme, key, source, previous_raw)?;
        // A user-selected different active scheme must not be deliberately
        // reactivated merely to restore the saved value of this scheme.
        let _ = self.api.apply_if_active(scheme)?;
        poll_readback(&previous_raw, 10, Duration::from_millis(50), || {
            self.api.read_value(scheme, key, source)
        })?;
        Ok(())
    }
}
