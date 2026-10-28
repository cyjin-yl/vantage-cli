use parking_lot::Mutex;

use lctrl_core::{
    ApplyMode, LctrlError, PowerScheme, PowerSchemeId, PowerSettingKey, PowerSource,
    PowerSourceValues, PowerValueRange, ProcessorBoostMode, Result,
};
use lctrl_hal::{PowerLimitKind, ProcessorPowerControl, TuningControl};
use lctrl_hal_win::{PowerApi, WindowsPowerP0, epp_percent_from_raw, epp_raw_from_percent};

const EPP_SETTING: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6863";
const BOOST_SETTING: &str = "be337238-0d82-4146-a960-4f3749d470c7";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    Read(PowerSource),
    Range,
    Write(PowerSource, u32),
    Apply(PowerSource),
}

#[derive(Clone, Copy)]
struct Values {
    epp: [u32; 2],
    boost: [u32; 2],
}

#[derive(Clone, Copy)]
struct ReadOverride {
    source: PowerSource,
    value: u32,
    remaining: usize,
}

struct FakePowerApi {
    values: Mutex<Values>,
    epp_range: PowerValueRange,
    calls: Mutex<Vec<Call>>,
    last_write: Mutex<Option<PowerSource>>,
    fail_apply_once: Mutex<Option<PowerSource>>,
    mismatch_next_write: Mutex<Option<(PowerSource, u32)>>,
    read_override: Mutex<Option<ReadOverride>>,
}

impl FakePowerApi {
    fn new(epp: [u32; 2], boost: [u32; 2]) -> Self {
        Self {
            values: Mutex::new(Values { epp, boost }),
            epp_range: PowerValueRange::new(0, 100, 1).unwrap(),
            calls: Mutex::new(Vec::new()),
            last_write: Mutex::new(None),
            fail_apply_once: Mutex::new(None),
            mismatch_next_write: Mutex::new(None),
            read_override: Mutex::new(None),
        }
    }

    fn with_epp_range(mut self, range: PowerValueRange) -> Self {
        self.epp_range = range;
        self
    }

    fn writes(&self) -> Vec<Call> {
        self.calls
            .lock()
            .iter()
            .filter(|call| matches!(call, Call::Write(_, _)))
            .copied()
            .collect()
    }
}

impl PowerApi for FakePowerApi {
    fn schemes(&self) -> Result<Vec<PowerScheme>> {
        Ok(Vec::new())
    }

    fn active_scheme(&self) -> Result<PowerScheme> {
        Ok(PowerScheme::new(
            PowerSchemeId::new("test-scheme")?,
            "Test",
            true,
        ))
    }

    fn activate(&self, _id: &PowerSchemeId) -> Result<()> {
        Err(LctrlError::Unsupported {
            feature: "test scheme activation".into(),
        })
    }

    fn read_value(
        &self,
        _: &PowerSchemeId,
        key: &PowerSettingKey,
        source: PowerSource,
    ) -> Result<u32> {
        self.calls.lock().push(Call::Read(source));
        {
            let mut override_slot = self.read_override.lock();
            if let Some(state) = override_slot.as_mut() {
                if state.source == source && state.remaining != 0 {
                    state.remaining -= 1;
                    return Ok(state.value);
                }
            }
            *override_slot = None;
        }
        let values = self.values.lock();
        let index = source_index(source);
        match key.setting.as_str() {
            EPP_SETTING => Ok(values.epp[index]),
            BOOST_SETTING => Ok(values.boost[index]),
            other => Err(LctrlError::InvalidArgument {
                detail: format!("unexpected test setting {other}"),
            }),
        }
    }

    fn range(&self, key: &PowerSettingKey) -> Result<PowerValueRange> {
        self.calls.lock().push(Call::Range);
        match key.setting.as_str() {
            EPP_SETTING => Ok(self.epp_range),
            BOOST_SETTING => PowerValueRange::new(0, 4, 1),
            other => Err(LctrlError::InvalidArgument {
                detail: format!("unexpected test setting {other}"),
            }),
        }
    }

