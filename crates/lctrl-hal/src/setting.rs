use std::fmt::Display;

use lctrl_core::{ApplyMode, ChangeReport, LctrlError, Result};

pub trait Setting<T> {
    fn read(&self) -> Result<T>;
    fn write(&self, value: &T) -> Result<()>;
}

pub fn apply_setting<T>(
    setting: &dyn Setting<T>,
    requested: T,
    mode: ApplyMode,
) -> Result<ChangeReport<T>>
where
    T: Clone + PartialEq + Display,
{
    let previous = setting.read()?;
    if mode == ApplyMode::DryRun {
        return Ok(ChangeReport::dry_run(previous, requested));
    }

    setting.write(&requested)?;
    let actual = setting.read()?;
    if actual != requested {
        return Err(LctrlError::VerifyMismatch {
            requested: requested.to_string(),
            actual: actual.to_string(),
        });
    }

    Ok(ChangeReport::committed(previous, requested, actual))
}
