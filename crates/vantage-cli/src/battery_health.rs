//! Battery wear, design-versus-full-charge capacity and cycle count with
//! per-field provenance. Source values are never merged between channels.

use lctrl_core::{BatteryHealth, LctrlError};
use lctrl_hal::BatteryControl;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::battery_observation::{capture, serialization_error};

/// The vendor device response that carries the firmware health code
/// (docs/02 §7.1a, §7.2). Its health/life fields are reported beside the
/// capacity facts, never merged into them.
const VENDOR_HEALTH_SOURCE: &str = "energy_drv_83102138_device0";

pub(crate) fn execute(battery: Option<&dyn BatteryControl>) -> crate::CommandResult {
    let battery = battery.ok_or_else(|| LctrlError::Unsupported {
        feature: "battery.health".into(),
    })?;
    let mut payload = Map::new();
    payload.insert("source_association".into(), Value::from("unverified"));
    // Read both channels even when the first failed; each keeps its own error.
    let wear = capture(
        &mut payload,
        "capacity_wear",
        "capacity_wear_error",
        battery.battery_wear(),
    )?;
    capture(
        &mut payload,
        "vendor_health",
        "vendor_health_error",
        vendor_health(battery),
    )?;
    let json = Value::Object(payload);
    let human = format!(
        "Battery health (capacities in mWh; wear percent is withheld when the design capacity is \
         absent or zero, or when the full-charge capacity exceeds it):\n{}\n",
        serde_json::to_string_pretty(&json).map_err(serialization_error)?
    );
    if !wear {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("battery.health: the capacity channel failed; {human}"),
        });
    }
    Ok(crate::CommandOutput { human, json })
}

/// Only the vendor-only facts: a firmware health code and a life percentage.
#[derive(Serialize)]
struct VendorHealth {
    source: &'static str,
    health: Option<BatteryHealth>,
    life_percent: Option<u16>,
}

