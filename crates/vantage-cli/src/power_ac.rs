//! Read-only AC-adapter and battery-pack power observations.
//!
//! Both commands report only what a named channel actually measured. Missing
//! fields stay absent with a recorded reason; no nominal rating, design
//! voltage, wall-input figure, or cross-source association is synthesized.

use lctrl_core::{AcPowerObservation, BatteryRateState, LctrlError, PowerDrawObservation};
use lctrl_hal::BatteryControl;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::battery_observation::serialization_error;

/// Channel labels for each platform backend. Core report types stay free of
/// API names; the composition root names the channel that actually answered.
const fn channel_labels(
    platform: lctrl_core::Platform,
) -> (&'static str, &'static str, &'static str) {
    match platform {
        lctrl_core::Platform::Windows => (
            "GetSystemPowerStatus.ACLineStatus",
            "Lenovo EnergyDrv GBMD 0xFF + GAPD 0x8310215c (bit 24 gated)",
            "SetupAPI battery class IOCTL_BATTERY_QUERY_STATUS (BATTERY_STATUS)",
        ),
        lctrl_core::Platform::Linux => (
            "Linux power_supply online",
            "Linux power_supply (battery.adapter)",
            "HAL battery electrical observation",
        ),
    }
}

/// `power ac`: AC line, vendor adapter identity, and adapter power where the
/// firmware advertises it. Succeeds on any partial observation and fails only
/// when both independent sources fail.
pub(crate) fn execute_ac(
    battery: Option<&dyn BatteryControl>,
    platform: lctrl_core::Platform,
) -> crate::CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.ac".into(),
    })?;
    let (ac_line_source, adapter_source, _) = channel_labels(platform);
    let (adapter, adapter_error) = optional(battery.adapter_info());
    let (system, system_error) = optional(battery.system_power_status());
    if adapter.is_none() && system.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "power.ac: AC-line and vendor-adapter sources both failed; adapter={adapter_error:?}; system_power={system_error:?}"
            ),
        });
    }
    let ac_line_status = match system.as_ref().and_then(|status| status.ac_line_status) {
        Some(value) => Some(value),
        // Linux exposes the same `online` bit through the adapter channel; the
        // Windows adapter object is not an AC-line sensor.
        None if platform == lctrl_core::Platform::Linux => {
            adapter.as_ref().and_then(|info| info.ac_connected)
        }
        None => None,
    };
    let observation = AcPowerObservation::from_sources(
        ac_line_status,
        ac_line_source,
        adapter,
        adapter_source,
    );
    let mut payload = object(&observation)?;
    payload.insert("source_association".into(), Value::from("unverified"));
    payload.insert("adapter_error".into(), adapter_error.clone().unwrap_or(Value::Null));
    payload.insert("system_power_error".into(), system_error.clone().unwrap_or(Value::Null));

    let mut human = String::from(
        "AC/adapter observations (read-only; OS AC-line state and vendor adapter are independent sources):\n",
    );
    match observation.ac_line_status {
        Some(true) => human.push_str("- AC line: connected\n"),
        Some(false) => human.push_str("- AC line: on battery\n"),
        None => human.push_str("- AC line: not reported\n"),
    }
    human.push_str(&format!("- AC line source: {}\n", observation.ac_line_source));
    match &observation.adapter {
        Some(info) => {
            human.push_str(&format!(
                "- vendor adapter: authentication={}, detail_advertised={}\n",
                info.authentication, info.has_detail
            ));
            human.push_str(&format!("- adapter source: {}\n", observation.adapter_source));
            match observation.adapter_current_power_w {
                Some(watts) => human.push_str(&format!(
                    "- adapter reported power: current={watts} W, system requirement={}\n",
                    observation
                        .adapter_system_power_w
                        .map_or_else(|| "unknown".to_owned(), |value| format!("{value} W"))
                )),
                None => human.push_str(
                    "- adapter reported power: unavailable (the firmware reported no nonzero adapter power; no number claimed)\n",
                ),
            }
        }
        None => human.push_str("- vendor adapter: unavailable\n"),
    }
    for field in &observation.unavailable {
        human.push_str(&format!("- unavailable: {} ({})\n", field.field, field.reason));
    }
    for issue in &observation.source_issues {
        human.push_str(&format!("- source issue [{}]: {}\n", issue.source, issue.detail));
    }
    if let Some(error) = &adapter_error {
        human.push_str(&format!("- adapter error: {error}\n"));
    }
    if let Some(error) = &system_error {
        human.push_str(&format!("- system power error: {error}\n"));
    }
    Ok(crate::CommandOutput {
        human,
        json: Value::Object(payload),
    })
}

