use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use lctrl_core::Result;

use crate::model::{
    BackgroundGoal, BackgroundPriority, Constraints, DgpuMode, EcMode, Epp, Fallback,
    FallbackTarget, FanMode, Goal, PanelRefresh, ProfileDocument, ProfileName, ProfileOrigin,
    ResolvedProfile, TimeRange, Trigger, invalid,
};

#[derive(Clone, Debug)]
pub struct ProfileCatalog {
    profiles: BTreeMap<ProfileName, ResolvedProfile>,
}

impl ProfileCatalog {
    /// Merge the three documented profile sources (`builtin < system < user`),
    /// then resolve all inheritance and fallback references.
    pub fn from_layers<B, S, U>(builtins: B, system: S, user: U) -> Result<Self>
    where
        B: IntoIterator<Item = ProfileDocument>,
        S: IntoIterator<Item = ProfileDocument>,
        U: IntoIterator<Item = ProfileDocument>,
    {
        let mut selected: BTreeMap<ProfileName, (ProfileOrigin, ProfileDocument)> = BTreeMap::new();
        insert_layer(&mut selected, builtins, ProfileOrigin::Builtin)?;
        insert_layer(&mut selected, system, ProfileOrigin::System)?;
        insert_layer(&mut selected, user, ProfileOrigin::User)?;

        let documents: BTreeMap<ProfileName, ProfileDocument> = selected
            .into_iter()
            .map(|(name, (_, document))| (name, document))
            .collect();
        let mut memo = HashMap::new();
        let mut visiting = Vec::new();
        let names: Vec<ProfileName> = documents.keys().cloned().collect();
        for name in &names {
            resolve_one(name, &documents, &mut memo, &mut visiting)?;
        }
        validate_fallbacks(&memo)?;

        let profiles = memo.into_iter().collect();
        Ok(Self { profiles })
    }

    /// The five documented built-in recipes. Values that the clean-room
    /// corrections make unavailable (balanced RAPL writes and plugged-max SKU
    /// reads) are deliberately absent rather than represented by fake values.
    pub fn builtins() -> Result<Self> {
        let silent = builtin_silent_library()?;
        let long = builtin_long_battery()?;
        let balanced = builtin_balanced()?;
        let performance = builtin_performance()?;
        let plugged = builtin_plugged_max()?;
        Self::from_layers([silent, long, balanced, performance, plugged], [], [])
    }

