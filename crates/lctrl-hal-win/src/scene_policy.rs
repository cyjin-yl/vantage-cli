//! Windows scene-policy backend: a transparent ruleset of documented-event →
//! verified-action rules.
//!
//! Every rule is declared in [`Self::ruleset`] and visible through `scene list`.
//! Each rule binds one documented Lenovo WMI event class to an allowlisted action
//! and dispatches it through the same verified control channel the CLI uses, with
//! a readback and a dry-run plan. The local ledger is persisted as JSON under the
//! user profile and read back by `scene track`.
//!
//! No vendor AI/ML scene model is admitted. An action whose write channel is gated
//! (e.g. fan/performance on 21VG firmware) reports `unavailable` rather than faking
//! a scene change.

use std::{
    env,
    fs,
    io::Write as _,
    path::PathBuf,
    sync::Mutex,
};

use lctrl_core::{
    ApplyMode, FanMode, LctrlError, PanelRefreshRequest, PerformanceMode, PowerMutation, Result,
    SceneAction, ScenePolicyLedger, ScenePolicyLedgerEntry, ScenePolicyMatch, ScenePolicyReport,
    ScenePolicyRule,
};
use lctrl_hal::{
    FanControl, PerformanceControl, PanelControl, PowerControl, ScenePolicyControl,
};
use crate::{
    NativeBatteryElectrical, NativeDisplayApi, NativeIoctl, NativePerformanceRegistry,
    NativePowerApi, NativeSystemPowerStatus, NativeWmi, UnverifiedChargeModeReader, WindowsBatteryP0,
    WindowsBatterySources, WindowsOsBattery, WindowsPanelController, WindowsPerformanceP0,
    WindowsPowerP0, WindowsThermal,
};

/// A transparent scene-policy backend over the Windows control channels.
///
/// The control objects (power scheme, panel refresh, fan mode, performance mode) are
/// constructed on demand from the native transports; all transports are unit types,
/// so no state needs to be held between rules.
pub struct NativeScenePolicy {
    ledger: Mutex<ScenePolicyLedger>,
}

impl Default for NativeScenePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeScenePolicy {
    #[must_use]
    pub fn new() -> Self {
        Self {
            ledger: Mutex::new(load_ledger()),
        }
    }

    /// The complete declared ruleset. Each rule binds one documented Lenovo WMI
    /// event class to one allowlisted action and its rollback.
    fn ruleset() -> Vec<ScenePolicyRule> {
        let mut rules = Vec::new();
        rules.push(
            ScenePolicyRule::new(
                "scene.gamezone.performance".into(),
                "Game Zone → Performance".into(),
                "LENOVO_GAMEZONE_START_EVENT".into(),
                SceneAction::SetPerformanceMode {
                    mode: PerformanceMode::Performance,
                },
                Some(SceneAction::SetPerformanceMode {
                    mode: PerformanceMode::Balanced,
                }),
                "Fires when a Game Zone session starts; requests the performance mode and reads back the performance state. Rollback returns to balanced.".into(),
            )
            .expect("well-formed rule"),
        );
        rules.push(
            ScenePolicyRule::new(
                "scene.gamezone.fan.fullspeed".into(),
                "Game Zone → Full-speed fan".into(),
                "LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT".into(),
                SceneAction::SetFanMode {
                    mode: FanMode::Performance,
                },
                Some(SceneAction::SetFanMode {
                    mode: FanMode::Standard,
                }),
                "Fires on the smart-fan-mode event; requests the full-speed fan mode and reads back the fan mode. Rollback returns to standard. Gated when the fan write channel is unavailable.".into(),
            )
            .expect("well-formed rule"),
        );
        rules.push(
            ScenePolicyRule::new(
                "scene.aidemo.power.silent".into(),
                "AI Demo → Silent power".into(),
                "LENOVO_AI_SCENARIO_TYPE_EVENT".into(),
                SceneAction::SetPerformanceMode {
                    mode: PerformanceMode::Quiet,
                },
                Some(SceneAction::SetPerformanceMode {
                    mode: PerformanceMode::Balanced,
                }),
                "Fires on an AI-scenario event; requests the quiet performance mode and reads back the performance state. Rollback returns to balanced.".into(),
            )
            .expect("well-formed rule"),
        );
        rules.push(
            ScenePolicyRule::new(
                "scene.panel.high-refresh".into(),
                "High-refresh panel scene".into(),
                "LENOVO_GAMEZONE_START_EVENT".into(),
                SceneAction::SetPanelRefresh { hz: 120 },
                Some(SceneAction::SetPanelRefresh { hz: 60 }),
                "Fires on a Game Zone start; requests a 120 Hz panel mode (validated by the display driver before commit) and reads back the current mode. Rollback returns to 60 Hz.".into(),
            )
            .expect("well-formed rule"),
        );
        rules
    }
}

