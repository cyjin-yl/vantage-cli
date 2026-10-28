use parking_lot::Mutex;

use lctrl_core::{
    ApplyMode, LctrlError, PowerGuid, PowerMutation, PowerRangeObservation, PowerScheme,
    PowerSchemeId, PowerSettingKey, PowerSettingValue, PowerSource, PowerSourceValues,
    PowerValueRange,
};
use lctrl_hal::PowerControl;
use lctrl_hal_win::{PowerApi, WindowsPowerP0};

struct FakePowerApi {
    schemes: Mutex<Vec<PowerScheme>>,
    active: Mutex<PowerSchemeId>,
    value: Mutex<u32>,
    calls: Mutex<Vec<&'static str>>,
    fail_next_apply: Mutex<bool>,
    fail_next_write: Mutex<bool>,
}

impl FakePowerApi {
    fn new() -> Self {
        let balanced = PowerScheme::new(PowerSchemeId::new("balanced").unwrap(), "Balanced", true);
        let performance = PowerScheme::new(
            PowerSchemeId::new("performance").unwrap(),
            "Performance",
            false,
        );
        Self {
            schemes: Mutex::new(vec![balanced.clone(), performance]),
            active: Mutex::new(balanced.id),
            value: Mutex::new(50),
            calls: Mutex::new(Vec::new()),
            fail_next_apply: Mutex::new(false),
            fail_next_write: Mutex::new(false),
        }
    }
}

impl PowerApi for FakePowerApi {
    fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
        self.calls.lock().push("schemes");
        Ok(self.schemes.lock().clone())
    }

    fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
        self.calls.lock().push("active");
        let active = self.active.lock();
        self.schemes
            .lock()
            .iter()
            .find(|scheme| scheme.id == *active)
            .cloned()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "missing active test scheme".into(),
            })
    }

    fn activate(&self, id: &PowerSchemeId) -> lctrl_core::Result<()> {
        self.calls.lock().push("activate");
        *self.active.lock() = id.clone();
        Ok(())
    }

    fn read_value(
        &self,
        _scheme: &PowerSchemeId,
        _key: &PowerSettingKey,
        _source: PowerSource,
    ) -> lctrl_core::Result<u32> {
        self.calls.lock().push("read_value");
        Ok(*self.value.lock())
    }

    fn range(&self, _key: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
        self.calls.lock().push("range");
        PowerValueRange::new(0, 100, 10)
    }

    fn policy_overridden(&self, _: &PowerSettingKey, _: PowerSource) -> lctrl_core::Result<bool> {
        unreachable!()
    }

    fn write_value(
        &self,
        _scheme: &PowerSchemeId,
        _key: &PowerSettingKey,
        _source: PowerSource,
        value: u32,
    ) -> lctrl_core::Result<()> {
        self.calls.lock().push("write_value");
        if std::mem::take(&mut *self.fail_next_write.lock()) {
            return Err(std::io::Error::other("write failed before effect").into());
        }
        *self.value.lock() = value;
        Ok(())
    }

    fn apply_if_active(&self, scheme: &PowerSchemeId) -> lctrl_core::Result<bool> {
        if &*self.active.lock() != scheme {
            return Ok(false);
        }
        self.calls.lock().push("apply_active");
        if std::mem::take(&mut *self.fail_next_apply.lock()) {
            return Err(LctrlError::PermissionDenied {
                need: "administrator".into(),
            });
        }
        Ok(true)
    }
}

fn mutation(value: u32) -> PowerMutation {
    let key = PowerSettingKey {
        subgroup: PowerGuid::new("subgroup").unwrap(),
        setting: PowerGuid::new("setting").unwrap(),
    };
    let range = PowerValueRange::new(0, 100, 10).unwrap();
    PowerMutation::SetValue {
        key,
        source: PowerSource::Ac,
        value: PowerSettingValue::new(value, &range).unwrap(),
    }
}

