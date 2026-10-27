use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, PowerGuid, PowerMutation, PowerSettingKey,
    PowerSettingValue, PowerSource, PowerSourceValues, PowerValueRange, ProcessorBoostMode, Result,
};
use lctrl_hal::{PowerLimitKind, ProcessorPowerControl, TuningControl};

use crate::{PowerApi, WindowsPowerP0};

pub const PROCESSOR_SETTINGS_SUBGROUP: &str = "54533251-82be-4824-96c1-47b60b740d00";
pub const PROCESSOR_EPP_SETTING: &str = "36687f9e-e3a5-4dbf-b1dc-15eb381c6863";
pub const PROCESSOR_BOOST_SETTING: &str = "be337238-0d82-4146-a960-4f3749d470c7";

/// Convert Windows `PERFEPP` percentage units to the raw HWP byte using the
/// documented truncation formula: `(percent * 255) / 100`.
pub fn epp_raw_from_percent(percent: u32) -> Result<u8> {
    validate_epp_percent(percent)?;
    Ok(((percent * 255) / 100) as u8)
}

fn validate_epp_percent(percent: u32) -> Result<()> {
    if percent > 100 {
        return Err(LctrlError::InvalidArgument {
            detail: format!("Windows EPP percentage {percent} is outside 0..=100"),
        });
    }
    Ok(())
}

/// Convert a raw HWP EPP byte to the only Windows percentage that programs
/// exactly that byte. Windows has only 101 settings for 256 byte values, so
/// values with no exact inverse are rejected rather than silently rounded.
pub fn epp_percent_from_raw(raw: u8) -> Result<u32> {
    let raw_u32 = u32::from(raw);
    let percent = (raw_u32 * 100 + 254) / 255;
    if percent <= 100 && epp_raw_from_percent(percent)? == raw {
        return Ok(percent);
    }
    Err(LctrlError::InvalidArgument {
        detail: format!(
            "raw EPP value {raw} has no exact Windows PERFEPP percentage; choose a value produced by epp_raw_from_percent"
        ),
    })
}

fn processor_key(setting: &str) -> PowerSettingKey {
    PowerSettingKey {
        subgroup: PowerGuid::new(PROCESSOR_SETTINGS_SUBGROUP)
            .expect("static processor subgroup GUID is valid"),
        setting: PowerGuid::new(setting).expect("static processor setting GUID is valid"),
    }
}

fn epp_key() -> PowerSettingKey {
    processor_key(PROCESSOR_EPP_SETTING)
}

fn boost_key() -> PowerSettingKey {
    processor_key(PROCESSOR_BOOST_SETTING)
}

