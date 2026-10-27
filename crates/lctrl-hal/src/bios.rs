use std::collections::BTreeSet;

use lctrl_core::{
    BiosAppliedChange, BiosApplyEffect, BiosApplyPlan, BiosApplyResult, BiosAssistantCapabilities,
    BiosAssistantValue, BiosChange, BiosItem, BiosPasswordStatus, LctrlError, Result,
};

/// Read and stage the verified subset of Lenovo's BIOS WMI contract.
///
/// Staging and saving acknowledge only the WMI transaction state. They do not
/// claim that a setting is effective before a reboot or other firmware action.
pub trait BiosControl: Send + Sync {
    fn list(&self) -> Result<Vec<BiosItem>>;
    fn get(&self, name: &str) -> Result<Option<BiosItem>>;
    fn selections(&self, name: &str) -> Result<Vec<String>>;
    fn stage(&self, change: BiosChange) -> Result<()>;
    fn save(&self) -> Result<()>;
    fn discard(&self) -> Result<()>;
    fn password_status(&self) -> Result<BiosPasswordStatus>;
}

/// Read-only access to the numeric BIOS Assistant methods.
///
/// No `SetValue` operation is part of this contract. Implementations must
/// reject a response whose bit31 command-status flag is not set rather than
/// treating its other bits as valid capabilities or values.
pub trait BiosAssistantControl: Send + Sync {
    fn assistant_capabilities(&self) -> Result<BiosAssistantCapabilities>;
    fn assistant_value(&self, index_data: u32) -> Result<BiosAssistantValue>;
}

/// Validate an entire batch and capture every previous value without staging
/// anything. Names are de-duplicated with the same ASCII case-insensitive
/// semantics as [`BiosControl::get`]; selection values remain case-sensitive.
pub fn plan_bios_apply(
    bios: &dyn BiosControl,
    requested: &[BiosChange],
) -> Result<BiosApplyPlan> {
    validate_batch(requested)?;

    let mut changes = Vec::with_capacity(requested.len());
    for requested_change in requested {
        let name = requested_change.name.as_str();
        let current = bios.get(name)?.ok_or_else(|| LctrlError::InvalidArgument {
            detail: format!("BIOS setting {name:?} was not found"),
        })?;
        let selections = bios.selections(&current.name)?;
        if selections.is_empty() {
            return Err(LctrlError::Unsupported {
                feature: format!("bios.selections.{}", current.name),
            });
        }
        if !selections
            .iter()
            .any(|selection| selection == requested_change.value.as_str())
        {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "BIOS value {:?} is not one of the exact selections for {}: {}",
                    requested_change.value.as_str(),
                    current.name,
                    selections.join(", ")
                ),
            });
        }

        let previous = BiosChange::new(current.name.clone(), current.value)?;
        let requested = BiosChange::new(current.name, requested_change.value.as_str())?;
        changes.push(lctrl_core::BiosPlannedChange {
            previous,
            requested,
        });
    }
    Ok(BiosApplyPlan { changes })
}

/// Apply a batch as one firmware transaction.
///
/// Every selection and previous value is captured before the first stage. A
/// partial stage or failed save is discarded. After the sole initial save,
/// every mirror is read back; any readback failure restores and verifies all
/// previous values with a second transaction. Recovery errors are never hidden.
pub fn apply_bios_transaction(
    bios: &dyn BiosControl,
    requested: &[BiosChange],
) -> Result<BiosApplyResult> {
    let plan = plan_bios_apply(bios, requested)?;
    stage_and_save(
        bios,
        plan.changes.iter().map(|change| &change.requested),
    )?;

    let mut readback = Vec::with_capacity(plan.changes.len());
    if let Err(error) = readback_mirror(
        bios,
        plan.changes.iter().map(|change| &change.requested),
        |item| readback.push(item),
    ) {
        return Err(recover_previous(bios, &plan, error));
    }

    let changes = plan
        .changes
        .into_iter()
        .zip(readback)
        .map(|(planned, readback)| BiosAppliedChange {
            previous: planned.previous,
            requested: planned.requested,
            readback,
        })
        .collect();
    Ok(BiosApplyResult {
        accepted: true,
        saved: true,
        effect: BiosApplyEffect::PendingRebootOrUnknown,
        changes,
    })
}

fn validate_batch(requested: &[BiosChange]) -> Result<()> {
    if requested.is_empty() {
        return Err(LctrlError::InvalidArgument {
            detail: "BIOS apply requires at least one setting".into(),
        });
    }

    let mut names = BTreeSet::new();
    for change in requested {
        let name = change.name.as_str();
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(LctrlError::InvalidArgument {
                detail: format!("duplicate BIOS setting in apply batch: {name:?}"),
            });
        }
    }
    Ok(())
}

fn stage_and_save<'a>(
    bios: &dyn BiosControl,
    changes: impl IntoIterator<Item = &'a BiosChange>,
) -> Result<()> {
    for change in changes {
        if let Err(error) = bios.stage(change.clone()) {
            return Err(discard_after_failure(bios, error));
        }
    }
    if let Err(error) = bios.save() {
        return Err(discard_after_failure(bios, error));
    }
    Ok(())
}

fn discard_after_failure(bios: &dyn BiosControl, error: LctrlError) -> LctrlError {
    match bios.discard() {
        Ok(()) => error,
        Err(discard) => LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS transaction failed ({error}); discarding staged settings also failed ({discard})"
            ),
        },
    }
}

fn readback_mirror<'a>(
    bios: &dyn BiosControl,
    expected: impl IntoIterator<Item = &'a BiosChange>,
    mut observed: impl FnMut(BiosItem),
) -> Result<()> {
    for expected in expected {
        let actual = bios.get(expected.name.as_str())?.ok_or_else(|| {
            LctrlError::VerifyMismatch {
                requested: format!("{}={}", expected.name, expected.value),
                actual: "setting absent after save".into(),
            }
        })?;
        if !actual.name.eq_ignore_ascii_case(expected.name.as_str())
            || actual.value.as_str() != expected.value.as_str()
        {
            return Err(LctrlError::VerifyMismatch {
                requested: format!("{}={}", expected.name, expected.value),
                actual: format!("{}={}", actual.name, actual.value),
            });
        }
        observed(actual);
    }
    Ok(())
}

fn recover_previous(
    bios: &dyn BiosControl,
    plan: &BiosApplyPlan,
    original: LctrlError,
) -> LctrlError {
    let recovery = stage_and_save(
        bios,
        plan.changes.iter().map(|change| &change.previous),
    )
    .and_then(|()| {
        readback_mirror(
            bios,
            plan.changes.iter().map(|change| &change.previous),
            |_| {},
        )
    });
    match recovery {
        Ok(()) => original,
        Err(recovery) => LctrlError::FirmwareRejected {
            detail: format!(
                "BIOS apply readback failed ({original}); restoring all previous settings also failed ({recovery})"
            ),
        },
    }
}