impl ScenePolicyControl for NativeScenePolicy {
    fn rules(&self) -> Vec<ScenePolicyRule> {
        Self::ruleset()
    }

    fn match_event(&self, event_class: &str) -> Vec<ScenePolicyMatch> {
        Self::ruleset()
            .into_iter()
            .map(|rule| {
                let matched = rule.event_class == event_class;
                let reason = if matched {
                    format!("rule {} fires on documented event class {}", rule.id, event_class)
                } else {
                    format!(
                        "rule {} is bound to event class {}, not {}",
                        rule.id, rule.event_class, event_class
                    )
                };
                ScenePolicyMatch {
                    rule,
                    event_class: event_class.to_string(),
                    matched,
                    reason,
                }
            })
            .collect()
    }

    fn apply(
        &self,
        rule_id: &str,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        let rule = Self::ruleset()
            .into_iter()
            .find(|rule| rule.id == rule_id)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("unknown scene rule {rule_id}; run `scene list` for the declared ruleset"),
            })?;
        match &rule.action {
            SceneAction::SetPowerScheme { scheme } => {
                self.apply_power(scheme, &rule, event_class, mode)
            }
            SceneAction::SetPanelRefresh { hz } => self.apply_panel(*hz, &rule, event_class, mode),
            SceneAction::SetFanMode { mode: requested } => {
                self.apply_fan(*requested, &rule, event_class, mode)
            }
            SceneAction::SetPerformanceMode { mode: requested } => {
                self.apply_performance(*requested, &rule, event_class, mode)
            }
        }
    }

    fn track(&self) -> ScenePolicyLedger {
        self.ledger
            .lock()
            .map(|ledger| ledger.clone())
            .unwrap_or_default()
    }
}

impl NativeScenePolicy {
    fn report_dry_run(
        &self,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
        previous: String,
    ) -> ScenePolicyReport {
        ScenePolicyReport {
            rule_id: rule.id.clone(),
            event_class: event_class.to_string(),
            mode,
            previous,
            requested: rule.action.clone(),
            actual: None,
            rollback: rule.rollback.clone(),
        }
    }

    fn record_entry(&self, rule: &ScenePolicyRule, event_class: &str, status: &str) -> Result<()> {
        let entry = ScenePolicyLedgerEntry {
            rule_id: rule.id.clone(),
            applied_at_unix_ms: now_unix_ms(),
            event_class: event_class.to_string(),
            action: rule.action.clone(),
            rollback: rule.rollback.clone(),
            status: status.to_string(),
        };
        persist_ledger_entry(&self.ledger, entry)
    }