fn vendor_health(battery: &dyn BatteryControl) -> lctrl_core::Result<VendorHealth> {
    let telemetry = battery.battery_telemetry(0)?;
    Ok(VendorHealth {
        source: VENDOR_HEALTH_SOURCE,
        health: telemetry.health,
        life_percent: telemetry.life_percent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lctrl_core::{
        AdapterInfo, ApplyMode, BatteryElectricalObservation, BatteryFieldProvenance,
        BatterySourceIssue, BatteryTelemetry, BatteryWearReport, ChangeReport, ChargeMode,
        ChargeModeActual, OsBatterySnapshot, SystemPowerStatus,
    };

    const FIELDS: BatteryFieldProvenance = BatteryFieldProvenance {
        design_capacity: "root\\WMI:BatteryStaticData.DesignedCapacity",
        full_charge_capacity: "root\\WMI:BatteryFullChargedCapacity.FullChargedCapacity",
        cycle_count: "root\\WMI:BatteryCycleCount.CycleCount",
    };

    struct Fake {
        wear: lctrl_core::Result<BatteryWearReport>,
        health: Option<BatteryHealth>,
        life_percent: Option<u16>,
    }

    impl Fake {
        fn new(wear: lctrl_core::Result<BatteryWearReport>) -> Self {
            Self {
                wear,
                health: None,
                life_percent: None,
            }
        }
    }

    impl BatteryControl for Fake {
        fn battery_telemetry(&self, _: u32) -> lctrl_core::Result<BatteryTelemetry> {
            Ok(BatteryTelemetry {
                design_capacity_mwh: Some(99_900),
                full_charge_capacity_mwh: Some(93_810),
                remaining_capacity_mwh: None,
                voltage_mv: None,
                current_ma: None,
                temperature_deci_kelvin: None,
                manufacture_date: None,
                first_used_date: None,
                design_voltage_mv: None,
                remaining_percent: None,
                life_percent: self.life_percent,
                charge_status: None,
                health: self.health,
                remaining_time_min: None,
                charge_completion_time_min: None,
                wattage_w: None,
                cycle_count: None,
                manufacturer: None,
                model_name: None,
                firmware_version: None,
                serial_number: None,
                chemistry: None,
                source_issues: Vec::new(),
            })
        }

        fn battery_wear(&self) -> lctrl_core::Result<BatteryWearReport> {
            match &self.wear {
                Ok(report) => Ok(report.clone()),
                Err(_) => Err(LctrlError::ChannelUnavailable {
                    channel: "fake capacity channel".into(),
                }),
            }
        }

        fn os_battery_snapshot(&self) -> lctrl_core::Result<OsBatterySnapshot> {
            Err(LctrlError::Unsupported {
                feature: "battery.os-observation".into(),
            })
        }

        fn battery_electrical(&self) -> lctrl_core::Result<BatteryElectricalObservation> {
            Err(LctrlError::Unsupported {
                feature: "battery.electrical-observation".into(),
            })
        }

        fn system_power_status(&self) -> lctrl_core::Result<SystemPowerStatus> {
            Err(LctrlError::Unsupported {
                feature: "battery.system-power".into(),
            })
        }

        fn adapter_info(&self) -> lctrl_core::Result<AdapterInfo> {
            Err(LctrlError::Unsupported {
                feature: "battery.adapter".into(),
            })
        }

        fn charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
            Err(LctrlError::Unsupported {
                feature: "battery.charge-mode.readback".into(),
            })
        }

        fn set_charge_mode(
            &self,
            _: ChargeMode,
            _: ApplyMode,
        ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
            Err(LctrlError::Unsupported {
                feature: "battery.charge_mode".into(),
            })
        }
    }

    #[test]
    fn geometry_and_provenance_are_reported_per_field() {
        let mut fake = Fake::new(Ok(BatteryWearReport::from_capacities(
            "root_wmi_battery_classes",
            FIELDS,
            Some(99_900),
            Some(93_810),
            Some(233),
            Vec::new(),
        )));
        fake.health = Some(BatteryHealth::Green);
        fake.life_percent = Some(93);
        let output = execute(Some(&fake)).expect("wear facts are available");
        assert_eq!(output.json["capacity_wear"]["wear_percent"], 6);
        assert_eq!(output.json["capacity_wear"]["capacity_ratio_percent"], 93);
        assert_eq!(output.json["capacity_wear"]["cycle_count"], 233);
        assert_eq!(
            output.json["capacity_wear"]["fields"]["design_capacity"],
            "root\\WMI:BatteryStaticData.DesignedCapacity"
        );
        assert_eq!(output.json["vendor_health"]["health"], "green");
        assert_eq!(output.json["vendor_health"]["source"], VENDOR_HEALTH_SOURCE);
    }

    #[test]
    fn missing_design_capacity_refuses_wear_but_keeps_the_observation() {
        let fake = Fake::new(Ok(BatteryWearReport::from_capacities(
            "root_wmi_battery_classes",
            FIELDS,
            None,
            Some(93_810),
            Some(233),
            Vec::new(),
        )));
        let output = execute(Some(&fake)).expect("a refusal is still an observation");
        assert!(output.json["capacity_wear"]["wear_percent"].is_null());
        assert!(output.json["capacity_wear"]["capacity_ratio_percent"].is_null());
        assert_eq!(output.json["capacity_wear"]["full_charge_capacity_mwh"], 93_810);
        assert_eq!(output.json["capacity_wear"]["cycle_count"], 233);
        let issues = output.json["capacity_wear"]["issues"]
            .as_array()
            .expect("issues array");
        assert!(
            issues.iter().any(|issue| issue["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("design capacity"))),
            "the refusal reason must name the missing design capacity"
        );
    }

    #[test]
    fn a_failed_capacity_channel_fails_but_reports_the_vendor_facts() {
        let fake = Fake::new(Err(LctrlError::ChannelUnavailable {
            channel: "root\\WMI battery classes".into(),
        }));
        let error = execute(Some(&fake)).expect_err("no capacity channel");
        assert!(error.to_string().contains("battery.health"));
    }

    #[test]
    fn a_missing_battery_service_is_unsupported() {
        let error = execute(None).expect_err("no battery service");
        assert!(error.to_string().contains("battery.health"));
    }

    #[test]
    fn source_issues_from_the_channel_are_preserved() {
        let fake = Fake::new(Ok(BatteryWearReport::from_capacities(
            "root_wmi_battery_classes",
            FIELDS,
            None,
            None,
            None,
            vec![BatterySourceIssue {
                source: "BatteryStaticData",
                detail: "relative capacity units; mWh unavailable".into(),
            }],
        )));
        let output = execute(Some(&fake)).expect("issues are reported");
        let issues = output.json["capacity_wear"]["issues"]
            .as_array()
            .expect("issues array");
        assert!(issues
            .iter()
            .any(|issue| issue["source"] == "BatteryStaticData"));
    }
}
