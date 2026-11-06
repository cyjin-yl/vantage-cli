use lctrl_core::{
    Availability, LctrlError, Result, TemperatureLocation, TemperatureSensor,
    TemperatureSensorMetadata, TemperatureSource,
};
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use lctrl_core::{
    ApplyMode, ChangeReport, FanDescriptor, FanId, FanMode, FanTable, SensorId,
};

use lctrl_hal::{BatteryControl, DbdcControl, TemperatureControl};
#[cfg(windows)]
use lctrl_hal::{FanControl, poll_readback};

use crate::{WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};
#[cfg(windows)]
use crate::GameZone;
// The PowerSlider projection is the Win32 registry reader, so both the reader
// and the fan-mode fallback that consumes it exist only on Windows. The rest of
// this module stays portable so the Linux CI host still typechecks it.
#[cfg(windows)]
use crate::{NativePerformanceRegistry, PerformanceRegistryReader};

const BATTERY_SENSOR_ID: &str = "windows-battery-class/temperature";
const THERMAL_ZONE_QUERY: &str =
    "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature";
const ESIF_PARTICIPANT_QUERY: &str =
    "SELECT InstanceName, Temperature FROM EsifDeviceInformation";
#[cfg(windows)]
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
/// Intel ESIF / Dynamic Tuning participants (`root\WMI
/// EsifDeviceInformation`) are reported under the provider's own instance name
/// for the same reason: the class publishes no participant name, and its
/// trip-point/power fields are limits, not measurements
/// (docs/evidence/re-thermal-20261105.md).
///
/// The registry reader supplies the dispatcher's `ITS_FanMode` projection, which
/// is only ever a fallback for the firmware fan-mode read.
pub struct WindowsThermal<W, B, R> {
    wmi: W,
    battery: B,
    registry: R,
}

#[cfg(windows)]
impl<W, B> WindowsThermal<W, B, NativePerformanceRegistry> {
    #[must_use]
    pub const fn new(wmi: W, battery: B) -> Self {
        Self {
            wmi,
            battery,
            registry: NativePerformanceRegistry,
        }
    }
}

impl<W, B, R> WindowsThermal<W, B, R> {
    /// Construct with an explicit PowerSlider reader (scripted fakes in tests).
    #[must_use]
    pub const fn with_registry(wmi: W, battery: B, registry: R) -> Self {
        Self {
            wmi,
            battery,
            registry,
        }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn battery(&self) -> &B {
        &self.battery
    }

    #[must_use]
    pub const fn registry(&self) -> &R {
        &self.registry
    }
}

#[cfg(windows)]
impl<W, B, R> WindowsThermal<W, B, R>
where
    W: WmiTransport,
    B: BatteryControl,
{
    /// Firmware fan-mode readback (`LENOVO_GAMEZONE_DATA.GetSmartFanMode`).
    ///
    /// This is the only read that can confirm a fan-mode write. It is absent on
    /// firmware whose GAMEZONE provider rejects the fan getters
    /// (`WBEM_E_INVALID_OBJECT`), e.g. ThinkBook 21VG.
    fn firmware_fan_mode(&self) -> Result<FanMode> {
        let raw = GameZone::new(&self.wmi).smart_fan_mode()?;
        u16::try_from(raw)
            .map(FanMode::from_raw)
            .map_err(|_| LctrlError::ChannelUnavailable {
                channel: format!("GetSmartFanMode returned {raw}, outside the u16 fan-mode domain"),
            })
    }
}

impl<W, B, R> TemperatureControl for WindowsThermal<W, B, R>
where
    W: WmiTransport,
    B: BatteryControl,
    // The trait requires `Send + Sync`; `R` is otherwise unused here.
    R: Send + Sync,
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

        // Intel ESIF / Dynamic Tuning participants. This is the only channel on
        // this SKU that returns a package-adjacent temperature: the GAMEZONE
        // getters are absent from the firmware, the fan-table sensor
        // enumeration is unsupported, and the ACPI class exposes a single
        // unnamed zone (docs/evidence/re-thermal-20261105.md §1, §3).
        let participants = match self.wmi.query(ROOT_WMI, ESIF_PARTICIPANT_QUERY) {
            Ok(participants) => participants,
            // Machines without Intel Dynamic Tuning have no such class. Other
            // errors describe a present source that failed.
            Err(LctrlError::Unsupported { .. }) => Vec::new(),
            Err(error) => return Err(error),
        };
        for participant in &participants {
            let instance_name = required_instance_name(participant)?;
            let temperature = required_u32(participant, "Temperature")?;
            // The provider reports whole degrees Celsius on this platform even
            // though the class metadata declares 1/10 K: that declaration is
            // boilerplate shared with the trip-point fields, and a
            // tenths-of-Kelvin reading would put a live participant below the
            // battery and board readings sampled in the same second
            // (docs/evidence/re-thermal-20261105.md §3.4). Nothing is
            // converted here.
            //
            // A reported 0 is the provider's "this participant has no
            // temperature" value: those instances return 0 in every sample
            // while their peers move (§3.5). It is not a reading, so the
            // instance is omitted rather than published as a measured 0.0 °C.
            if temperature == 0 {
                continue;
            }
            sensors.push(TemperatureSensor {
                metadata: TemperatureSensorMetadata {
                    id: format!("esif/{instance_name}"),
                    name: format!("Intel ESIF participant ({instance_name})"),
                    source: TemperatureSource::WmiEsif,
                    location: TemperatureLocation::Unknown,
                    availability: Availability::Available,
                },
                value_c: Some(f64::from(temperature) as f32),
            });
        }

