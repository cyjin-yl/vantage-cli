use lctrl_core::{ApplyMode, FixLedger, FixObservation, FixReport, FixRule, Result};

/// Transparent, rollback-capable MachineFix control.
///
/// `rules` exposes the full declared ruleset (no hidden logic). `scan` runs the
/// read-only probes. `fix` applies one rule's allowlisted repair with readback,
/// honoring dry-run. `track` returns the local ledger of applied fixes.
pub trait MachineFixControl: Send + Sync {
    /// The complete declared ruleset, for transparency.
    fn rules(&self) -> Vec<FixRule>;

    /// Run every rule's read-only probe and report which issues are present.
    fn scan(&self) -> Result<Vec<FixObservation>>;

    /// Apply the named rule's repair (or report the plan under dry-run).
    fn fix(&self, issue_id: &str, mode: ApplyMode) -> Result<FixReport>;

    /// The local ledger of applied fixes and their rollbacks.
    fn track(&self) -> FixLedger;
}