#[test]
fn dry_run_reads_current_value_but_does_not_write() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    let report = p0
        .apply_power_mutation(mutation(70), ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.actual(), None);
    assert_eq!(*p0.api().value.lock(), 50);
    assert!(!p0.api().calls.lock().contains(&"write_value"));
}

#[test]
fn set_value_reads_validates_writes_and_reads_back() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    let report = p0
        .apply_power_mutation(mutation(70), ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&mutation(70)));
    assert_eq!(*p0.api().value.lock(), 70);
}

#[test]
fn activate_requires_enumerated_scheme_and_readback() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);
    let request = PowerMutation::Activate(PowerSchemeId::new("performance").unwrap());

    let report = p0
        .apply_power_mutation(request.clone(), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&request));
    assert_eq!(p0.api().active.lock().as_str(), "performance");
}

#[test]
fn activate_rejects_unknown_scheme_before_write() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.apply_power_mutation(
            PowerMutation::Activate(PowerSchemeId::new("unknown").unwrap()),
            ApplyMode::Commit
        ),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert_eq!(p0.api().active.lock().as_str(), "balanced");
    assert!(!p0.api().calls.lock().contains(&"activate"));
}

#[test]
fn activation_failure_does_not_overwrite_an_external_third_scheme() {
    struct ExternalSwitch {
        active: Mutex<PowerSchemeId>,
    }
    impl PowerApi for ExternalSwitch {
        fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
            Ok(vec![PowerScheme::new(
                PowerSchemeId::new("target")?,
                "Target",
                false,
            )])
        }
        fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
            Ok(PowerScheme::new(self.active.lock().clone(), "Active", true))
        }
        fn activate(&self, id: &PowerSchemeId) -> lctrl_core::Result<()> {
            if id.as_str() == "target" {
                *self.active.lock() = PowerSchemeId::new("other")?;
                return Err(std::io::Error::other("activation completion lost").into());
            }
            *self.active.lock() = id.clone();
            Ok(())
        }
        fn read_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<u32> {
            unreachable!()
        }
        fn range(&self, _: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
            unreachable!()
        }
        fn policy_overridden(
            &self,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<bool> {
            unreachable!()
        }
        fn write_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
            _: u32,
        ) -> lctrl_core::Result<()> {
            unreachable!()
        }
        fn apply_if_active(&self, _: &PowerSchemeId) -> lctrl_core::Result<bool> {
            unreachable!()
        }
    }

    let controller = WindowsPowerP0::new(ExternalSwitch {
        active: Mutex::new(PowerSchemeId::new("previous").unwrap()),
    });
    assert!(
        controller
            .apply_power_mutation(
                PowerMutation::Activate(PowerSchemeId::new("target").unwrap()),
                ApplyMode::Commit,
            )
            .is_err()
    );
    assert_eq!(controller.api().active.lock().as_str(), "other");
}

#[test]
fn active_plan_failure_after_write_restores_the_previous_value() {
    let api = FakePowerApi::new();
    *api.fail_next_apply.lock() = true;
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.apply_power_mutation(mutation(70), ApplyMode::Commit),
        Err(LctrlError::PermissionDenied { .. })
    ));
    assert_eq!(*p0.api().value.lock(), 50);
}