    #[must_use]
    pub fn get(&self, name: impl AsRef<str>) -> Option<&ResolvedProfile> {
        self.profiles.get(name.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResolvedProfile> {
        self.profiles.values()
    }

    /// Return every profile in the deterministic winner order. Trigger class
    /// is primary; priority and lexical name break ties.
    pub fn ranked(&self) -> Vec<&ResolvedProfile> {
        let mut profiles: Vec<&ResolvedProfile> = self.profiles.values().collect();
        profiles.sort_by(|left, right| {
            right
                .trigger_class()
                .cmp(&left.trigger_class())
                .then_with(|| right.priority.cmp(&left.priority))
                .then_with(|| left.name.cmp(&right.name))
        });
        profiles
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

fn insert_layer<I>(
    selected: &mut BTreeMap<ProfileName, (ProfileOrigin, ProfileDocument)>,
    documents: I,
    origin: ProfileOrigin,
) -> Result<()>
where
    I: IntoIterator<Item = ProfileDocument>,
{
    let mut seen = BTreeSet::new();
    for mut document in documents {
        if document.schema != crate::model::PROFILE_SCHEMA_V1 {
            return Err(invalid(format!(
                "unsupported profile schema {}; expected {}",
                document.schema,
                crate::model::PROFILE_SCHEMA_V1
            )));
        }
        let name = document.profile.name.clone();
        if !seen.insert(name.clone()) {
            return Err(invalid(format!(
                "duplicate profile {name:?} in {} source",
                origin_name(origin)
            )));
        }
        document.origin = origin;
        match selected.get(&name) {
            Some((existing, _)) if existing.precedence() > origin.precedence() => {}
            _ => {
                selected.insert(name, (origin, document));
            }
        }
    }
    Ok(())
}

fn origin_name(origin: ProfileOrigin) -> &'static str {
    match origin {
        ProfileOrigin::Builtin => "builtin",
        ProfileOrigin::System => "system",
        ProfileOrigin::User => "user",
    }
}

fn resolve_one(
    name: &ProfileName,
    documents: &BTreeMap<ProfileName, ProfileDocument>,
    memo: &mut HashMap<ProfileName, ResolvedProfile>,
    visiting: &mut Vec<ProfileName>,
) -> Result<ResolvedProfile> {
    if let Some(profile) = memo.get(name) {
        return Ok(profile.clone());
    }
    if let Some(index) = visiting.iter().position(|current| current == name) {
        let mut cycle: Vec<String> = visiting[index..].iter().map(ToString::to_string).collect();
        cycle.push(name.to_string());
        return Err(invalid(format!(
            "profile inheritance cycle: {}",
            cycle.join(" -> ")
        )));
    }
    let document = documents
        .get(name)
        .ok_or_else(|| invalid(format!("profile {name:?} is not defined")))?;
    visiting.push(name.clone());
    let base = document
        .profile
        .inherits
        .as_ref()
        .map(|parent| resolve_one(parent, documents, memo, visiting))
        .transpose()?;
    let mut resolved = base.unwrap_or_else(|| ResolvedProfile {
        name: document.profile.name.clone(),
        description: None,
        priority: 0,
        goal: Goal::default(),
        triggers: Vec::new(),
        constraints: Constraints::default(),
        fallback: Fallback::default(),
        origin: document.origin,
    });

    resolved.name = document.profile.name.clone();
    resolved.origin = document.origin;
    if document.profile.description_explicit {
        resolved.description = document.profile.description.clone();
    }
    if document.profile.priority_explicit {
        resolved.priority = document.profile.priority;
    }
    merge_goal(&mut resolved.goal, &document.goal);
    if let Some(triggers) = &document.triggers {
        resolved.triggers = triggers.clone();
    }
    if let Some(constraints) = &document.constraints {
        merge_constraints(&mut resolved.constraints, constraints);
    }
    if let Some(fallback) = &document.fallback {
        merge_fallback(&mut resolved.fallback, fallback);
    }
    validate_power_limits(&resolved)?;

    visiting.pop();
    memo.insert(name.clone(), resolved.clone());
    Ok(resolved)
}

fn merge_goal(destination: &mut Goal, patch: &Goal) {
    if patch.ec_mode.is_some() {
        destination.ec_mode = patch.ec_mode;
    }
    if patch.pl1_w.is_some() {
        destination.pl1_w = patch.pl1_w;
    }
    if patch.pl2_w.is_some() {
        destination.pl2_w = patch.pl2_w;
    }
    if patch.tau_s.is_some() {
        destination.tau_s = patch.tau_s;
    }
    if patch.epp.is_some() {
        destination.epp = patch.epp;
    }
    if patch.turbo.is_some() {
        destination.turbo = patch.turbo;
    }
    if patch.fan_mode.is_some() {
        destination.fan_mode = patch.fan_mode;
    }
    if patch.panel_hz.is_some() {
        destination.panel_hz = patch.panel_hz;
    }
    if patch.dgpu.is_some() {
        destination.dgpu = patch.dgpu;
    }
    if patch.charge_mode.is_some() {
        destination.charge_mode = patch.charge_mode;
    }
    if patch.backlight.is_some() {
        destination.backlight = patch.backlight;
    }
    if let Some(background) = &patch.background {
        let destination_background = destination
            .background
            .get_or_insert_with(BackgroundGoal::default);
        if background.workset_trim.is_some() {
            destination_background.workset_trim = background.workset_trim;
        }
        if background.priority.is_some() {
            destination_background.priority = background.priority;
        }
    }
}

fn merge_constraints(destination: &mut Constraints, patch: &Constraints) {
    if patch.temp_max_c.is_some() {
        destination.temp_max_c = patch.temp_max_c;
    }
    if patch.fan_rpm_max.is_some() {
        destination.fan_rpm_max = patch.fan_rpm_max;
    }
    if patch.battery_only.is_some() {
        destination.battery_only = patch.battery_only;
    }
    if patch.min_dwell_s.is_some() {
        destination.min_dwell_s = patch.min_dwell_s;
    }
}

fn merge_fallback(destination: &mut Fallback, patch: &Fallback) {
    if patch.on_temp_exceed.is_some() {
        destination.on_temp_exceed = patch.on_temp_exceed.clone();
    }
    if patch.on_conflict.is_some() {
        destination.on_conflict = patch.on_conflict;
    }
    if patch.max_reapply.is_some() {
        destination.max_reapply = patch.max_reapply;
    }
}

fn validate_power_limits(profile: &ResolvedProfile) -> Result<()> {
    if let (Some(pl1), Some(pl2)) = (profile.goal.pl1_w, profile.goal.pl2_w) {
        if pl1 > pl2 {
            return Err(invalid(format!(
                "profile {:?} violates PL1 <= PL2: PL1={pl1}, PL2={pl2}",
                profile.name
            )));
        }
    }
    Ok(())
}

fn validate_fallbacks(profiles: &HashMap<ProfileName, ResolvedProfile>) -> Result<()> {
    for (name, profile) in profiles {
        if let Some(FallbackTarget::Profile(target)) = &profile.fallback.on_temp_exceed {
            if target == name {
                return Err(invalid(format!(
                    "profile {name:?} cannot fall back to itself"
                )));
            }
            if !profiles.contains_key(target) {
                return Err(invalid(format!(
                    "profile {name:?} fallback references missing profile {target:?}"
                )));
            }
        }
    }

    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for name in profiles.keys() {
        validate_fallback_chain(name, profiles, &mut visiting, &mut done)?;
    }
    Ok(())
}

fn validate_fallback_chain(
    name: &ProfileName,
    profiles: &HashMap<ProfileName, ResolvedProfile>,
    visiting: &mut HashSet<ProfileName>,
    done: &mut HashSet<ProfileName>,
) -> Result<()> {
    if done.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.clone()) {
        return Err(invalid(format!("fallback cycle includes profile {name:?}")));
    }
    if let Some(profile) = profiles.get(name) {
        if let Some(FallbackTarget::Profile(target)) = &profile.fallback.on_temp_exceed {
            validate_fallback_chain(target, profiles, visiting, done)?;
        }
    }
    visiting.remove(name);
    done.insert(name.clone());
    Ok(())
}

fn builtin_document(name: &str) -> Result<ProfileDocument> {
    ProfileDocument::new(name, ProfileOrigin::Builtin)
}

fn builtin_silent_library() -> Result<ProfileDocument> {
    let mut document = builtin_document("silent-library")?;
    document.profile.priority = 60;
    document.profile.priority_explicit = true;
    document.goal = Goal {
        ec_mode: Some(EcMode::PowerSave),
        pl1_w: Some(9),
        pl2_w: Some(15),
        tau_s: Some(28),
        epp: Some(Epp::BalancePower),
        turbo: Some(false),
        fan_mode: Some(FanMode::Smart),
        panel_hz: Some(PanelRefresh::Hz(60)),
        dgpu: None,
        charge_mode: Some(lctrl_core::ChargeMode::Conservation),
        backlight: Some(1),
        background: Some(BackgroundGoal {
            workset_trim: Some(true),
            priority: Some(BackgroundPriority::Low),
        }),
    };
    document.triggers = Some(vec![Trigger::TimeRange(TimeRange::parse(
        "08:00", "22:00",
    )?)]);
    document.constraints = Some(Constraints {
        temp_max_c: Some(80),
        ..Constraints::default()
    });
    document.fallback = Some(Fallback {
        on_temp_exceed: Some(FallbackTarget::RestoreFactory),
        ..Fallback::default()
    });
    Ok(document)
}

fn builtin_long_battery() -> Result<ProfileDocument> {
    let mut document = builtin_document("long-battery")?;
    document.profile.priority = 50;
    document.profile.priority_explicit = true;
    document.goal = Goal {
        ec_mode: Some(EcMode::PowerSave),
        pl1_w: Some(12),
        pl2_w: Some(20),
        tau_s: Some(28),
        epp: Some(Epp::BalancePower),
        turbo: Some(true),
        fan_mode: Some(FanMode::Smart),
        panel_hz: Some(PanelRefresh::Hz(60)),
        dgpu: Some(DgpuMode::IgpPriority),
        charge_mode: Some(lctrl_core::ChargeMode::Conservation),
        backlight: None,
        background: None,
    };
    document.triggers = Some(vec![
        Trigger::OnBattery,
        Trigger::BatteryBelow { percent: 30 },
    ]);
    document.fallback = Some(Fallback {
        on_temp_exceed: Some(FallbackTarget::Profile(ProfileName::new("silent-library")?)),
        ..Fallback::default()
    });
    Ok(document)
}

fn builtin_balanced() -> Result<ProfileDocument> {
    let mut document = builtin_document("balanced")?;
    document.goal = Goal {
        ec_mode: Some(EcMode::Smart),
        epp: Some(Epp::BalancePerformance),
        fan_mode: Some(FanMode::Smart),
        panel_hz: Some(PanelRefresh::Adaptive),
        ..Goal::default()
    };
    Ok(document)
}

fn builtin_performance() -> Result<ProfileDocument> {
    let mut document = builtin_document("performance")?;
    document.goal = Goal {
        ec_mode: Some(EcMode::Beast),
        pl1_w: Some(25),
        pl2_w: Some(45),
        epp: Some(Epp::Performance),
        fan_mode: Some(FanMode::Performance),
        panel_hz: Some(PanelRefresh::Hz(120)),
        ..Goal::default()
    };
    Ok(document)
}

fn builtin_plugged_max() -> Result<ProfileDocument> {
    let mut document = builtin_document("plugged-max")?;
    document.goal = Goal {
        ec_mode: Some(EcMode::Beast),
        epp: Some(Epp::Performance),
        // Factory SKU PL and max tau are intentionally not fabricated. They
        // require an unavailable Windows read channel and have no DSL literal.
        ..Goal::default()
    };
    Ok(document)
}
