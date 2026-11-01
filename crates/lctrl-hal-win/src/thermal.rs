use std::time::Duration;

use lctrl_core::{
    ApplyMode, Availability, ChangeReport, FanDescriptor, FanId, FanMode, FanTable, LctrlError,
    Result, SensorId, TemperatureLocation, TemperatureSensor, TemperatureSensorMetadata,
    TemperatureSource,
};
use lctrl_hal::{BatteryControl, DbdcControl, FanControl, TemperatureControl, poll_readback};

use crate::{GameZone, WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};

const BATTERY_SENSOR_ID: &str = "windows-battery-class/temperature";
const THERMAL_ZONE_QUERY: &str =
    "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
const FAN_INVENTORY_QUERY: &str =
    "SELECT NumOfFans, FanId, FanMinSpeed, FanMaxSpeed FROM LENOVO_FAN_TEST_DATA";
const DBDC_QUERY: &str =
    "SELECT Counts, CurrentLimit, ROS_Power, Threshold FROM LENOVO_REPORT_DBDC_DATA";

/// Windows temperature and read-only fan-inventory services.
///
/// Battery temperature comes from the Windows battery class, without a vendor
/// pack association. ACPI thermal
/// zones retain their firmware instance identity and are never relabelled as
/// CPU sensors: the public WMI class does not establish that physical mapping.
pub struct WindowsThermal<W, B> {
    wmi: W,
    battery: B,
}

impl<W, B> WindowsThermal<W, B> {
    #[must_use]
    pub const fn new(wmi: W, battery: B) -> Self {
        Self { wmi, battery }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn battery(&self) -> &B {
        &self.battery
    }
}

impl<W, B> TemperatureControl for WindowsThermal<W, B>
where
    W: WmiTransport,
    B: BatteryControl,
{
    fn temperature_sensors(&self) -> Result<Vec<TemperatureSensor>> {
        let mut sensors = Vec::new();
        match self.battery.battery_electrical() {
            Ok(observation) => {
                if !observation.source_issues.is_empty() {
                    return Err(LctrlError::ChannelUnavailable {
                        channel: observation
                            .source_issues
                            .iter()
                            .map(|issue| format!("{}: {}", issue.source, issue.detail))
                            .collect::<Vec<_>>()
                            .join("; "),
                    });
                }
                let value_c = observation
                    .temperature_deci_kelvin
                    .map(|value| (f64::from(value) / 10.0 - 273.15) as f32);
                sensors.push(TemperatureSensor {
                    metadata: TemperatureSensorMetadata {
                        id: BATTERY_SENSOR_ID.into(),
                        name: "Windows battery class temperature (unassociated)".into(),
                        source: TemperatureSource::WindowsBatteryClass,
                        location: TemperatureLocation::Battery,
                        availability: if value_c.is_some() {
                            Availability::Available
                        } else {
                            Availability::Unavailable
                        },
                    },
                    value_c,
                });
            }
            Err(LctrlError::Unsupported { .. }) => {
                // The native battery source is optional on Windows.
                // Other errors still describe a present source that failed.
            }
            Err(error) => return Err(error),
        }

        let zones = match self.wmi.query(ROOT_WMI, THERMAL_ZONE_QUERY) {
            Ok(zones) => zones,
            // Native WMI maps an absent class to Unsupported. Service,
            // permission, and malformed-response errors remain observable.
            Err(LctrlError::Unsupported { .. }) => Vec::new(),
            Err(error) => return Err(error),
        };
        for zone in &zones {
            let instance_name = required_instance_name(zone)?;
            let deci_kelvin = required_u32(zone, "CurrentTemperature")?;
            sensors.push(TemperatureSensor {
                metadata: TemperatureSensorMetadata {
                    id: format!("acpi/{instance_name}"),
                    name: format!("ACPI thermal zone ({instance_name})"),
                    source: TemperatureSource::Acpi,
                    location: TemperatureLocation::Unknown,
                    availability: Availability::Available,
                },
                value_c: Some((f64::from(deci_kelvin) / 10.0 - 273.15) as f32),
            });
        }

        sensors.sort_by(|left, right| left.metadata.id.cmp(&right.metadata.id));
        if sensors
            .windows(2)
            .any(|pair| pair[0].metadata.id == pair[1].metadata.id)
        {
            return Err(LctrlError::ChannelUnavailable {
                channel: "MSAcpi_ThermalZoneTemperature returned duplicate InstanceName values"
                    .into(),
            });
        }
        if sensors.is_empty() {
            return Err(LctrlError::Unsupported {
                feature: "perf.temp".into(),
            });
        }
        Ok(sensors)
    }

    fn temperature(&self, id: &str) -> Result<TemperatureSensor> {
        self.temperature_sensors()?
            .into_iter()
            .find(|sensor| sensor.metadata.id == id)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!("temperature sensor {id:?} was not found"),
            })
    }
}