#[test]
fn failed_write_with_previous_value_intact_needs_no_recovery_write() {
    let api = FakePowerApi::new();
    *api.fail_next_write.lock() = true;
    let p0 = WindowsPowerP0::new(api);

    assert!(
        p0.apply_power_mutation(mutation(70), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(*p0.api().value.lock(), 50);
    assert_eq!(
        p0.api()
            .calls
            .lock()
            .iter()
            .filter(|call| **call == "write_value")
            .count(),
        1
    );
}

#[test]
fn scheme_switch_after_snapshot_never_writes_a_different_scheme() {
    struct SwitchedPowerApi {
        active: Mutex<PowerSchemeId>,
        values: Mutex<[u32; 2]>,
        switched: Mutex<bool>,
    }

    impl PowerApi for SwitchedPowerApi {
        fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
            Ok(vec![])
        }

        fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
            let id = self.active.lock().clone();
            Ok(PowerScheme::new(id, "active", true))
        }

        fn activate(&self, id: &PowerSchemeId) -> lctrl_core::Result<()> {
            *self.active.lock() = id.clone();
            Ok(())
        }

        fn read_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<u32> {
            let mut active = self.active.lock();
            let index = usize::from(scheme.as_str() == "performance");
            let value = self.values.lock()[index];
            let mut switched = self.switched.lock();
            if !*switched {
                *switched = true;
                *active = PowerSchemeId::new("performance")?;
            }
            Ok(value)
        }

        fn range(&self, _: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
            PowerValueRange::new(0, 100, 10)
        }

        fn policy_overridden(
            &self,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<bool> {
            unreachable!()
        }

        fn write_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
            value: u32,
        ) -> lctrl_core::Result<()> {
            let index = usize::from(scheme.as_str() == "performance");
            self.values.lock()[index] = value;
            Ok(())
        }

        fn apply_if_active(&self, scheme: &PowerSchemeId) -> lctrl_core::Result<bool> {
            Ok(&*self.active.lock() == scheme)
        }
    }

    let controller = WindowsPowerP0::new(SwitchedPowerApi {
        active: Mutex::new(PowerSchemeId::new("balanced").unwrap()),
        values: Mutex::new([50, 90]),
        switched: Mutex::new(false),
    });
    assert!(
        controller
            .apply_power_mutation(mutation(70), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(*controller.api().values.lock(), [50, 90]);
}

#[test]
fn scheme_switch_after_write_restores_only_original_scheme_without_reactivating_it() {
    struct SwitchedAfterWrite {
        active: Mutex<PowerSchemeId>,
        values: Mutex<[u32; 2]>,
        writes: Mutex<Vec<(PowerSchemeId, u32)>>,
    }

    impl PowerApi for SwitchedAfterWrite {
        fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
            Ok(vec![])
        }
        fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
            Ok(PowerScheme::new(self.active.lock().clone(), "active", true))
        }
        fn activate(&self, id: &PowerSchemeId) -> lctrl_core::Result<()> {
            *self.active.lock() = id.clone();
            Ok(())
        }
        fn read_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<u32> {
            Ok(self.values.lock()[usize::from(scheme.as_str() == "performance")])
        }
        fn range(&self, _: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
            PowerValueRange::new(0, 100, 10)
        }
        fn policy_overridden(
            &self,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<bool> {
            unreachable!()
        }
        fn write_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
            value: u32,
        ) -> lctrl_core::Result<()> {
            let mut writes = self.writes.lock();
            writes.push((scheme.clone(), value));
            self.values.lock()[usize::from(scheme.as_str() == "performance")] = value;
            if writes.len() == 1 {
                *self.active.lock() = PowerSchemeId::new("performance")?;
            }
            Ok(())
        }
        fn apply_if_active(&self, scheme: &PowerSchemeId) -> lctrl_core::Result<bool> {
            Ok(&*self.active.lock() == scheme)
        }
    }

    let controller = WindowsPowerP0::new(SwitchedAfterWrite {
        active: Mutex::new(PowerSchemeId::new("balanced").unwrap()),
        values: Mutex::new([50, 90]),
        writes: Mutex::new(Vec::new()),
    });
    assert!(
        controller
            .apply_power_mutation(mutation(70), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(*controller.api().values.lock(), [50, 90]);
    assert_eq!(controller.api().active.lock().as_str(), "performance");
    assert!(
        controller
            .api()
            .writes
            .lock()
            .iter()
            .all(|(scheme, _)| scheme.as_str() == "balanced")
    );
}

#[test]
fn rollback_does_not_overwrite_a_distinct_concurrent_value() {
    struct ConcurrentValue {
        value: Mutex<u32>,
        failed: Mutex<bool>,
    }
    impl PowerApi for ConcurrentValue {
        fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
            Ok(vec![])
        }
        fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
            Ok(PowerScheme::new(
                PowerSchemeId::new("balanced")?,
                "Balanced",
                true,
            ))
        }
        fn activate(&self, _: &PowerSchemeId) -> lctrl_core::Result<()> {
            unreachable!()
        }
        fn read_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<u32> {
            Ok(*self.value.lock())
        }
        fn range(&self, _: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
            PowerValueRange::new(0, 100, 10)
        }
        fn policy_overridden(
            &self,
            _: &PowerSettingKey,
            _: PowerSource,
        ) -> lctrl_core::Result<bool> {
            unreachable!()
        }
        fn write_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            _: PowerSource,
            value: u32,
        ) -> lctrl_core::Result<()> {
            let mut failed = self.failed.lock();
            if !*failed {
                *failed = true;
                *self.value.lock() = 80;
                return Err(std::io::Error::other("completion lost after concurrent edit").into());
            }
            *self.value.lock() = value;
            Ok(())
        }
        fn apply_if_active(&self, _: &PowerSchemeId) -> lctrl_core::Result<bool> {
            Ok(true)
        }
    }

    let controller = WindowsPowerP0::new(ConcurrentValue {
        value: Mutex::new(50),
        failed: Mutex::new(false),
    });
    assert!(
        controller
            .apply_power_mutation(mutation(70), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(*controller.api().value.lock(), 80);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservationFailure {
    None,
    Read(PowerSource),
    ReadUnsupported(PowerSource),
    RangeUnsupported,
    RangeUnavailable,
    RangeUnavailableWithPolicyFailure(PowerSource),
    RangePermission,
    RangeInvalid,
    RangeIo,
    Policy(PowerSource),
    PolicyUnsupported(PowerSource),
    PolicyUnavailable(PowerSource),
}

#[derive(Debug, Eq, PartialEq)]
enum ObservationCall {
    Schemes,
    Active,
    Read(PowerSchemeId, PowerSettingKey, PowerSource),
    Range(PowerSettingKey),
    Policy(PowerSettingKey, PowerSource),
    Activate,
    Write,
    Apply,
}

struct ObservationApi {
    active: Mutex<PowerSchemeId>,
    calls: Mutex<Vec<ObservationCall>>,
    failure: ObservationFailure,
    switch_after_ac: bool,
}

impl ObservationApi {
    fn new(failure: ObservationFailure, switch_after_ac: bool) -> Self {
        Self {
            active: Mutex::new(PowerSchemeId::new("balanced").unwrap()),
            calls: Mutex::new(Vec::new()),
            failure,
            switch_after_ac,
        }
    }

    fn assert_no_mutations(&self) {
        assert!(self.calls.lock().iter().all(|call| !matches!(
            call,
            ObservationCall::Activate | ObservationCall::Write | ObservationCall::Apply
        )));
    }
}

impl PowerApi for ObservationApi {
    fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
        self.calls.lock().push(ObservationCall::Schemes);
        let active = self.active.lock();
        ["balanced", "selected", "external"]
            .into_iter()
            .map(|name| {
                let id = PowerSchemeId::new(name)?;
                let is_active = id == *active;
                Ok(PowerScheme::new(id, name, is_active))
            })
            .collect()
    }

    fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
        self.calls.lock().push(ObservationCall::Active);
        Ok(PowerScheme::new(self.active.lock().clone(), "active", true))
    }

    fn activate(&self, _: &PowerSchemeId) -> lctrl_core::Result<()> {
        self.calls.lock().push(ObservationCall::Activate);
        Err(LctrlError::Unsupported {
            feature: "unexpected observation activation".into(),
        })
    }

    fn read_value(
        &self,
        scheme: &PowerSchemeId,
        key: &PowerSettingKey,
        source: PowerSource,
    ) -> lctrl_core::Result<u32> {
        self.calls
            .lock()
            .push(ObservationCall::Read(scheme.clone(), key.clone(), source));
        if self.failure == ObservationFailure::Read(source) {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{source} value failed"),
            });
        }
        if self.failure == ObservationFailure::ReadUnsupported(source) {
            return Err(LctrlError::Unsupported {
                feature: format!("{source} value unsupported"),
            });
        }
        let values = match scheme.as_str() {
            "balanced" => [1, 2],
            "selected" => [20, 80],
            "external" => [3, 4],
            _ => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "requested scheme not found".into(),
                });
            }
        };
        if self.switch_after_ac && source == PowerSource::Ac {
            *self.active.lock() = PowerSchemeId::new("external")?;
        }
        Ok(values[usize::from(source == PowerSource::Dc)])
    }

    fn range(&self, key: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
        self.calls.lock().push(ObservationCall::Range(key.clone()));
        match self.failure {
            ObservationFailure::RangeUnsupported => Err(LctrlError::Unsupported {
                feature: "numeric range not supported".into(),
            }),
            ObservationFailure::RangeUnavailable
            | ObservationFailure::RangeUnavailableWithPolicyFailure(_) => {
                Err(LctrlError::ChannelUnavailable {
                    channel: "numeric range not present".into(),
                })
            }
            ObservationFailure::RangePermission => Err(LctrlError::PermissionDenied {
                need: "range access".into(),
            }),
            ObservationFailure::RangeInvalid => PowerValueRange::new(100, 0, 10),
            ObservationFailure::RangeIo => Err(std::io::Error::other("range I/O failed").into()),
            _ => PowerValueRange::new(0, 100, 10),
        }
    }

    fn policy_overridden(
        &self,
        key: &PowerSettingKey,
        source: PowerSource,
    ) -> lctrl_core::Result<bool> {
        self.calls
            .lock()
            .push(ObservationCall::Policy(key.clone(), source));
        if self.failure == ObservationFailure::Policy(source)
            || self.failure == ObservationFailure::RangeUnavailableWithPolicyFailure(source)
        {
            return Err(LctrlError::PermissionDenied {
                need: format!("{source} policy check"),
            });
        }
        if self.failure == ObservationFailure::PolicyUnsupported(source) {
            return Err(LctrlError::Unsupported {
                feature: format!("{source} policy unsupported"),
            });
        }
        if self.failure == ObservationFailure::PolicyUnavailable(source) {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{source} policy unavailable"),
            });
        }
        Ok(source == PowerSource::Ac)
    }

    fn write_value(
        &self,
        _: &PowerSchemeId,
        _: &PowerSettingKey,
        _: PowerSource,
        _: u32,
    ) -> lctrl_core::Result<()> {
        self.calls.lock().push(ObservationCall::Write);
        Err(LctrlError::Unsupported {
            feature: "unexpected observation write".into(),
        })
    }

    fn apply_if_active(&self, _: &PowerSchemeId) -> lctrl_core::Result<bool> {
        self.calls.lock().push(ObservationCall::Apply);
        Err(LctrlError::Unsupported {
            feature: "unexpected observation apply".into(),
        })
    }
}

