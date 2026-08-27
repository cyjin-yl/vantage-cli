use lctrl_core::{ChargeMode, Result};
use serde::Deserialize;

use crate::model::{
    BackgroundGoal, BackgroundPriority, ConflictAction, Constraints, DgpuMode, EcMode, Epp,
    Fallback, FallbackTarget, FanMode, Goal, PanelRefresh, ProfileDocument, ProfileMetadata,
    ProfileName, ProfileOrigin, TimeRange, Trigger, invalid,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProfileDocument {
    #[serde(default = "schema_v1")]
    schema: u32,
    profile: WireProfile,
    #[serde(default)]
    goal: WireGoal,
    #[serde(default)]
    trigger: Option<Vec<WireTrigger>>,
    #[serde(default)]
    constraints: Option<WireConstraints>,
    #[serde(default)]
    fallback: Option<WireFallback>,
}

const fn schema_v1() -> u32 {
    crate::model::PROFILE_SCHEMA_V1
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProfile {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<i64>,
    #[serde(default)]
    inherits: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireGoal {
    #[serde(default)]
    ec_mode: Option<i64>,
    #[serde(default)]
    pl1_w: Option<i64>,
    #[serde(default)]
    pl2_w: Option<i64>,
    #[serde(default)]
    tau_s: Option<i64>,
    #[serde(default)]
    epp: Option<WireEpp>,
    #[serde(default)]
    turbo: Option<bool>,
    #[serde(default)]
    fan_mode: Option<String>,
    #[serde(default)]
    panel_hz: Option<WirePanelRefresh>,
    #[serde(default)]
    dgpu: Option<String>,
    #[serde(default)]
    charge_mode: Option<String>,
    #[serde(default)]
    backlight: Option<i64>,
    #[serde(default)]
    background: Option<WireBackground>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum WireEpp {
    Text(String),
    Raw(i64),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum WirePanelRefresh {
    Text(String),
    Hertz(i64),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBackground {
    #[serde(default)]
    workset_trim: Option<bool>,
    #[serde(default)]
    priority: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireConstraints {
    #[serde(default)]
    temp_max_c: Option<i64>,
    #[serde(default)]
    fan_rpm_max: Option<i64>,
    #[serde(default)]
    battery_only: Option<bool>,
    #[serde(default)]
    min_dwell_s: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFallback {
    #[serde(default)]
    on_temp_exceed: Option<String>,
    #[serde(default)]
    on_conflict: Option<String>,
    #[serde(default)]
    max_reapply: Option<i64>,
}

/// A strict raw trigger envelope. Validation below rejects fields that do not
/// belong to its selected `type`; deny_unknown_fields catches every typo.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTrigger {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    names: Option<Vec<String>>,
    #[serde(default)]
    paths: Option<Vec<String>>,
    #[serde(default)]
    celsius: Option<i64>,
    #[serde(default)]
    watts: Option<i64>,
    #[serde(default)]
    hysteresis: Option<i64>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    percent: Option<i64>,
}

pub fn parse_profile_toml(text: &str, origin: ProfileOrigin) -> Result<ProfileDocument> {
    let wire: WireProfileDocument =
        toml::from_str(text).map_err(|error| invalid(format!("invalid profile TOML: {error}")))?;
    if wire.schema != crate::model::PROFILE_SCHEMA_V1 {
        return Err(invalid(format!(
            "unsupported profile schema {}; expected {}",
            wire.schema,
            crate::model::PROFILE_SCHEMA_V1
        )));
    }

    let name = ProfileName::new(wire.profile.name)?;
    let priority = match wire.profile.priority {
        Some(value) => i32::try_from(value)
            .map_err(|_| invalid(format!("profile priority is outside i32: {value}")))?,
        None => 0,
    };
    let inherits = wire.profile.inherits.map(ProfileName::new).transpose()?;
    let profile = ProfileMetadata {
        name,
        description_explicit: wire.profile.description.is_some(),
        description: wire.profile.description,
        priority_explicit: wire.profile.priority.is_some(),
        priority,
        inherits,
    };

    let goal = parse_goal(wire.goal)?;
    let triggers = wire
        .trigger
        .map(|triggers| triggers.into_iter().map(parse_trigger).collect())
        .transpose()?;
    let constraints = wire.constraints.map(parse_constraints).transpose()?;
    let fallback = wire.fallback.map(parse_fallback).transpose()?;

    Ok(ProfileDocument {
        schema: wire.schema,
        profile,
        goal,
        triggers,
        constraints,
        fallback,
        origin,
    })
}

fn parse_goal(wire: WireGoal) -> Result<Goal> {
    let ec_mode = wire.ec_mode.map(EcMode::from_raw).transpose()?;
    let pl1_w = wire.pl1_w.map(parse_nonnegative_u32("pl1_w")).transpose()?;
    let pl2_w = wire.pl2_w.map(parse_nonnegative_u32("pl2_w")).transpose()?;
    let tau_s = wire.tau_s.map(parse_nonnegative_u32("tau_s")).transpose()?;
    let epp = wire.epp.map(parse_epp).transpose()?;
    let fan_mode = wire.fan_mode.map(parse_fan_mode).transpose()?;
    let panel_hz = wire.panel_hz.map(parse_panel_refresh).transpose()?;
    let dgpu = wire
        .dgpu
        .map(|value| DgpuMode::from_text(&value))
        .transpose()?;
    let charge_mode = wire
        .charge_mode
        .map(|value| parse_charge_mode(&value))
        .transpose()?;
    let backlight = wire
        .backlight
        .map(|value| parse_u8(value, "backlight"))
        .transpose()?;
    let background = wire.background.map(parse_background).transpose()?;

    Ok(Goal {
        ec_mode,
        pl1_w,
        pl2_w,
        tau_s,
        epp,
        turbo: wire.turbo,
        fan_mode,
        panel_hz,
        dgpu,
        charge_mode,
        backlight,
        background,
    })
}

fn parse_nonnegative_u32(name: &'static str) -> impl FnOnce(i64) -> Result<u32> {
    move |value| {
        u32::try_from(value)
            .map_err(|_| invalid(format!("{name} must be a nonnegative u32; got {value}")))
    }
}

fn parse_u8(value: i64, name: &'static str) -> Result<u8> {
    u8::try_from(value)
        .map_err(|_| invalid(format!("{name} must be between 0 and 255; got {value}")))
}

fn parse_epp(value: WireEpp) -> Result<Epp> {
    match value {
        WireEpp::Text(value) => Epp::from_text(&value),
        WireEpp::Raw(value) => Epp::from_raw(value),
    }
}

fn parse_panel_refresh(value: WirePanelRefresh) -> Result<PanelRefresh> {
    match value {
        WirePanelRefresh::Hertz(value) => PanelRefresh::hz(value),
        WirePanelRefresh::Text(value) => match value.as_str() {
            "auto" | "adaptive" => Ok(PanelRefresh::Adaptive),
            other => Err(invalid(format!(
                "unknown panel_hz mode {other:?}; expected auto or adaptive, or a positive integer hertz value"
            ))),
        },
    }
}

fn parse_fan_mode(value: String) -> Result<FanMode> {
    match value.as_str() {
        "auto" => Ok(FanMode::Auto),
        "manual" => Ok(FanMode::Manual),
        "fullspeed" => Ok(FanMode::Fullspeed),
        "smart" => Ok(FanMode::Smart),
        "off" => Ok(FanMode::Off),
        "performance" => Ok(FanMode::Performance),
        other => Err(invalid(format!(
            "unknown fan_mode {other:?}; expected auto, manual, fullspeed, smart, off, or performance"
        ))),
    }
}

fn parse_charge_mode(value: &str) -> Result<ChargeMode> {
    match value {
        "normal" => Ok(ChargeMode::Normal),
        "conservation" => Ok(ChargeMode::Conservation),
        "rapid" => Ok(ChargeMode::Rapid),
        other => Err(invalid(format!(
            "unknown charge_mode {other:?}; expected normal, conservation, or rapid"
        ))),
    }
}

fn parse_background(wire: WireBackground) -> Result<BackgroundGoal> {
    let priority = wire
        .priority
        .map(|value| BackgroundPriority::from_text(&value))
        .transpose()?;
    Ok(BackgroundGoal {
        workset_trim: wire.workset_trim,
        priority,
    })
}

fn parse_constraints(wire: WireConstraints) -> Result<Constraints> {
    Ok(Constraints {
        temp_max_c: wire
            .temp_max_c
            .map(|value| parse_nonnegative_u32("temp_max_c")(value))
            .transpose()?,
        fan_rpm_max: wire
            .fan_rpm_max
            .map(|value| parse_nonnegative_u32("fan_rpm_max")(value))
            .transpose()?,
        battery_only: wire.battery_only,
        min_dwell_s: wire
            .min_dwell_s
            .map(|value| parse_nonnegative_u32("min_dwell_s")(value))
            .transpose()?,
    })
}

fn parse_fallback(wire: WireFallback) -> Result<Fallback> {
    let on_temp_exceed = wire
        .on_temp_exceed
        .map(|value| {
            if value == "restore-factory" {
                Ok(FallbackTarget::RestoreFactory)
            } else {
                ProfileName::new(value).map(FallbackTarget::Profile)
            }
        })
        .transpose()?;
    let on_conflict = wire
        .on_conflict
        .map(|value| match value.as_str() {
            "restore-factory" => Ok(ConflictAction::RestoreFactory),
            "give-up" => Ok(ConflictAction::GiveUp),
            other => Err(invalid(format!(
                "unknown on_conflict {other:?}; expected restore-factory or give-up"
            ))),
        })
        .transpose()?;
    let max_reapply = wire
        .max_reapply
        .map(|value| parse_nonnegative_u32("max_reapply")(value))
        .transpose()?;
    Ok(Fallback {
        on_temp_exceed,
        on_conflict,
        max_reapply,
    })
}

fn parse_trigger(wire: WireTrigger) -> Result<Trigger> {
    let WireTrigger {
        kind,
        names,
        paths,
        celsius,
        watts,
        hysteresis,
        from,
        to,
        percent,
    } = wire;
    match kind.as_str() {
        "on_ac" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                celsius.is_some(),
                watts.is_some(),
                hysteresis.is_some(),
                from.is_some(),
                to.is_some(),
                percent.is_some(),
            )?;
            Ok(Trigger::OnAc)
        }
        "on_battery" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                celsius.is_some(),
                watts.is_some(),
                hysteresis.is_some(),
                from.is_some(),
                to.is_some(),
                percent.is_some(),
            )?;
            Ok(Trigger::OnBattery)
        }
        "process_match" => {
            reject_parameters(
                &kind,
                false,
                false,
                celsius.is_some(),
                watts.is_some(),
                hysteresis.is_some(),
                from.is_some(),
                to.is_some(),
                percent.is_some(),
            )?;
            let names = names.unwrap_or_default();
            let paths = paths.unwrap_or_default();
            if names.is_empty() && paths.is_empty() {
                return Err(invalid(
                    "process_match requires a nonempty names or paths list",
                ));
            }
            if names.iter().any(|value| value.trim().is_empty())
                || paths.iter().any(|value| value.trim().is_empty())
            {
                return Err(invalid("process_match names and paths must not be empty"));
            }
            if !names.is_empty() && !paths.is_empty() {
                return Err(invalid(
                    "process_match must specify names or paths, not both",
                ));
            }
            Ok(Trigger::ProcessMatch { names, paths })
        }
        "temp_above" | "temp_below" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                false,
                watts.is_some(),
                false,
                from.is_some(),
                to.is_some(),
                percent.is_some(),
            )?;
            let celsius = parse_required_nonnegative(celsius, "celsius", &kind)?;
            let hysteresis = parse_optional_hysteresis(hysteresis)?;
            if kind == "temp_above" {
                Ok(Trigger::TempAbove {
                    celsius,
                    hysteresis,
                })
            } else {
                Ok(Trigger::TempBelow {
                    celsius,
                    hysteresis,
                })
            }
        }
        "power_above" | "power_below" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                celsius.is_some(),
                false,
                false,
                from.is_some(),
                to.is_some(),
                percent.is_some(),
            )?;
            let watts = parse_required_nonnegative(watts, "watts", &kind)?;
            let hysteresis = parse_optional_hysteresis(hysteresis)?;
            if kind == "power_above" {
                Ok(Trigger::PowerAbove { watts, hysteresis })
            } else {
                Ok(Trigger::PowerBelow { watts, hysteresis })
            }
        }
        "time_range" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                celsius.is_some(),
                watts.is_some(),
                hysteresis.is_some(),
                false,
                false,
                percent.is_some(),
            )?;
            let from = from.ok_or_else(|| invalid("time_range requires from"))?;
            let to = to.ok_or_else(|| invalid("time_range requires to"))?;
            Ok(Trigger::TimeRange(TimeRange::parse(&from, &to)?))
        }
        "battery_below" => {
            reject_parameters(
                &kind,
                names.is_some(),
                paths.is_some(),
                celsius.is_some(),
                watts.is_some(),
                hysteresis.is_some(),
                from.is_some(),
                to.is_some(),
                false,
            )?;
            let percent = parse_required_nonnegative(percent, "percent", &kind)?;
            let percent = u8::try_from(percent).map_err(|_| {
                invalid(format!(
                    "battery_below percent must be 0..=100; got {percent}"
                ))
            })?;
            if percent > 100 {
                return Err(invalid(format!(
                    "battery_below percent must be 0..=100; got {percent}"
                )));
            }
            Ok(Trigger::BatteryBelow { percent })
        }
        other => Err(invalid(format!(
            "unknown trigger type {other:?}; expected on_ac, on_battery, process_match, temp_above, temp_below, power_above, power_below, time_range, or battery_below"
        ))),
    }
}

fn reject_parameters(
    kind: &str,
    names: bool,
    paths: bool,
    celsius: bool,
    watts: bool,
    hysteresis: bool,
    from: bool,
    to: bool,
    percent: bool,
) -> Result<()> {
    if names || paths || celsius || watts || hysteresis || from || to || percent {
        Err(invalid(format!(
            "trigger type {kind} has incompatible parameters"
        )))
    } else {
        Ok(())
    }
}

fn parse_required_nonnegative(value: Option<i64>, field: &str, kind: &str) -> Result<u32> {
    let value = value.ok_or_else(|| invalid(format!("trigger type {kind} requires {field}")))?;
    u32::try_from(value)
        .map_err(|_| invalid(format!("trigger {field} must be nonnegative; got {value}")))
}

fn parse_optional_hysteresis(value: Option<i64>) -> Result<u32> {
    match value {
        Some(value) => u32::try_from(value).map_err(|_| {
            invalid(format!(
                "trigger hysteresis must be nonnegative; got {value}"
            ))
        }),
        None => Ok(3),
    }
}