impl<W, B> FanControl for WindowsThermal<W, B>
where
    W: WmiTransport,
    B: BatteryControl,
{
    fn fan_mode(&self) -> Result<FanMode> {
        let raw = GameZone::new(&self.wmi).smart_fan_mode()?;
        let raw = u16::try_from(raw).map_err(|_| LctrlError::ChannelUnavailable {
            channel: format!("GetSmartFanMode returned {raw}, outside the u16 fan-mode domain"),
        })?;
        Ok(FanMode::from_raw(raw))
    }

    fn set_fan_mode(&self, mode: FanMode, apply: ApplyMode) -> Result<ChangeReport<FanMode>> {
        let raw = smart_fan_mode_raw(mode)?;
        let previous = self.fan_mode()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }
        GameZone::new(&self.wmi).set_smart_fan_mode(u32::from(raw))?;
        let actual = poll_readback(&mode, 3, Duration::from_millis(200), || self.fan_mode())?;
        Ok(ChangeReport::committed(previous, mode, actual))
    }

    fn fans(&self) -> Result<Vec<FanDescriptor>> {
        let objects = self.wmi.query(ROOT_WMI, FAN_INVENTORY_QUERY)?;
        let mut fans = Vec::new();
        for object in &objects {
            let count = usize::try_from(required_u32(object, "NumOfFans")?).map_err(|_| {
                LctrlError::ChannelUnavailable {
                    channel: "LENOVO_FAN_TEST_DATA NumOfFans exceeds usize".into(),
                }
            })?;
            let ids = required_array(object, "FanId")?;
            let min_speeds = required_array(object, "FanMinSpeed")?;
            let max_speeds = required_array(object, "FanMaxSpeed")?;
            if ids.len() != count || min_speeds.len() != count || max_speeds.len() != count {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "LENOVO_FAN_TEST_DATA NumOfFans is {count}, but FanId/FanMinSpeed/FanMaxSpeed lengths are {}/{}/{}",
                        ids.len(),
                        min_speeds.len(),
                        max_speeds.len()
                    ),
                });
            }

            // Validate all lengths before indexing. Each array element at the
            // same index describes the same fan; zip would hide truncation.
            for index in 0..count {
                fans.push(FanDescriptor::new(
                    FanId::new(numeric_u32(&ids[index], "FanId")?),
                    numeric_u32(&min_speeds[index], "FanMinSpeed")?,
                    numeric_u32(&max_speeds[index], "FanMaxSpeed")?,
                )?);
            }
        }
        fans.sort_by_key(|fan| fan.id);
        if fans.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(LctrlError::ChannelUnavailable {
                channel: "LENOVO_FAN_TEST_DATA returned duplicate fan IDs".into(),
            });
        }
        Ok(fans)
    }

    fn fan_table(&self, _fan: FanId, _sensor: SensorId) -> Result<FanTable> {
        Err(LctrlError::Unsupported {
            feature: "perf.fan.curve".into(),
        })
    }
}

/// Maps the semantic fan mode to the firmware `SetSmartFanMode` value.
///
/// Placeholder inverse of [`FanMode::from_raw`] (standard=0, silent=1,
/// performance=2, custom=3). Reverse engineering (interfaces.md §10.3)
/// shows the vendor plugin operates on raw firmware encodings with 0x7A/0x80
/// threshold comparisons, so this mapping is unconfirmed and partially
/// refuted; the reference machine's provider rejects every GAMEZONE call,
/// so it cannot be validated there either. Treat the write path as
/// unverified until a supporting machine confirms the wire values.
fn smart_fan_mode_raw(mode: FanMode) -> Result<u16> {
    match mode {
        FanMode::Standard => Ok(0),
        FanMode::Silent => Ok(1),
        FanMode::Performance => Ok(2),
        FanMode::Custom => Ok(3),
        FanMode::Unknown(raw) => Err(LctrlError::InvalidArgument {
            detail: format!("cannot write unknown firmware fan mode {raw}"),
        }),
    }
}

