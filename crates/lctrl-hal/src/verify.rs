use std::{fmt::Display, thread, time::Duration};

use lctrl_core::{LctrlError, Result};

/// Poll a channel-specific readback until it matches or the bounded policy is
/// exhausted. The first read is immediate; delay occurs only between misses.
pub fn poll_readback<T>(
    requested: &T,
    attempts: usize,
    delay: Duration,
    mut read: impl FnMut() -> Result<T>,
) -> Result<T>
where
    T: PartialEq + Display,
{
    if attempts == 0 {
        return Err(LctrlError::InvalidArgument {
            detail: "readback attempts must be nonzero".into(),
        });
    }
    let mut actual = read()?;
    for attempt in 1..attempts {
        if actual == *requested {
            return Ok(actual);
        }
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        actual = read()?;
        if attempt + 1 == attempts {
            break;
        }
    }
    if actual == *requested {
        Ok(actual)
    } else {
        Err(LctrlError::VerifyMismatch {
            requested: requested.to_string(),
            actual: actual.to_string(),
        })
    }
}