fn observation_key() -> PowerSettingKey {
    PowerSettingKey {
        subgroup: PowerGuid::new("requested-subgroup").unwrap(),
        setting: PowerGuid::new("requested-setting").unwrap(),
    }
}

#[test]
fn observation_uses_requested_scheme_across_active_switch_and_never_writes() {
    let controller = WindowsPowerP0::new(ObservationApi::new(ObservationFailure::None, true));
    let scheme = PowerSchemeId::new("selected").unwrap();
    let key = observation_key();
    assert_eq!(controller.api().active.lock().as_str(), "balanced");

    let observation = controller.read_power_setting(&scheme, &key).unwrap();

    assert_eq!(observation.scheme, scheme);
    assert_eq!(observation.key, key);
    assert_eq!(observation.values, PowerSourceValues::new(20, 80));
    assert_eq!(
        observation.range,
        PowerRangeObservation::Available {
            range: PowerValueRange::new(0, 100, 10).unwrap(),
        }
    );
    assert_eq!(
        observation.policy_overrides,
        PowerSourceValues::new(true, false)
    );
    assert_eq!(controller.api().active.lock().as_str(), "external");
    controller.api().assert_no_mutations();
}

#[test]
fn unavailable_range_does_not_hide_values_or_policy() {
    for (failure, detail) in [
        (
            ObservationFailure::RangeUnsupported,
            "feature is unsupported: numeric range not supported",
        ),
        (
            ObservationFailure::RangeUnavailable,
            "channel is unavailable: numeric range not present",
        ),
    ] {
        let controller = WindowsPowerP0::new(ObservationApi::new(failure, false));
        let scheme = PowerSchemeId::new("selected").unwrap();
        let key = observation_key();
        let observation = controller.read_power_setting(&scheme, &key).unwrap();

        assert_eq!(observation.scheme, scheme);
        assert_eq!(observation.key, key);
        assert_eq!(observation.values, PowerSourceValues::new(20, 80));
        assert_eq!(
            observation.range,
            PowerRangeObservation::Unavailable {
                detail: detail.into(),
            }
        );
        assert_eq!(
            observation.policy_overrides,
            PowerSourceValues::new(true, false)
        );
        controller.api().assert_no_mutations();
    }
}