impl<A> WindowsPowerP0<A>
where
    A: PowerApi,
{
    /// Read both source policies in raw HWP `0..=255` units.
    pub fn epp_ac_dc(&self) -> Result<PowerSourceValues<u8>> {
        let percentages = self.epp_percent_ac_dc()?;
        Ok(PowerSourceValues::new(
            epp_raw_from_percent(percentages.ac)?,
            epp_raw_from_percent(percentages.dc)?,
        ))
    }

    /// Set both source policies from raw HWP bytes. Each byte must have an
    /// exact integer-percent representation; the previous AC/DC values are
    /// retained independently for rollback.
    pub fn set_epp_ac_dc(
        &self,
        values: PowerSourceValues<u8>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<u8>>> {
        let percentages = PowerSourceValues::new(
            epp_percent_from_raw(values.ac)?,
            epp_percent_from_raw(values.dc)?,
        );
        let report = self.set_epp_percent_ac_dc(percentages, apply)?;
        convert_change(&report, |value| epp_raw_from_percent(*value))
    }

    fn read_index_ac_dc(&self, key: &PowerSettingKey) -> Result<PowerSourceValues<u32>> {
        Ok(PowerSourceValues::new(
            self.api().read_value(key, PowerSource::Ac)?,
            self.api().read_value(key, PowerSource::Dc)?,
        ))
    }

    fn set_index_ac_dc(
        &self,
        key: PowerSettingKey,
        values: PowerSourceValues<u32>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<u32>>> {
        // Validate both requests before either source is written.
        let range = self.api().range(&key)?;
        let ac = PowerSettingValue::new(values.ac, &range)?;
        let dc = PowerSettingValue::new(values.dc, &range)?;
        let report = self.apply_source_pair(
            PowerMutation::SetValue {
                key: key.clone(),
                source: PowerSource::Ac,
                value: ac,
            },
            PowerMutation::SetValue {
                key,
                source: PowerSource::Dc,
                value: dc,
            },
            apply,
            range,
        )?;
        convert_change(&report, mutation_value)
    }

    fn apply_source_pair(
        &self,
        ac: PowerMutation,
        dc: PowerMutation,
        apply: ApplyMode,
        range: PowerValueRange,
    ) -> Result<ChangeReport<PowerSourceValues<PowerMutation>>> {
        let ac_report = self.apply_source_mutation(ac, apply, range)?;
        let dc_report = match self.apply_source_mutation(dc, apply, range) {
            Ok(report) => report,
            Err(error) => {
                if apply == ApplyMode::DryRun {
                    return Err(error);
                }
                // The failed DC mutation has already attempted its own
                // rollback. Restore the successful AC mutation as well.
                return Err(
                    match self.apply_source_mutation(
                        ac_report.previous().clone(),
                        ApplyMode::Commit,
                        range,
                    ) {
                        Ok(_) => error,
                        Err(rollback) => LctrlError::FirmwareRejected {
                            detail: format!(
                                "AC/DC power-policy write failed ({error}); restoring AC also failed ({rollback})"
                            ),
                        },
                    },
                );
            }
        };

        let previous =
            PowerSourceValues::new(ac_report.previous().clone(), dc_report.previous().clone());
        let requested =
            PowerSourceValues::new(ac_report.requested().clone(), dc_report.requested().clone());
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let actual = PowerSourceValues::new(
            ac_report.actual().cloned().ok_or_else(missing_actual)?,
            dc_report.actual().cloned().ok_or_else(missing_actual)?,
        );
        Ok(ChangeReport::committed(previous, requested, actual))
    }

    fn apply_source_mutation(
        &self,
        mutation: PowerMutation,
        apply: ApplyMode,
        range: PowerValueRange,
    ) -> Result<ChangeReport<PowerMutation>> {
        match mutation {
            PowerMutation::SetValue { key, source, value } => {
                self.set_value(key, source, value, apply, Some(range))
            }
            PowerMutation::Activate(_) => Err(LctrlError::ChannelUnavailable {
                channel: "processor source pair contained a scheme activation".into(),
            }),
        }
    }
}

impl<A> ProcessorPowerControl for WindowsPowerP0<A>
where
    A: PowerApi,
{
    fn epp_percent_ac_dc(&self) -> Result<PowerSourceValues<u32>> {
        let values = self.read_index_ac_dc(&epp_key())?;
        validate_epp_percent(values.ac)?;
        validate_epp_percent(values.dc)?;
        Ok(values)
    }

    fn set_epp_percent_ac_dc(
        &self,
        values: PowerSourceValues<u32>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<u32>>> {
        validate_epp_percent(values.ac)?;
        validate_epp_percent(values.dc)?;
        self.set_index_ac_dc(epp_key(), values, apply)
    }

    fn boost_ac_dc(&self) -> Result<PowerSourceValues<ProcessorBoostMode>> {
        Ok(self
            .read_index_ac_dc(&boost_key())?
            .map(ProcessorBoostMode::from_raw))
    }

    fn set_boost_ac_dc(
        &self,
        values: PowerSourceValues<ProcessorBoostMode>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<ProcessorBoostMode>>> {
        let report =
            self.set_index_ac_dc(boost_key(), values.map(ProcessorBoostMode::get), apply)?;
        convert_change(&report, |value| Ok(ProcessorBoostMode::from_raw(*value)))
    }
}

/// Legacy cross-platform tuning uses one raw HWP value. It is valid on this
/// backend only when AC and DC snapshots agree; consumers that need to retain
/// distinct source policies use `ProcessorPowerControl` instead.
impl<A> TuningControl for WindowsPowerP0<A>
where
    A: PowerApi,
{
    fn epp(&self) -> Result<u8> {
        single_epp(self.epp_ac_dc()?)
    }

    fn set_epp(&self, value: u8, apply: ApplyMode) -> Result<ChangeReport<u8>> {
        // Refuse a lossy legacy snapshot before performing either write.
        let previous = self.epp()?;
        let report = self.set_epp_ac_dc(PowerSourceValues::same(value), apply)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, value));
        }
        let actual = report.actual().map_or(value, |values| values.ac);
        Ok(ChangeReport::committed(previous, value, actual))
    }

    fn power_limit(&self, kind: PowerLimitKind) -> Result<u64> {
        Err(LctrlError::Unsupported {
            feature: power_limit_feature(kind).into(),
        })
    }

    fn set_power_limit(
        &self,
        kind: PowerLimitKind,
        _value: u64,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<u64>> {
        Err(LctrlError::Unsupported {
            feature: power_limit_feature(kind).into(),
        })
    }
}

fn power_limit_feature(kind: PowerLimitKind) -> &'static str {
    match kind {
        PowerLimitKind::Pl1 => "tune.pl1",
        PowerLimitKind::Pl2 => "tune.pl2",
        PowerLimitKind::Tau => "tune.tau",
    }
}

fn single_epp(values: PowerSourceValues<u8>) -> Result<u8> {
    if values.ac == values.dc {
        return Ok(values.ac);
    }
    Err(LctrlError::ChannelUnavailable {
        channel: format!(
            "legacy single-value EPP cannot represent AC={} and DC={}; use ProcessorPowerControl",
            values.ac, values.dc
        ),
    })
}

fn mutation_value(mutation: &PowerMutation) -> Result<u32> {
    match mutation {
        PowerMutation::SetValue { value, .. } => Ok(value.get()),
        PowerMutation::Activate(_) => Err(LctrlError::ChannelUnavailable {
            channel: "processor power transaction returned a scheme activation".into(),
        }),
    }
}

fn convert_change<T, U>(
    report: &ChangeReport<PowerSourceValues<T>>,
    mut convert: impl FnMut(&T) -> Result<U>,
) -> Result<ChangeReport<PowerSourceValues<U>>> {
    let previous = PowerSourceValues::new(
        convert(&report.previous().ac)?,
        convert(&report.previous().dc)?,
    );
    let requested = PowerSourceValues::new(
        convert(&report.requested().ac)?,
        convert(&report.requested().dc)?,
    );
    if report.mode() == ApplyMode::DryRun {
        return Ok(ChangeReport::dry_run(previous, requested));
    }
    let actual = report.actual().ok_or_else(missing_actual)?;
    Ok(ChangeReport::committed(
        previous,
        requested,
        PowerSourceValues::new(convert(&actual.ac)?, convert(&actual.dc)?),
    ))
}

fn missing_actual() -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: "committed power transaction omitted its readback value".into(),
    }
}