    fn apply_power(
        &self,
        scheme: &str,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        let power = WindowsPowerP0::new(NativePowerApi);
        let previous = power.active_power_scheme()?.name.clone();
        if mode == ApplyMode::DryRun {
            return Ok(self.report_dry_run(rule, event_class, mode, previous));
        }
        let schemes = power.power_schemes()?;
        let target = schemes
            .iter()
            .find(|s| s.name == scheme || s.id.as_str() == scheme)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("power scheme {scheme} was not enumerated on this machine"),
            })?;
        power.apply_power_mutation(PowerMutation::Activate(target.id.clone()), ApplyMode::Commit)?;
        let actual = power.active_power_scheme()?.name.clone();
        if actual != scheme {
            return Err(LctrlError::VerifyMismatch {
                requested: format!("power scheme {scheme}"),
                actual: format!("power scheme {actual}"),
            });
        }
        self.record_entry(rule, event_class, "applied")?;
        Ok(ScenePolicyReport {
            rule_id: rule.id.clone(),
            event_class: event_class.to_string(),
            mode,
            previous,
            requested: rule.action.clone(),
            actual: Some(actual),
            rollback: rule.rollback.clone(),
        })
    }

    fn apply_panel(
        &self,
        hz: u32,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        let panel = WindowsPanelController::new(NativeDisplayApi, NativeWmi);
        let request = PanelRefreshRequest::Fixed(hz as u16);
        let report = panel.set_refresh_rate(request, mode)?;
        let previous = report.previous().to_string();
        if mode == ApplyMode::DryRun {
            return Ok(self.report_dry_run(rule, event_class, mode, previous));
        }
        let actual = report.actual().cloned().ok_or_else(|| {
            LctrlError::VerifyMismatch {
                requested: format!("panel refresh {hz} Hz"),
                actual: "panel refresh readback was not returned".into(),
            }
        })?;
        self.record_entry(rule, event_class, "applied")?;
        Ok(ScenePolicyReport {
            rule_id: rule.id.clone(),
            event_class: event_class.to_string(),
            mode,
            previous,
            requested: rule.action.clone(),
            actual: Some(actual.to_string()),
            rollback: rule.rollback.clone(),
        })
    }

    fn apply_fan(
        &self,
        requested: FanMode,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        let vendor_battery = WindowsBatteryP0::new(NativeIoctl, UnverifiedChargeModeReader);
        let os_battery = WindowsOsBattery::new(NativeWmi, NativeSystemPowerStatus);
        let battery = WindowsBatterySources::new(vendor_battery, os_battery, NativeBatteryElectrical);
        let fan = WindowsThermal::new(NativeWmi, &battery);
        match fan.set_fan_mode(requested, mode) {
            Ok(report) => {
                let previous = report.previous().to_string();
                if mode == ApplyMode::DryRun {
                    return Ok(self.report_dry_run(rule, event_class, mode, previous));
                }
                let actual = report.actual().cloned().ok_or_else(|| {
                    LctrlError::VerifyMismatch {
                        requested: format!("fan mode {requested}"),
                        actual: "fan mode readback was not returned".into(),
                    }
                })?;
                self.record_entry(rule, event_class, "applied")?;
                Ok(ScenePolicyReport {
                    rule_id: rule.id.clone(),
                    event_class: event_class.to_string(),
                    mode,
                    previous,
                    requested: rule.action.clone(),
                    actual: Some(actual.to_string()),
                    rollback: rule.rollback.clone(),
                })
            }
            Err(error) => self.unavailable(rule, event_class, mode, error.to_string()),
        }
    }

    fn apply_performance(
        &self,
        requested: PerformanceMode,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
    ) -> Result<ScenePolicyReport> {
        let performance = WindowsPerformanceP0::new(NativePerformanceRegistry);
        match performance.set_performance_mode(requested, mode) {
            Ok(report) => {
                let previous = report.previous().to_string();
                if mode == ApplyMode::DryRun {
                    return Ok(self.report_dry_run(rule, event_class, mode, previous));
                }
                let actual = report.actual().cloned().ok_or_else(|| {
                    LctrlError::VerifyMismatch {
                        requested: format!("performance mode {requested}"),
                        actual: "performance mode readback was not returned".into(),
                    }
                })?;
                self.record_entry(rule, event_class, "applied")?;
                Ok(ScenePolicyReport {
                    rule_id: rule.id.clone(),
                    event_class: event_class.to_string(),
                    mode,
                    previous,
                    requested: rule.action.clone(),
                    actual: Some(actual.to_string()),
                    rollback: rule.rollback.clone(),
                })
            }
            Err(error) => self.unavailable(rule, event_class, mode, error.to_string()),
        }
    }

    /// A gated write channel reports `unavailable` and is recorded, so a dry-run
    /// of a scene whose action is unsupported is honest: no write was attempted
    /// and the rollback is still declared.
    fn unavailable(
        &self,
        rule: &ScenePolicyRule,
        event_class: &str,
        mode: ApplyMode,
        detail: String,
    ) -> Result<ScenePolicyReport> {
        if mode == ApplyMode::DryRun {
            return Ok(ScenePolicyReport {
                rule_id: rule.id.clone(),
                event_class: event_class.to_string(),
                mode,
                previous: format!("channel unavailable: {detail}"),
                requested: rule.action.clone(),
                actual: None,
                rollback: rule.rollback.clone(),
            });
        }
        self.record_entry(rule, event_class, "unavailable")?;
        Ok(ScenePolicyReport {
            rule_id: rule.id.clone(),
            event_class: event_class.to_string(),
            mode,
            previous: format!("channel unavailable: {detail}"),
            requested: rule.action.clone(),
            actual: None,
            rollback: rule.rollback.clone(),
        })
    }
}

