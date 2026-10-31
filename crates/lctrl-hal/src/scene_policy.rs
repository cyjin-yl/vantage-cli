use lctrl_core::{
    ApplyMode, Result, ScenePolicyLedger, ScenePolicyMatch, ScenePolicyReport, ScenePolicyRule,
};

/// Transparent, rollback-capable scene-policy control.
///
/// `rules` exposes the full declared ruleset (no hidden logic, no vendor AI/ML
/// model). `match_event` reports which rules fire for a candidate WMI event
/// class. `apply` dispatches one rule's allowlisted action through the same
/// verified control channel the CLI uses, with a readback, honoring dry-run.
/// `track` returns the local ledger of applied scene actions and their
/// rollbacks. A rule whose action's write channel is gated reports
/// `unavailable` rather than faking a scene change.
pub trait ScenePolicyControl: Send + Sync {
    /// The complete declared ruleset, for transparency.
    fn rules(&self) -> Vec<ScenePolicyRule>;

    /// Which rules fire for a candidate WMI event class (read-only).
    fn match_event(&self, event_class: &str) -> Vec<ScenePolicyMatch>;

    /// Apply the named rule's action (or report the plan under dry-run).
    fn apply(
        &self,
        rule_id: &str,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport>;

    /// The local ledger of applied scene actions and their rollbacks.
    fn track(&self) -> ScenePolicyLedger;
}