/// `power draw`: one admitted battery-class electrical reading. The raw signed
/// rate and its derived direction are reported; a measured zero is never
/// turned into a draw and an absent rate is never filled in.
pub(crate) fn execute_draw(
    battery: Option<&dyn BatteryControl>,
    platform: lctrl_core::Platform,
) -> crate::CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "power.draw".into(),
    })?;
    let (ac_line_source, _, draw_source) = channel_labels(platform);
    let electrical = battery.battery_electrical()?;
    let (system, system_error) = optional(battery.system_power_status());
    let ac_line_status = system.as_ref().and_then(|status| status.ac_line_status);
    let observation = PowerDrawObservation::from_sources(
        &electrical,
        ac_line_status,
        ac_line_source,
        draw_source,
    );
    let mut payload = object(&observation)?;
    payload.insert(
        "source_association".into(),
        Value::from("single admitted battery pack; not wall or adapter input"),
    );
    payload.insert(
        "system_power_error".into(),
        system_error.clone().unwrap_or(Value::Null),
    );

    let mut human =
        String::from("Battery-pack power flow (read-only; this is not wall-input power):\n");
    human.push_str(&format!("- source: {}\n", observation.source));
    match observation.ac_line_status {
        Some(true) => human.push_str("- AC line: connected\n"),
        Some(false) => human.push_str("- AC line: on battery\n"),
        None => human.push_str("- AC line: not reported\n"),
    }
    human.push_str(&format!("- AC line source: {}\n", observation.ac_line_source));
    match observation.state {
        BatteryRateState::Discharging => {
            if let Some(rate) = observation.rate_mw {
                human.push_str(&format!("- pack rate: {rate} mW (discharging)\n"));
            }
            if let Some(draw) = observation.draw_mw {
                human.push_str(&format!("- draw: {} W\n", watts(draw)));
            }
        }
        BatteryRateState::Charging => {
            if let Some(rate) = observation.rate_mw {
                human.push_str(&format!("- pack rate: +{rate} mW (charging)\n"));
            }
            if let Some(charge) = observation.charge_mw {
                human.push_str(&format!("- charge: {} W\n", watts(charge)));
            }
        }
        BatteryRateState::Zero => human.push_str(
            "- pack rate: 0 mW (measured zero; neither charging nor discharging reported)\n- draw: not asserted\n",
        ),
        BatteryRateState::NotReported => human.push_str(
            "- pack rate: not reported; draw is not asserted\n",
        ),
    }
    match observation.voltage_mv {
        Some(millivolts) => human.push_str(&format!("- pack voltage: {millivolts} mV\n")),
        None => human.push_str("- pack voltage: not reported\n"),
    }
    human.push_str(&format!("- raw power-state bits: 0x{:08x}\n", observation.power_state_bits));
    for field in &observation.unavailable {
        human.push_str(&format!("- unavailable: {} ({})\n", field.field, field.reason));
    }
    for issue in &observation.source_issues {
        human.push_str(&format!("- source issue [{}]: {}\n", issue.source, issue.detail));
    }
    if let Some(error) = &system_error {
        human.push_str(&format!("- system power error: {error}\n"));
    }
    Ok(crate::CommandOutput {
        human,
        json: Value::Object(payload),
    })
}

fn optional<T: Serialize>(result: lctrl_core::Result<T>) -> (Option<T>, Option<Value>) {
    match result {
        Ok(value) => (Some(value), None),
        Err(error) => {
            let report = match serde_json::to_value(error.report()) {
                Ok(value) => value,
                Err(_) => Value::String(error.to_string()),
            };
            (None, Some(report))
        }
    }
}

fn object<T: Serialize>(value: &T) -> std::result::Result<Map<String, Value>, LctrlError> {
    match serde_json::to_value(value).map_err(serialization_error)? {
        Value::Object(map) => Ok(map),
        _ => Err(LctrlError::Io(std::io::Error::other(
            "power observation did not serialize to an object",
        ))),
    }
}

/// Render measured milliwatts as watts without floating-point rounding.
fn watts(milliwatts: u32) -> String {
    let whole = milliwatts / 1000;
    let remainder = milliwatts % 1000;
    if remainder == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{remainder:03}")
    }
}
