use std::fmt;

use serde::Serialize;

use crate::error::{LctrlError, Result};

/// A validated, nonempty power-scheme name. Construction is fallible;
/// the stored text is preserved verbatim (no trimming, no case folding).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PowerSchemeId(String);

/// A validated Windows-style GUID string. The canonical registry text is
/// preserved verbatim, including leading/trailing spaces if any (they are
/// never trimmed away).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PowerGuid(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Ac,
    Dc,
}

/// A named power setting inside a power scheme, addressed by its subgroup
/// GUID and its setting GUID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PowerSettingKey {
    pub subgroup: PowerGuid,
    pub setting: PowerGuid,
}

/// A validated, closed numeric range with a nonzero step measured from
/// `min`. The bounds themselves are always in-range.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PowerValueRange {
    pub min: u32,
    pub max: u32,
    pub increment: u32,
}

/// A validated power-setting index.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PowerSettingValue(u32);

/// Metadata returned while enumerating a power scheme.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PowerScheme {
    pub id: PowerSchemeId,
    pub name: String,
    pub active: bool,
}

/// A mutation on the power layer. Only operations that reuse an
/// already-enumerated scheme are representable; creating, cloning, or
/// deleting schemes is intentionally not expressible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerMutation {
    Activate(PowerSchemeId),
    SetValue {
        key: PowerSettingKey,
        source: PowerSource,
        value: PowerSettingValue,
    },
}

impl PowerSchemeId {
    pub fn new(text: impl Into<String>) -> Result<Self> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "power scheme id must not be empty or whitespace".into(),
            });
        }
        if text.contains('\0') {
            return Err(LctrlError::InvalidArgument {
                detail: "power scheme id must not contain NUL".into(),
            });
        }
        Ok(Self(text))
    }

    #[must_use]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl PowerGuid {
    pub fn new(text: impl Into<String>) -> Result<Self> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "power guid must not be empty or whitespace".into(),
            });
        }
        if text.contains('\0') {
            return Err(LctrlError::InvalidArgument {
                detail: "power guid must not contain NUL".into(),
            });
        }
        Ok(Self(text))
    }

    #[must_use]
    pub const fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl PowerValueRange {
    pub fn new(min: u32, max: u32, increment: u32) -> Result<Self> {
        if min > max {
            return Err(LctrlError::InvalidArgument {
                detail: "power value range min must not exceed max".into(),
            });
        }
        if increment == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: "power value range increment must be nonzero".into(),
            });
        }
        Ok(Self {
            min,
            max,
            increment,
        })
    }

    #[must_use]
    pub fn contains(&self, value: u32) -> bool {
        value >= self.min && value <= self.max
    }
}

impl PowerSettingValue {
    pub fn new(value: u32, range: &PowerValueRange) -> Result<Self> {
        validate_power_write(value, range)?;
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl PowerScheme {
    #[must_use]
    pub fn new(id: PowerSchemeId, name: impl Into<String>, active: bool) -> Self {
        Self {
            id,
            name: name.into(),
            active,
        }
    }
}

/// Validates a value before it is written to a power setting.
///
/// The value must lie within `[min, max]` and must be reachable from `min`
/// by whole steps of `range.increment`:
/// `value == min` always passes, and for `value > min` the remainder
/// `(value - min) % increment` must be zero.
pub fn validate_power_write(value: u32, range: &PowerValueRange) -> Result<()> {
    if range.min > range.max {
        return Err(LctrlError::InvalidArgument {
            detail: "power value range min must not exceed max".into(),
        });
    }
    if range.increment == 0 {
        return Err(LctrlError::InvalidArgument {
            detail: "power value range increment must be nonzero".into(),
        });
    }
    if !range.contains(value) {
        return Err(LctrlError::InvalidArgument {
            detail: format!(
                "power value {value} outside range [{}, {}]",
                range.min, range.max
            ),
        });
    }
    let offset = value - range.min;
    if offset % range.increment != 0 {
        return Err(LctrlError::InvalidArgument {
            detail: format!(
                "power value {value} not aligned to increment {} from min {}",
                range.increment, range.min
            ),
        });
    }
    Ok(())
}

impl fmt::Display for PowerSchemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for PowerGuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl fmt::Display for PowerSettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for PowerSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ac => "ac",
            Self::Dc => "dc",
        })
    }
}
