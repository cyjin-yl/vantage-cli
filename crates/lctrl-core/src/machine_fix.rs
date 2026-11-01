//! Transparent, rollback-capable MachineFix contracts.
//!
//! A [`FixRule`] binds a named issue to an allowlisted repair primitive and its
//! rollback. Detection is read-only; repair is explicit, dry-run capable, and
//! read back. The local ledger records what was applied and how to undo it.
//! No vendor issue database or opaque "one-click fix" is admitted: every rule
//! is declared here and visible through `fix scan`.

use serde::Deserialize;
use serde::Serialize;

use crate::{LctrlError, Result};

/// A named, user-visible MachineFix issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixIssue {
    /// Stable identifier used by `fix run` (e.g. `service.stopped.wuauserv`).
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// What the detection looks for and what the repair does.
    pub detail: String,
}

impl FixIssue {
    pub fn validate(&self) -> Result<()> {
        require_text(&self.id, 128, "fix issue id")?;
        require_text(&self.title, 256, "fix issue title")?;
        require_text(&self.detail, 2048, "fix issue detail")?;
        Ok(())
    }
}

/// Allowlisted repair primitives. Only these may be executed by `fix run`;
/// anything outside this set is a contract gap, not a fix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixRepairKind {
    /// Start a named service and read back its running state.
    StartService { name: String },
    /// Stop a named service and read back its stopped state.
    StopService { name: String },
}

impl FixRepairKind {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::StartService { name } | Self::StopService { name } => {
                require_text(name, 256, "service name")
            }
        }
    }
}

/// A transparent rule: the issue, its repair, and its rollback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixRule {
    pub issue: FixIssue,
    pub repair: FixRepairKind,
    pub rollback: FixRepairKind,
}

impl FixRule {
    pub fn new(issue: FixIssue, repair: FixRepairKind, rollback: FixRepairKind) -> Result<Self> {
        issue.validate()?;
        repair.validate()?;
        rollback.validate()?;
        Ok(Self {
            issue,
            repair,
            rollback,
        })
    }
}

/// The read-only observation of one issue produced by `fix scan`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixObservation {
    pub issue: FixIssue,
    /// Whether the issue is currently present on this machine.
    pub present: bool,
    /// The observed state the probe measured (e.g. "service state = 1").
    pub observed: String,
}

/// The report produced by `fix run` for one issue.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixReport {
    pub issue_id: String,
    pub mode: crate::ApplyMode,
    /// State observed before the repair (dry-run and commit).
    pub previous: String,
    /// The requested repair primitive.
    pub requested: FixRepairKind,
    /// State read back after a committed repair; `None` during dry-run.
    pub actual: Option<String>,
    /// The declared rollback for this rule.
    pub rollback: FixRepairKind,
}

/// One entry in the local MachineFix tracking ledger.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixLedgerEntry {
    pub issue_id: String,
    pub applied_at_unix_ms: u64,
    pub repair: FixRepairKind,
    pub rollback: FixRepairKind,
    /// "applied" when the repair committed; "rolled_back" when undone.
    pub status: String,
}

/// The local MachineFix tracking ledger (`fix track`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixLedger {
    pub entries: Vec<FixLedgerEntry>,
}

fn require_text(value: &str, max: usize, label: &str) -> Result<()> {
    if value.is_empty() || value.len() > max || value.contains('\0') {
        return Err(LctrlError::InvalidArgument {
            detail: format!("{label} is empty, oversized, or contains NUL"),
        });
    }
    Ok(())
}