        sensors.sort_by(|left, right| left.metadata.id.cmp(&right.metadata.id));
        if sensors
            .windows(2)
            .any(|pair| pair[0].metadata.id == pair[1].metadata.id)
        {
            return Err(LctrlError::ChannelUnavailable {
                channel: "temperature sources returned duplicate sensor ids".into(),
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

#[cfg(windows)]
impl<W, B, R> FanControl for WindowsThermal<W, B, R>
where
    W: WmiTransport,
    B: BatteryControl,
    R: PerformanceRegistryReader,
{
    fn fan_mode(&self) -> Result<FanMode> {
        // Firmware readback first. Where the GAMEZONE provider rejects it
        // (`GetSmartFanMode` -> WBEM_E_INVALID_OBJECT on 21VG), report the
        // dispatcher's PowerSlider value instead. That value is a write-only
        // dispatcher projection - nothing in LNVDispatcherService reads it back
        // into firmware (docs/evidence/re-fan-write-20261105.md §3) - so it is
        // returned raw (`FanMode::from_raw`, unknown values stay `Unknown`) and
        // never presented as a confirmed firmware mode.
        if let Ok(mode) = self.firmware_fan_mode() {
            return Ok(mode);
        }
        let reg = self
            .registry
            .read_dword("ITS_FanMode")
            .map_err(|_| LctrlError::ChannelUnavailable {
                channel: "GAMEZONE GetSmartFanMode rejected and ITS_FanMode registry unreadable"
                    .to_string(),
            })?;
        u16::try_from(reg)
            .map(FanMode::from_raw)
            .map_err(|_| LctrlError::ChannelUnavailable {
                channel: format!("ITS_FanMode registry value {reg} outside u16"),
            })
    }

    fn set_fan_mode(&self, mode: FanMode, apply: ApplyMode) -> Result<ChangeReport<FanMode>> {
        // Reject a mode with no firmware encoding before touching any channel.
        let raw = smart_fan_mode_raw(mode)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(self.fan_mode()?, mode));
        }
        // Never issue a write whose result cannot be read back. On 21VG the
        // firmware acknowledges one `SetSmartFanMode` per caller process and
        // rejects every later call regardless of value or VARIANT type, while
        // every fan getter returns WBEM_E_INVALID_OBJECT; `ITS_FanMode` is a
        // dispatcher projection that our write does not update. Writing there
        // would report an effect that is unobservable and unvalidated
        // (docs/evidence/re-fan-write-20261105.md §2, §5).
        let firmware_previous =
            self.firmware_fan_mode()
                .map_err(|error| LctrlError::ChannelUnavailable {
                    channel: format!(
                        "perf.fan.mode.write requires a firmware fan-mode readback, \
                         but GetSmartFanMode is unavailable ({error}); the GAMEZONE setter \
                         acknowledges calls without validating the value and nothing reads \
                         the result back (docs/evidence/re-fan-write-20261105.md)"
                    ),
                })?;
        GameZone::new(&self.wmi).set_smart_fan_mode(u32::from(raw))?;
        let actual =
            poll_readback(&mode, 3, Duration::from_millis(200), || self.firmware_fan_mode())?;
        Ok(ChangeReport::committed(firmware_previous, mode, actual))
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
/// performance=2, custom=3). Reverse engineering (interfaces.md §10.3) shows
/// the vendor plugin operates on raw firmware encodings with 0x7A/0x80
/// threshold comparisons, so this mapping is unconfirmed and partially
/// refuted.
///
/// The 21VG provider exposes no fan-mode domain at all: `SetSmartFanMode`
/// acknowledges one call per caller process and rejects every later call for
/// every value tried (0, 1, 2, 3, 4, 5, 255) and every VARIANT type
/// (docs/evidence/re-fan-write-20261105.md §2). This table therefore stays
/// unverified, and `WindowsThermal::set_fan_mode` only reaches it on firmware
/// that also answers `GetSmartFanMode`.
#[cfg(windows)]
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

#[cfg(windows)]
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

impl<W, B, R> DbdcControl for WindowsThermal<W, B, R>
where
    W: WmiTransport,
    B: BatteryControl,
    // The trait requires `Send + Sync`; `R` is otherwise unused here.
    R: Send + Sync,
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
