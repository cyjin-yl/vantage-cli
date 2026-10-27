use lctrl_core::{
    ApplyMode, Availability, ChangeReport, FanDescriptor, FanId, FanMode, FanTable, LctrlError,
    Result, SensorId, TemperatureLocation, TemperatureSensor, TemperatureSensorMetadata,
    TemperatureSource,
};
use lctrl_hal::{BatteryControl, FanControl, TemperatureControl};

use crate::{WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};

const BATTERY_SENSOR_ID: &str = "battery/0";
const THERMAL_ZONE_QUERY: &str =
    "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
const FAN_INVENTORY_QUERY: &str =
    "SELECT NumOfFans, FanId, FanMinSpeed, FanMaxSpeed FROM LENOVO_FAN_TEST_DATA";

/// Windows temperature and read-only fan-inventory services.
///
/// Battery temperature comes from the existing battery channel. ACPI thermal
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
        match self.battery.battery_telemetry(0) {
            Ok(telemetry) => {
                let value_c = telemetry.temperature_celsius();
                sensors.push(TemperatureSensor {
                    metadata: TemperatureSensorMetadata {
                        id: BATTERY_SENSOR_ID.into(),
                        name: "Battery pack 0 (SMBus)".into(),
                        source: TemperatureSource::BatterySmbus,
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
            Err(LctrlError::Unsupported { .. } | LctrlError::ChannelUnavailable { .. }) => {
                // A battery and its vendor channel are optional on Windows.
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
                value_c: Some((deci_kelvin as f32 - 2731.5) / 10.0),
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
        Err(LctrlError::Unsupported {
            feature: "perf.fan.mode".into(),
        })
    }

    fn set_fan_mode(&self, _mode: FanMode, _apply: ApplyMode) -> Result<ChangeReport<FanMode>> {
        Err(LctrlError::Unsupported {
            feature: "perf.fan.mode.set".into(),
        })
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

fn required_u32(object: &WmiObject, name: &str) -> Result<u32> {
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
