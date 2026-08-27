use lctrl_core::{ApplyMode, Availability, CapabilitySet, HardwareInfo, Platform, Result};

use crate::model::{
    Goal, ResolvedProfile, TunePlan, TuneSetting, TuningTarget, UnavailableTarget, invalid,
};
pub struct Planner;

impl Planner {
    /// Compile semantic profile goals into an ordered plan. This function is
    /// deliberately pure: it performs no channel lookup beyond the supplied
    /// capability snapshot and never calls an OS or HAL API.
    ///
    /// `DryRun` omits unavailable targets and reports them in `skipped`.
    /// `Commit` is strict and returns an error if any requested target is not
    /// advertised. Neither mode substitutes another target or channel.
    pub fn compile(
        profile: &ResolvedProfile,
        platform: Platform,
        _hardware: &HardwareInfo,
        capabilities: &CapabilitySet,
        mode: ApplyMode,
    ) -> Result<TunePlan> {
        let settings = settings_in_order(&profile.goal);
        let mut writes = Vec::with_capacity(settings.len());
        let mut skipped = Vec::new();
        let mut warnings = Vec::new();

        for setting in settings {
            let target = setting.target();
            if let Some(detail) = unavailable_detail(platform, target, capabilities) {
                skipped.push(UnavailableTarget { target, detail });
            } else {
                if let Some(capability) = capabilities.get(target.as_str()) {
                    if capability.availability == Availability::Limited {
                        if let Some(detail) = &capability.detail {
                            warnings.push(format!("{} is limited: {detail}", target.as_str()));
                        }
                    }
                }
                writes.push(setting);
            }
        }

        if mode == ApplyMode::Commit && !skipped.is_empty() {
            let targets = skipped
                .iter()
                .map(|target| target.target.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(invalid(format!(
                "profile {:?} has unavailable tuning targets: {targets}",
                profile.name
            )));
        }

        let snapshot_targets = writes.iter().map(TuneSetting::target).collect();
        Ok(TunePlan {
            profile: profile.name.clone(),
            mode,
            snapshot_targets,
            writes,
            skipped,
            warnings,
        })
    }
}

fn unavailable_detail(
    platform: Platform,
    target: TuningTarget,
    capabilities: &CapabilitySet,
) -> Option<String> {
    // The clean-room corrections make Windows raw MSR/RAPL unavailable. A
    // capability record cannot turn that specifically forbidden channel back
    // into a write; no semantic substitution is attempted.
    if platform == Platform::Windows
        && matches!(
            target,
            TuningTarget::Pl1 | TuningTarget::Pl2 | TuningTarget::Tau
        )
    {
        return Some("Windows raw MSR/RAPL power-limit channel is unavailable".into());
    }
    match capabilities.get(target.as_str()) {
        Some(capability) if capability.availability != Availability::Unavailable => None,
        Some(capability) => Some(
            capability
                .detail
                .clone()
                .unwrap_or_else(|| format!("capability {} is unavailable", target.as_str())),
        ),
        None => Some(format!("capability {} is not advertised", target.as_str())),
    }
}

fn settings_in_order(goal: &Goal) -> Vec<TuneSetting> {
    let mut settings = Vec::new();
    if let Some(value) = goal.ec_mode {
        settings.push(TuneSetting::EcMode(value));
    }
    if let Some(value) = goal.pl1_w {
        settings.push(TuneSetting::Pl1(value));
    }
    if let Some(value) = goal.pl2_w {
        settings.push(TuneSetting::Pl2(value));
    }
    if let Some(value) = goal.tau_s {
        settings.push(TuneSetting::Tau(value));
    }
    if let Some(value) = goal.epp {
        settings.push(TuneSetting::Epp(value));
    }
    if let Some(value) = goal.turbo {
        settings.push(TuneSetting::Turbo(value));
    }
    if let Some(value) = goal.fan_mode {
        settings.push(TuneSetting::FanMode(value));
    }
    if let Some(value) = goal.panel_hz {
        settings.push(TuneSetting::PanelRefresh(value));
    }
    if let Some(value) = goal.dgpu {
        settings.push(TuneSetting::Dgpu(value));
    }
    if let Some(value) = goal.charge_mode {
        settings.push(TuneSetting::ChargeMode(value));
    }
    if let Some(value) = goal.backlight {
        settings.push(TuneSetting::Backlight(value));
    }
    settings
}
