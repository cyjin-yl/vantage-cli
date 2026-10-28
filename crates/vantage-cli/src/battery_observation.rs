//! Source-separated battery observations; no cross-source pack association is assumed.

use lctrl_core::LctrlError;
use lctrl_hal::BatteryControl;
use serde::Serialize;
use serde_json::{Map, Value};

pub(crate) fn execute(battery: Option<&dyn BatteryControl>) -> crate::CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.status".into(),
    })?;
    let mut payload = Map::new();
    payload.insert("source_association".into(), Value::from("unverified"));
    // Read every source independently, even when an earlier source failed.
    let device = capture(
        &mut payload,
        "device_telemetry",
        "device_error",
        battery.battery_telemetry(0),
    )?;
    let os = capture(
        &mut payload,
        "os_battery",
        "os_error",
        battery.os_battery_snapshot(),
    )?;
    let electrical = capture(
        &mut payload,
        "electrical",
        "electrical_error",
        battery.battery_electrical(),
    )?;
    let system = capture(
        &mut payload,
        "system_power",
        "system_power_error",
        battery.system_power_status(),
    )?;
    capture(
        &mut payload,
        "adapter",
        "adapter_error",
        battery.adapter_info(),
    )?;
    capture(
        &mut payload,
        "charge_mode",
        "charge_mode_error",
        battery.charge_mode(),
    )?;
    let json = Value::Object(payload);
    // The human view uses the same complete, labeled observation tree, including
    // partial source issues and structured error context. No fields are flattened.
    let human = format!(
        "Battery observations (source association: unverified; system power is aggregate):\n{}\n",
        serde_json::to_string_pretty(&json).map_err(serialization_error)?
    );
    if !(device || os || electrical || system) {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("battery.status: all primary observation sources failed; {human}"),
        });
    }
    Ok(crate::CommandOutput { human, json })
}

fn serialization_error(error: serde_json::Error) -> LctrlError {
    LctrlError::Io(std::io::Error::other(error))
}