#[test]
fn observation_propagates_value_policy_and_nonavailability_range_errors_without_writes() {
    for (failure, code, detail) in [
        (
            ObservationFailure::Read(PowerSource::Ac),
            4,
            "ac value failed",
        ),
        (
            ObservationFailure::Read(PowerSource::Dc),
            4,
            "dc value failed",
        ),
        (
            ObservationFailure::ReadUnsupported(PowerSource::Dc),
            3,
            "dc value unsupported",
        ),
        (ObservationFailure::RangePermission, 5, "range access"),
        (
            ObservationFailure::RangeInvalid,
            2,
            "range min must not exceed max",
        ),
        (ObservationFailure::RangeIo, 1, "range I/O failed"),
        (
            ObservationFailure::Policy(PowerSource::Ac),
            5,
            "ac policy check",
        ),
        (
            ObservationFailure::Policy(PowerSource::Dc),
            5,
            "dc policy check",
        ),
        (
            ObservationFailure::PolicyUnsupported(PowerSource::Ac),
            3,
            "ac policy unsupported",
        ),
        (
            ObservationFailure::PolicyUnavailable(PowerSource::Dc),
            4,
            "dc policy unavailable",
        ),
        (
            ObservationFailure::RangeUnavailableWithPolicyFailure(PowerSource::Dc),
            5,
            "dc policy check",
        ),
    ] {
        let controller = WindowsPowerP0::new(ObservationApi::new(failure, false));
        let scheme = PowerSchemeId::new("selected").unwrap();
        let error = controller
            .read_power_setting(&scheme, &observation_key())
            .unwrap_err();

        assert_eq!(error.exit_code(), code, "failure: {failure:?}");
        assert!(error.to_string().contains(detail), "failure: {failure:?}");
        controller.api().assert_no_mutations();
    }
}

#[test]
fn missing_requested_scheme_is_not_replaced_with_the_active_scheme() {
    let controller = WindowsPowerP0::new(ObservationApi::new(ObservationFailure::None, false));
    let missing = PowerSchemeId::new("missing").unwrap();
    let error = controller
        .read_power_setting(&missing, &observation_key())
        .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::ChannelUnavailable { channel } if channel == "requested scheme not found"
    ));
    assert_eq!(controller.api().active.lock().as_str(), "balanced");
    controller.api().assert_no_mutations();
}