fn required_instance_name(object: &WmiObject) -> Result<&str> {
    match object.get("InstanceName") {
        Some(WmiValue::String(name)) if !name.is_empty() => Ok(name),
        Some(other) => Err(invalid_wmi_value("InstanceName", other)),
        None => Err(LctrlError::ChannelUnavailable {
            channel: "MSAcpi_ThermalZoneTemperature omitted InstanceName".into(),
        }),
    }
}

fn required_array<'a>(object: &'a WmiObject, name: &str) -> Result<&'a [WmiValue]> {
    match object.get(name) {
        Some(WmiValue::Array(values)) => Ok(values),
        Some(other) => Err(invalid_wmi_value(name, other)),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("LENOVO_FAN_TEST_DATA omitted {name}"),
        }),
    }
}

pub(crate) fn required_u32(object: &WmiObject, name: &str) -> Result<u32> {
    object
        .get(name)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("WMI temperature/fan data omitted {name}"),
        })
        .and_then(|value| numeric_u32(value, name))
}

fn numeric_u32(wmi_value: &WmiValue, name: &str) -> Result<u32> {
    match wmi_value {
        WmiValue::U8(value) => Ok(u32::from(*value)),
        WmiValue::U16(value) => Ok(u32::from(*value)),
        WmiValue::U32(value) => Ok(*value),
        WmiValue::U64(value) => {
            u32::try_from(*value).map_err(|_| invalid_wmi_value(name, wmi_value))
        }
        WmiValue::I8(value) => {
            u32::try_from(*value).map_err(|_| invalid_wmi_value(name, wmi_value))
        }
        WmiValue::I16(value) => {
            u32::try_from(*value).map_err(|_| invalid_wmi_value(name, wmi_value))
        }
        WmiValue::I32(value) => {
            u32::try_from(*value).map_err(|_| invalid_wmi_value(name, wmi_value))
        }
        WmiValue::I64(value) => {
            u32::try_from(*value).map_err(|_| invalid_wmi_value(name, wmi_value))
        }
        other => Err(invalid_wmi_value(name, other)),
    }
}

fn invalid_wmi_value(name: &str, value: &WmiValue) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("WMI {name} has an invalid value: {value:?}"),
    }
}

impl<W, B> DbdcControl for WindowsThermal<W, B>
where
    W: WmiTransport,
    B: BatteryControl,
{
    fn dbdc_profile(&self) -> Result<lctrl_core::DbdcProfile> {
        let rows = self.wmi.query(ROOT_WMI, DBDC_QUERY)?;
        let row = rows.first().ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "LENOVO_REPORT_DBDC_DATA returned no instance".into(),
        })?;
        let counts = row
            .get("Counts")
            .ok_or_else(|| invalid_wmi_value("Counts", &WmiValue::Empty))
            .and_then(|value| numeric_u32(value, "Counts"))?;
        let current_limit_ma = dbdc_u32_array(row, "CurrentLimit")?;
        let ros_power_mw = dbdc_u32_array(row, "ROS_Power")?;
        let threshold_pct = dbdc_u32_array(row, "Threshold")?;
        let profile = lctrl_core::DbdcProfile {
            counts,
            current_limit_ma,
            ros_power_mw,
            threshold_pct,
        };
        if !profile.is_consistent() {
            return Err(invalid_wmi_value("DBDC arrays", &WmiValue::Null));
        }
        Ok(profile)
    }
}

fn dbdc_u32_array(row: &WmiObject, name: &str) -> Result<Vec<u32>> {
    match row.get(name) {
        Some(WmiValue::Array(items)) => items.iter().map(|item| numeric_u32(item, name)).collect(),
        Some(other) => Err(invalid_wmi_value(name, other)),
        None => Err(invalid_wmi_value(name, &WmiValue::Empty)),
    }
}