    fn write_value(
        &self,
        _: &PowerSchemeId,
        key: &PowerSettingKey,
        source: PowerSource,
        value: u32,
    ) -> Result<()> {
        self.calls.lock().push(Call::Write(source, value));
        {
            let mut values = self.values.lock();
            let index = source_index(source);
            match key.setting.as_str() {
                EPP_SETTING => values.epp[index] = value,
                BOOST_SETTING => values.boost[index] = value,
                other => {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!("unexpected test setting {other}"),
                    });
                }
            }
        }
        *self.last_write.lock() = Some(source);
        let mismatch = {
            let mut configured = self.mismatch_next_write.lock();
            if configured
                .as_ref()
                .is_some_and(|(configured_source, _)| *configured_source == source)
            {
                configured.take()
            } else {
                None
            }
        };
        if let Some((_, actual)) = mismatch {
            *self.read_override.lock() = Some(ReadOverride {
                source,
                value: actual,
                remaining: 10,
            });
        }
        Ok(())
    }

    fn apply_if_active(&self, _: &PowerSchemeId) -> Result<bool> {
        let source = (*self.last_write.lock()).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "test apply had no preceding write".into(),
        })?;
        self.calls.lock().push(Call::Apply(source));
        let should_fail = {
            let mut failure = self.fail_apply_once.lock();
            if *failure == Some(source) {
                failure.take();
                true
            } else {
                false
            }
        };
        if should_fail {
            return Err(LctrlError::PermissionDenied {
                need: "administrator".into(),
            });
        }
        Ok(true)
    }
}

fn source_index(source: PowerSource) -> usize {
    match source {
        PowerSource::Ac => 0,
        PowerSource::Dc => 1,
    }
}

#[test]
fn epp_conversion_uses_the_documented_exact_integer_mapping() {
    assert_eq!(epp_raw_from_percent(0).unwrap(), 0);
    assert_eq!(epp_raw_from_percent(1).unwrap(), 2);
    assert_eq!(epp_raw_from_percent(50).unwrap(), 127);
    assert_eq!(epp_raw_from_percent(75).unwrap(), 191);
    assert_eq!(epp_raw_from_percent(100).unwrap(), 255);
    assert!(epp_raw_from_percent(101).is_err());

    for percent in 0..=100 {
        let raw = epp_raw_from_percent(percent).unwrap();
        assert_eq!(epp_percent_from_raw(raw).unwrap(), percent);
    }
    let representable = (0..=u8::MAX)
        .filter(|raw| epp_percent_from_raw(*raw).is_ok())
        .count();
    assert_eq!(representable, 101);
    for raw in [1, 128, 192, 254] {
        assert!(matches!(
            epp_percent_from_raw(raw),
            Err(LctrlError::InvalidArgument { .. })
        ));
    }
}

