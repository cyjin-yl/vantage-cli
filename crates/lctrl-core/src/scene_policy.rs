//! Transparent, rollback-capable scene-policy contracts.
//!
//! A [`ScenePolicyRule`] binds a documented Lenovo WMI event class to an
//! allowlisted control action and its rollback. The ruleset is fully declared
//! and visible through `scene list`. Applying a rule dispatches through the
//! same verified control channel the CLI uses, with a readback and a dry-run
//! plan. The local ledger records what was applied and how to undo it.
//!
//! No vendor AI/ML scene model is admitted: every rule is declared here, and a
//! rule whose action's write channel is gated reports `unavailable` rather than
//! faking a scene change.

use serde::{Deserialize, Serialize};

use crate::{ApplyMode, FanMode, LctrlError, PerformanceMode, Result};

/// Allowlisted scene-policy actions. Only these may be dispatched by a rule;
/// anything outside this set is a contract gap, not a scene adaptation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneAction {
    /// Switch the active Windows power scheme; read back via the active scheme.
    SetPowerScheme { scheme: String },
    /// Set the panel refresh rate; read back via the current display mode.
    SetPanelRefresh { hz: u32 },
    /// Request a semantic fan mode; gated when no verified write channel exists.
    SetFanMode { mode: FanMode },
    /// Request an EC/firmware performance mode; gated when unavailable.
    SetPerformanceMode { mode: PerformanceMode },
}

impl SceneAction {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::SetPowerScheme { scheme } => require_text(scheme, 128, "power scheme name"),
            Self::SetPanelRefresh { hz } => {
                if !matches!(hz, 30..=240) {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!("panel refresh {hz} Hz is outside the 30..=240 envelope"),
                    });
                }
                Ok(())
            }
            Self::SetFanMode { .. } | Self::SetPerformanceMode { .. } => Ok(()),
        }
    }
}

/// A transparent scene-policy rule: the event class that fires it, the action
/// it dispatches, and its rollback.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicyRule {
    /// Stable identifier used by `scene test` (e.g. `scene.refresh.rate`).
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// The documented Lenovo WMI event class that triggers this rule.
    pub event_class: String,
    /// The allowlisted action dispatched when the event fires.
    pub action: SceneAction,
    /// The rollback action, when the inverse is derivable. A power-scheme
    /// switch has no fixed inverse (the previous scheme is captured at apply
    /// time), so only the mode-class actions carry one.
    pub rollback: Option<SceneAction>,
    /// What the rule fires on and what it does.
    pub detail: String,
}

impl ScenePolicyRule {
    pub fn new(
        id: String,
        title: String,
        event_class: String,
        action: SceneAction,
        rollback: Option<SceneAction>,
        detail: String,
    ) -> Result<Self> {
        require_text(&id, 128, "scene rule id")?;
        require_text(&title, 256, "scene rule title")?;
        require_text(&event_class, 128, "scene event class")?;
        action.validate()?;
        if let Some(rollback) = &rollback {
            rollback.validate()?;
        }
        require_text(&detail, 2048, "scene rule detail")?;
        Ok(Self {
            id,
            title,
            event_class,
            action,
            rollback,
            detail,
        })
    }
}

/// The read-only observation of whether one rule fires for a candidate event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicyMatch {
    pub rule: ScenePolicyRule,
    /// The event class the candidate was tested against.
    pub event_class: String,
    /// Whether this rule's event class matches the candidate.
    pub matched: bool,
    /// Why the rule matched or did not (the transparency record).
    pub reason: String,
}

/// The report produced by applying one rule's action (or a dry-run plan).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicyReport {
    pub rule_id: String,
    pub event_class: String,
    pub mode: ApplyMode,
    /// State observed before the action (dry-run and commit).
    pub previous: String,
    /// The requested action.
    pub requested: SceneAction,
    /// State read back after a committed action; `None` during dry-run or when
    /// the channel was gated.
    pub actual: Option<String>,
    /// The declared rollback for this rule.
    pub rollback: Option<SceneAction>,
}

/// One entry in the local scene-policy tracking ledger.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicyLedgerEntry {
    pub rule_id: String,
    pub applied_at_unix_ms: u64,
    pub event_class: String,
    pub action: SceneAction,
    pub rollback: Option<SceneAction>,
    /// "applied" when the action committed; "unavailable" when the channel was
    /// gated and no write was attempted.
    pub status: String,
}

/// The local scene-policy tracking ledger (`scene track`).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicyLedger {
    pub entries: Vec<ScenePolicyLedgerEntry>,
}

fn require_text(value: &str, max: usize, label: &str) -> Result<()> {
    if value.is_empty() || value.len() > max || value.contains('\0') {
        return Err(LctrlError::InvalidArgument {
            detail: format!("{label} is empty, oversized, or contains NUL"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_validation_rejects_out_of_envelope_refresh() {
        assert!(SceneAction::SetPanelRefresh { hz: 60 }.validate().is_ok());
        assert!(SceneAction::SetPanelRefresh { hz: 240 }.validate().is_ok());
        assert!(SceneAction::SetPanelRefresh { hz: 29 }.validate().is_err());
        assert!(SceneAction::SetPanelRefresh { hz: 241 }.validate().is_err());
    }

    #[test]
    fn rule_requires_a_documented_event_class() {
        let action = SceneAction::SetFanMode { mode: FanMode::Silent };
        let rule = ScenePolicyRule::new(
            "scene.fan.silent".into(),
            "Silent fan scene".into(),
            "LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT".into(),
            action,
            Some(SceneAction::SetFanMode { mode: FanMode::Standard }),
            "Fires on the smart-fan-mode event and requests the silent fan mode.".into(),
        )
        .expect("well-formed rule");
        assert_eq!(rule.event_class, "LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT");
        // An empty event class is rejected.
        assert!(ScenePolicyRule::new(
            "x".into(),
            "t".into(),
            "".into(),
            SceneAction::SetFanMode { mode: FanMode::Silent },
            None,
            "d".into(),
        )
        .is_err());
    }
}