fn load_ledger() -> ScenePolicyLedger {
    read_ledger_file().unwrap_or_default()
}

fn ledger_path() -> Option<PathBuf> {
    let base = env::var("USERPROFILE").ok()?;
    Some(PathBuf::from(base).join(".vantage-scene-ledger.json"))
}

fn read_ledger_file() -> Option<ScenePolicyLedger> {
    let path = ledger_path()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn persist_ledger_entry(
    ledger: &Mutex<ScenePolicyLedger>,
    entry: ScenePolicyLedgerEntry,
) -> Result<()> {
    let mut guard = ledger.lock().map_err(|_| LctrlError::InvalidArgument {
        detail: "scene ledger lock was poisoned".into(),
    })?;
    guard.entries.push(entry);
    let path = ledger_path().ok_or_else(|| LctrlError::InvalidArgument {
        detail: "USERPROFILE is unset; cannot persist the scene ledger".into(),
    })?;
    let text = serde_json::to_string_pretty(&*guard).map_err(|error| LctrlError::InvalidArgument {
        detail: format!("serializing scene ledger failed: {error}"),
    })?;
    write_all(&path, text.as_bytes())
}

fn write_all(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| LctrlError::InvalidArgument {
            detail: format!("opening scene ledger failed: {error}"),
        })?;
    file.write_all(bytes).and_then(|_| file.flush()).map_err(|error| {
        LctrlError::InvalidArgument {
            detail: format!("writing scene ledger failed: {error}"),
        }
    })
}

fn now_unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruleset_is_transparent_and_matches_only_documented_classes() {
        let rules = NativeScenePolicy::ruleset();
        assert!(rules.len() >= 4);
        // Every rule is bound to a documented event class.
        for rule in &rules {
            assert!(!rule.event_class.is_empty());
            assert!(!rule.id.is_empty());
        }
        let policy = NativeScenePolicy::new();
        // A documented class matches exactly its bound rules.
        let matched = policy
            .match_event("LENOVO_GAMEZONE_START_EVENT")
            .iter()
            .filter(|m| m.matched)
            .count();
        assert_eq!(matched, 2); // performance + panel high-refresh both fire on game-zone start
        // An unknown class matches nothing.
        assert!(policy
            .match_event("NOT_A_LENOVO_CLASS")
            .iter()
            .all(|m| !m.matched));
    }
}