#[test]
fn epp_dry_run_captures_ac_and_dc_without_writing() {
    let p0 = WindowsPowerP0::new(FakePowerApi::new([50, 75], [1, 2]));
    let requested = PowerSourceValues::new(0, 100);

    let report = p0
        .set_epp_percent_ac_dc(requested, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(*report.previous(), PowerSourceValues::new(50, 75));
    assert_eq!(*report.requested(), requested);
    assert_eq!(report.actual(), None);
    assert_eq!(p0.api().values.lock().epp, [50, 75]);
    assert!(
        !p0.api()
            .calls
            .lock()
            .iter()
            .any(|call| matches!(call, Call::Write(_, _) | Call::Apply(_)))
    );
}

#[test]
fn epp_scheme_switch_between_sources_restores_only_original_scheme() {
    struct SwitchBeforeDc {
        active: Mutex<PowerSchemeId>,
        values: Mutex<[[u32; 2]; 2]>,
        switched: Mutex<bool>,
        writes: Mutex<Vec<PowerSchemeId>>,
    }

    impl PowerApi for SwitchBeforeDc {
        fn schemes(&self) -> Result<Vec<PowerScheme>> {
            Ok(vec![])
        }
        fn active_scheme(&self) -> Result<PowerScheme> {
            Ok(PowerScheme::new(self.active.lock().clone(), "active", true))
        }
        fn activate(&self, scheme: &PowerSchemeId) -> Result<()> {
            *self.active.lock() = scheme.clone();
            Ok(())
        }
        fn read_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            source: PowerSource,
        ) -> Result<u32> {
            let index = usize::from(scheme.as_str() == "performance");
            let value = self.values.lock()[index][source_index(source)];
            let mut switched = self.switched.lock();
            if source == PowerSource::Dc && !*switched {
                *switched = true;
                *self.active.lock() = PowerSchemeId::new("performance")?;
            }
            Ok(value)
        }
        fn range(&self, _: &PowerSettingKey) -> Result<PowerValueRange> {
            PowerValueRange::new(0, 100, 1)
        }
        fn write_value(
            &self,
            scheme: &PowerSchemeId,
            _: &PowerSettingKey,
            source: PowerSource,
            value: u32,
        ) -> Result<()> {
            self.writes.lock().push(scheme.clone());
            self.values.lock()[usize::from(scheme.as_str() == "performance")]
                [source_index(source)] = value;
            Ok(())
        }
        fn apply_if_active(&self, scheme: &PowerSchemeId) -> Result<bool> {
            Ok(&*self.active.lock() == scheme)
        }
    }

    let controller = WindowsPowerP0::new(SwitchBeforeDc {
        active: Mutex::new(PowerSchemeId::new("balanced").unwrap()),
        values: Mutex::new([[50, 75], [10, 20]]),
        switched: Mutex::new(false),
        writes: Mutex::new(Vec::new()),
    });
    assert!(
        controller
            .set_epp_percent_ac_dc(PowerSourceValues::new(0, 100), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(*controller.api().values.lock(), [[50, 75], [10, 20]]);
    assert_eq!(controller.api().active.lock().as_str(), "performance");
    assert!(
        controller
            .api()
            .writes
            .lock()
            .iter()
            .all(|id| id.as_str() == "balanced")
    );
}

#[test]
fn dc_failure_does_not_overwrite_a_concurrent_ac_edit() {
    struct ConcurrentAcEdit {
        values: Mutex<[u32; 2]>,
    }
    impl PowerApi for ConcurrentAcEdit {
        fn schemes(&self) -> Result<Vec<PowerScheme>> {
            Ok(vec![])
        }
        fn active_scheme(&self) -> Result<PowerScheme> {
            Ok(PowerScheme::new(
                PowerSchemeId::new("balanced")?,
                "Balanced",
                true,
            ))
        }
        fn activate(&self, _: &PowerSchemeId) -> Result<()> {
            unreachable!()
        }
        fn read_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            source: PowerSource,
        ) -> Result<u32> {
            Ok(self.values.lock()[source_index(source)])
        }
        fn range(&self, _: &PowerSettingKey) -> Result<PowerValueRange> {
            PowerValueRange::new(0, 100, 1)
        }
        fn write_value(
            &self,
            _: &PowerSchemeId,
            _: &PowerSettingKey,
            source: PowerSource,
            value: u32,
        ) -> Result<()> {
            let mut values = self.values.lock();
            if source == PowerSource::Dc && value == 100 {
                values[0] = 60;
                return Err(std::io::Error::other("DC completion lost").into());
            }
            values[source_index(source)] = value;
            Ok(())
        }
        fn apply_if_active(&self, _: &PowerSchemeId) -> Result<bool> {
            Ok(true)
        }
    }

    let controller = WindowsPowerP0::new(ConcurrentAcEdit {
        values: Mutex::new([50, 75]),
    });
    let error = controller
        .set_epp_percent_ac_dc(PowerSourceValues::new(0, 100), ApplyMode::Commit)
        .unwrap_err();
    assert!(error.to_string().contains("DC completion lost"));
    assert_eq!(*controller.api().values.lock(), [60, 75]);
}

#[test]
fn raw_epp_commit_writes_exact_percentages_and_preserves_previous_values() {
    let p0 = WindowsPowerP0::new(FakePowerApi::new([50, 75], [1, 2]));

    let report = p0
        .set_epp_ac_dc(PowerSourceValues::new(0, 255), ApplyMode::Commit)
        .unwrap();

    assert_eq!(*report.previous(), PowerSourceValues::new(127, 191));
    assert_eq!(*report.requested(), PowerSourceValues::new(0, 255));
    assert_eq!(report.actual(), Some(&PowerSourceValues::new(0, 255)));
    assert_eq!(p0.api().values.lock().epp, [0, 100]);
}

#[test]
fn epp_rejects_unrepresentable_and_unaligned_requests_before_writes() {
    let p0 = WindowsPowerP0::new(
        FakePowerApi::new([50, 50], [1, 1])
            .with_epp_range(PowerValueRange::new(0, 100, 10).unwrap()),
    );

    assert!(matches!(
        p0.set_epp_ac_dc(PowerSourceValues::same(128), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    // Raw 2 maps to 1%, which is not aligned to the advertised increment.
    assert!(matches!(
        p0.set_epp_ac_dc(PowerSourceValues::same(2), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(matches!(
        p0.set_epp_percent_ac_dc(PowerSourceValues::same(101), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(p0.api().writes().is_empty());
}

#[test]
fn epp_percentage_contract_is_enforced_independently_of_the_power_range() {
    let p0 = WindowsPowerP0::new(
        FakePowerApi::new([50, 50], [1, 1])
            .with_epp_range(PowerValueRange::new(0, 255, 1).unwrap()),
    );
    assert!(matches!(
        p0.set_epp_percent_ac_dc(PowerSourceValues::same(101), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(p0.api().writes().is_empty());

    let p0 = WindowsPowerP0::new(
        FakePowerApi::new([101, 50], [1, 1])
            .with_epp_range(PowerValueRange::new(0, 255, 1).unwrap()),
    );
    assert!(matches!(
        p0.epp_percent_ac_dc(),
        Err(LctrlError::InvalidArgument { .. })
    ));
}

#[test]
fn dc_active_plan_failure_rolls_back_both_source_values() {
    let api = FakePowerApi::new([50, 75], [1, 2]);
    *api.fail_apply_once.lock() = Some(PowerSource::Dc);
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.set_epp_percent_ac_dc(PowerSourceValues::new(0, 100), ApplyMode::Commit),
        Err(LctrlError::PermissionDenied { .. })
    ));
    assert_eq!(p0.api().values.lock().epp, [50, 75]);
}

#[test]
fn readback_mismatch_restores_both_previous_source_values() {
    let api = FakePowerApi::new([50, 75], [1, 2]);
    *api.mismatch_next_write.lock() = Some((PowerSource::Ac, 60));
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.set_epp_percent_ac_dc(PowerSourceValues::new(0, 100), ApplyMode::Commit),
        Err(LctrlError::VerifyMismatch { .. })
    ));
    assert_eq!(p0.api().values.lock().epp, [50, 75]);
}

#[test]
fn turbo_uses_boost_mode_and_restores_the_exact_previous_modes() {
    let api = FakePowerApi::new([50, 50], [2, 4]);
    *api.fail_apply_once.lock() = Some(PowerSource::Dc);
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.set_turbo(false, ApplyMode::Commit),
        Err(LctrlError::PermissionDenied { .. })
    ));
    assert_eq!(p0.api().values.lock().boost, [2, 4]);
}

#[test]
fn turbo_on_selects_enabled_and_reports_previous_boost_policies() {
    let p0 = WindowsPowerP0::new(FakePowerApi::new([50, 50], [2, 4]));

    let report = p0.set_turbo(true, ApplyMode::Commit).unwrap();

    assert_eq!(
        *report.previous(),
        PowerSourceValues::new(
            ProcessorBoostMode::AGGRESSIVE,
            ProcessorBoostMode::EFFICIENT_AGGRESSIVE,
        )
    );
    assert_eq!(
        *report.requested(),
        PowerSourceValues::same(ProcessorBoostMode::ENABLED)
    );
    assert_eq!(report.actual(), Some(report.requested()));
    assert_eq!(p0.api().values.lock().boost, [1, 1]);
}

#[test]
fn legacy_tuning_is_source_safe_and_does_not_fake_power_limits() {
    let p0 = WindowsPowerP0::new(FakePowerApi::new([50, 75], [1, 1]));
    assert!(matches!(
        p0.epp(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(matches!(
        p0.set_epp(0, ApplyMode::Commit),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(matches!(
        p0.power_limit(PowerLimitKind::Pl1),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(matches!(
        p0.set_power_limit(PowerLimitKind::Pl2, 30_000_000, ApplyMode::DryRun),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(p0.api().writes().is_empty());

    let p0 = WindowsPowerP0::new(FakePowerApi::new([50, 50], [1, 1]));
    let dry_run = p0.set_epp(0, ApplyMode::DryRun).unwrap();
    assert_eq!(*dry_run.previous(), 127);
    assert_eq!(dry_run.actual(), None);
    assert!(p0.api().writes().is_empty());

    let report = p0.set_epp(0, ApplyMode::Commit).unwrap();
    assert_eq!(*report.previous(), 127);
    assert_eq!(report.actual(), Some(&0));
    assert_eq!(p0.api().values.lock().epp, [0, 0]);
}