fn capture<T: Serialize>(
    payload: &mut Map<String, Value>,
    value_key: &str,
    error_key: &str,
    result: lctrl_core::Result<T>,
) -> lctrl_core::Result<bool> {
    let (value, error, available) = match result {
        Ok(value) => (
            serde_json::to_value(value).map_err(serialization_error)?,
            Value::Null,
            true,
        ),
        Err(error) => (
            Value::Null,
            serde_json::to_value(error.report()).map_err(serialization_error)?,
            false,
        ),
    };
    payload.insert(value_key.into(), value);
    payload.insert(error_key.into(), error);
    Ok(available)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lctrl_core::{
        AdapterInfo, ApplyMode, BatteryElectricalObservation, BatterySourceIssue, BatteryTelemetry,
        ChangeReport, ChargeMode, ChargeModeActual, OsBatterySnapshot, SystemPowerStatus,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Sources {
        available: bool,
        fail_os: bool,
        calls: AtomicUsize,
    }

    fn unavailable<T>(source: &str) -> lctrl_core::Result<T> {
        Err(LctrlError::ChannelUnavailable {
            channel: source.into(),
        })
    }

    impl BatteryControl for Sources {
        fn battery_telemetry(&self, _: u32) -> lctrl_core::Result<BatteryTelemetry> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.available {
                return unavailable("vendor");
            }
            Ok(BatteryTelemetry {
                design_capacity_mwh: None,
                full_charge_capacity_mwh: None,
                remaining_capacity_mwh: Some(1234),
                voltage_mv: Some(11000),
                current_ma: None,
                temperature_deci_kelvin: None,
                manufacture_date: None,
                first_used_date: None,
                design_voltage_mv: None,
                remaining_percent: Some(17),
                life_percent: None,
                charge_status: None,
                health: None,
                remaining_time_min: None,
                charge_completion_time_min: None,
                wattage_w: None,
                cycle_count: None,
                manufacturer: None,
                model_name: None,
                firmware_version: None,
                serial_number: None,
                chemistry: None,
                source_issues: vec![],
            })
        }
        fn os_battery_snapshot(&self) -> lctrl_core::Result<OsBatterySnapshot> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.available || self.fail_os {
                return unavailable("os-pack");
            }
            Ok(OsBatterySnapshot {
                remaining_percent: Some(43),
                voltage_mv: Some(12000),
                source_issues: vec![BatterySourceIssue {
                    source: "os-temperature",
                    detail: "temperature query denied".into(),
                }],
                ..Default::default()
            })
        }
        fn battery_electrical(&self) -> lctrl_core::Result<BatteryElectricalObservation> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.available {
                return unavailable("native-electrical");
            }
            Ok(BatteryElectricalObservation {
                voltage_mv: Some(13000),
                rate_mw: Some(-26000),
                power_state: 2,
                temperature_deci_kelvin: None,
                source_issues: Vec::new(),
            })
        }
        fn system_power_status(&self) -> lctrl_core::Result<SystemPowerStatus> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.available {
                return unavailable("aggregate-power");
            }
            Ok(SystemPowerStatus {
                battery_life_percent: Some(81),
                ..Default::default()
            })
        }
        fn adapter_info(&self) -> lctrl_core::Result<AdapterInfo> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            unavailable("adapter-channel")
        }
        fn charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            unavailable("charge-channel")
        }
        fn set_charge_mode(
            &self,
            _: ChargeMode,
            _: ApplyMode,
        ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
            panic!("status must not write")
        }
    }

    #[test]
    fn keeps_disagreeing_sources_separate_without_inventing_current_or_health() {
        let sources = Sources {
            available: true,
            fail_os: false,
            calls: AtomicUsize::new(0),
        };
        let output = execute(Some(&sources)).unwrap();
        let json = &output.json;
        assert_eq!(sources.calls.load(Ordering::SeqCst), 6);
        assert_eq!(json["source_association"], "unverified");
        assert_eq!(json["device_telemetry"]["remaining_percent"], 17);
        assert_eq!(json["os_battery"]["remaining_percent"], 43);
        assert_eq!(json["system_power"]["battery_life_percent"], 81);
        assert_eq!(json["device_telemetry"]["voltage_mv"], 11000);
        assert_eq!(json["os_battery"]["voltage_mv"], 12000);
        assert_eq!(json["electrical"]["voltage_mv"], 13000);
        assert_eq!(json["electrical"]["rate_mw"], -26000);
        assert!(json["device_telemetry"]["current_ma"].is_null());
        assert!(json["device_telemetry"]["health"].is_null());
        assert!(json["electrical"].get("current_ma").is_none());
        for key in [
            "telemetry",
            "remaining_percent",
            "current_ma",
            "health",
            "voltage_mv",
        ] {
            assert!(json.get(key).is_none(), "unexpected flattened field {key}");
        }
    }

    #[test]
    fn retains_partial_issues_and_optional_errors_in_both_views() {
        let sources = Sources {
            available: true,
            fail_os: false,
            calls: AtomicUsize::new(0),
        };
        let output = execute(Some(&sources)).unwrap();
        assert!(output.json["adapter"].is_null());
        assert!(output.json["charge_mode"].is_null());
        assert_eq!(
            output.json["adapter_error"]["error"]["channel"],
            "adapter-channel"
        );
        assert_eq!(
            output.json["charge_mode_error"]["error"]["channel"],
            "charge-channel"
        );
        assert_eq!(
            output.json["os_battery"]["source_issues"][0]["source"],
            "os-temperature"
        );
        for detail in [
            "adapter-channel",
            "charge-channel",
            "temperature query denied",
        ] {
            assert!(output.human.contains(detail));
        }
    }

    #[test]
    fn primary_source_failure_does_not_hide_other_observations() {
        let sources = Sources {
            available: true,
            fail_os: true,
            calls: AtomicUsize::new(0),
        };
        let output = execute(Some(&sources)).unwrap();
        assert!(output.json["os_battery"].is_null());
        assert_eq!(output.json["os_error"]["error"]["channel"], "os-pack");
        assert!(output.human.contains("os-pack"));
        assert_eq!(output.json["device_telemetry"]["remaining_percent"], 17);
        assert_eq!(output.json["system_power"]["battery_life_percent"], 81);
        assert_eq!(sources.calls.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn all_primary_failures_return_nonzero_with_every_source_context() {
        let sources = Sources {
            available: false,
            fail_os: false,
            calls: AtomicUsize::new(0),
        };
        let error = execute(Some(&sources)).unwrap_err();
        assert_ne!(error.exit_code(), 0);
        assert_eq!(sources.calls.load(Ordering::SeqCst), 6);
        let detail = error.to_string();
        for source in [
            "vendor",
            "os-pack",
            "native-electrical",
            "aggregate-power",
            "adapter-channel",
            "charge-channel",
        ] {
            assert!(detail.contains(source), "missing error context: {source}");
        }
    }

    #[test]
    fn absent_service_is_an_error() {
        assert_ne!(execute(None).unwrap_err().exit_code(), 0);
    }
}
