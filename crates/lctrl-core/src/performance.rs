use std::fmt;

use serde::Serialize;

use crate::{Availability, LctrlError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Balanced,
    Quiet,
    Performance,
    Geek,
}

impl PerformanceMode {
    #[must_use]
    pub const fn its_value(self) -> u32 {
        match self {
            Self::Balanced => 1,
            Self::Quiet => 2,
            Self::Performance => 3,
            Self::Geek => 4,
        }
    }

    pub fn from_its(raw: u32) -> Result<Self> {
        match raw {
            1 => Ok(Self::Balanced),
            2 => Ok(Self::Quiet),
            3 => Ok(Self::Performance),
            4 => Ok(Self::Geek),
            _ => Err(LctrlError::InvalidArgument {
                detail: format!("unknown ITS performance mode {raw}"),
            }),
        }
    }
}

impl fmt::Display for PerformanceMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Balanced => "balanced",
            Self::Quiet => "quiet",
            Self::Performance => "performance",
            Self::Geek => "geek",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatcherVersion {
    Legacy(u32),
    V2,
    V3,
    V4,
}

impl DispatcherVersion {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        if raw >= 0x3000 {
            Self::V4
        } else if raw >= 0x2000 {
            Self::V3
        } else if raw >= 0x1000 {
            Self::V2
        } else {
            Self::Legacy(raw)
        }
    }

    #[must_use]
    pub const fn supports_geek(self) -> bool {
        matches!(self, Self::V4)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PerformanceCapabilities {
    raw: u32,
}

impl PerformanceCapabilities {
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self { raw }
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.raw
    }

    #[must_use]
    pub fn supports(self, mode: PerformanceMode, version: DispatcherVersion) -> bool {
        let mask = match mode {
            PerformanceMode::Balanced => 0x01,
            PerformanceMode::Quiet => 0x02,
            PerformanceMode::Performance => 0x08,
            PerformanceMode::Geek => 0x10,
        };
        self.raw & mask != 0 && (mode != PerformanceMode::Geek || version.supports_geek())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PerformanceState {
    pub requested: Option<PerformanceMode>,
    pub active: Option<PerformanceMode>,
    pub automatic: bool,
    pub version: DispatcherVersion,
    pub capabilities: PerformanceCapabilities,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct FanId(u32);

impl FanId {
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn method_arg(self) -> Result<u8> {
        u8::try_from(self.0).map_err(|_| LctrlError::InvalidArgument {
            detail: format!("fan id {} exceeds WMI u8 boundary", self.0),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct SensorId(u32);

impl SensorId {
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn method_arg(self) -> Result<u8> {
        u8::try_from(self.0).map_err(|_| LctrlError::InvalidArgument {
            detail: format!("sensor id {} exceeds WMI u8 boundary", self.0),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FanDescriptor {
    pub id: FanId,
    pub min_rpm: u32,
    pub max_rpm: u32,
}

impl FanDescriptor {
    pub fn new(id: FanId, min_rpm: u32, max_rpm: u32) -> Result<Self> {
        id.method_arg()?;
        if min_rpm >= max_rpm {
            return Err(LctrlError::InvalidArgument {
                detail: format!("fan RPM range must increase: {min_rpm}..{max_rpm}"),
            });
        }
        Ok(Self {
            id,
            min_rpm,
            max_rpm,
        })
    }

    pub fn rpm_percent(&self, rpm: u32) -> Result<f32> {
        if !(self.min_rpm..=self.max_rpm).contains(&rpm) {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "fan RPM {rpm} is outside {}..={}",
                    self.min_rpm, self.max_rpm
                ),
            });
        }
        Ok((rpm - self.min_rpm) as f32 * 100.0 / (self.max_rpm - self.min_rpm) as f32)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    Standard,
    Silent,
    Performance,
    Custom,
    Unknown(u16),
}

impl FanMode {
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        match raw {
            0 => Self::Standard,
            1 => Self::Silent,
            2 => Self::Performance,
            3 => Self::Custom,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct FanStep {
    pub temperature_deci_c: u16,
    pub rpm: u16,
}

impl FanStep {
    #[must_use]
    pub const fn new(temperature_deci_c: u16, rpm: u16) -> Self {
        Self {
            temperature_deci_c,
            rpm,
        }
    }

    #[must_use]
    pub const fn from_packed(raw: u32) -> Self {
        Self {
            temperature_deci_c: raw as u16,
            rpm: (raw >> 16) as u16,
        }
    }

    #[must_use]
    pub const fn packed(self) -> u32 {
        self.temperature_deci_c as u32 | ((self.rpm as u32) << 16)
    }

    #[must_use]
    pub fn set_bytes(self) -> [u8; 4] {
        let temperature = self.temperature_deci_c.to_le_bytes();
        let rpm = self.rpm.to_le_bytes();
        [temperature[0], temperature[1], rpm[0], rpm[1]]
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FanTable {
    pub fan_id: FanId,
    pub sensor_id: SensorId,
    pub mode: FanMode,
    pub min_temperature_deci_c: u16,
    pub max_temperature_deci_c: u16,
    pub steps: Vec<FanStep>,
    pub raw_fan_table: Vec<u32>,
    pub raw_sensor_table: Vec<u32>,
}

impl FanTable {
    pub fn validate(&self, descriptor: &FanDescriptor) -> Result<()> {
        self.fan_id.method_arg()?;
        self.sensor_id.method_arg()?;
        if self.fan_id != descriptor.id {
            return Err(LctrlError::InvalidArgument {
                detail: "fan table id does not match descriptor".into(),
            });
        }
        if self.min_temperature_deci_c > self.max_temperature_deci_c {
            return Err(LctrlError::InvalidArgument {
                detail: "fan table temperature range is reversed".into(),
            });
        }
        if self.steps.is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "fan table must contain at least one step".into(),
            });
        }
        let mut previous = None;
        for step in &self.steps {
            if !(self.min_temperature_deci_c..=self.max_temperature_deci_c)
                .contains(&step.temperature_deci_c)
            {
                return Err(LctrlError::InvalidArgument {
                    detail: format!(
                        "fan step temperature {} is outside {}..={}",
                        step.temperature_deci_c,
                        self.min_temperature_deci_c,
                        self.max_temperature_deci_c
                    ),
                });
            }
            if previous.is_some_and(|value| step.temperature_deci_c <= value) {
                return Err(LctrlError::InvalidArgument {
                    detail: "fan step temperatures must increase strictly".into(),
                });
            }
            descriptor.rpm_percent(u32::from(step.rpm))?;
            previous = Some(step.temperature_deci_c);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureLocation {
    Cpu,
    Gpu,
    Battery,
    Mainboard,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureSource {
    WmiGamezone,
    WmiFanMetadata,
    BatterySmbus,
    Acpi,
    Sysfs,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TemperatureSensorMetadata {
    pub id: String,
    pub name: String,
    pub source: TemperatureSource,
    pub location: TemperatureLocation,
    pub availability: Availability,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemperatureSensor {
    pub metadata: TemperatureSensorMetadata,
    pub value_c: Option<f32>,
}
