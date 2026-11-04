use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Availability, LctrlError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceMode {
    Balanced,
    Quiet,
    Performance,
    Geek,
    SilentHighPerformance,
    Custom,
}

impl PerformanceMode {
    #[must_use]
    pub const fn its_value(self) -> u32 {
        match self {
            Self::Balanced => 1,
            Self::Quiet => 2,
            Self::Performance => 3,
            Self::Geek => 4,
            Self::SilentHighPerformance | Self::Custom => 0,
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
            Self::SilentHighPerformance => "silent-high-performance",
            Self::Custom => "custom",
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
            PerformanceMode::SilentHighPerformance | PerformanceMode::Custom => return false,
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
    /// Raw `PowerSlider` DWORDs as published by the installed dispatcher.
    ///
    /// The newer ITS-era schema replaces the older
    /// `VERSION`/`CURRENT_SETTING`/`POWER_SLIDER` names with `ITS_*` names
    /// whose capability bit assignments are not yet attributed to modes. This
    /// block carries those values verbatim so a report never hides what the
    /// machine actually published; `None` when the key predates that schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub its: Option<ItsPowerSlider>,
}

/// The ITS-era `PowerSlider` value set. Every field is `None` when the value
/// is absent from the key, so a partially-populated key stays representable
/// instead of failing the whole read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ItsPowerSlider {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_setting: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_setting_v: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic_mode_setting: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hw_capability: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fn_capability: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_mode: Option<u32>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

impl fmt::Display for FanMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => formatter.write_str("standard"),
            Self::Silent => formatter.write_str("silent"),
            Self::Performance => formatter.write_str("performance"),
            Self::Custom => formatter.write_str("custom"),
            Self::Unknown(raw) => write!(formatter, "unknown ({raw})"),
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
    WindowsBatteryClass,
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

/// Battery direct-discharge current-limit (DBDC) table reported by the vendor
/// `LENOVO_REPORT_DBDC_DATA` WMI class (docs/03 §5.3).
///
/// Semantics: when battery charge falls at or below `threshold_pct[i]`, the EC
/// limits battery discharge current to `current_limit_ma[i]` and system power
/// draw to `ros_power_mw[i]`. Thresholds are ordered high to low; tier count is
/// `counts` and all arrays are expected to agree in length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DbdcProfile {
    pub counts: u32,
    /// Discharge current limit per tier, mA.
    pub current_limit_ma: Vec<u32>,
    /// System available power per tier, mW.
    pub ros_power_mw: Vec<u32>,
    /// Battery charge percentage threshold per tier, high to low.
    pub threshold_pct: Vec<u32>,
}

impl DbdcProfile {
    /// True when the table is internally consistent (equal-length arrays and a
    /// matching tier count). A malformed vendor response is surfaced as an
    /// error by readers, not silently normalised.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.current_limit_ma.len() == self.counts as usize
            && self.ros_power_mw.len() == self.counts as usize
            && self.threshold_pct.len() == self.counts as usize
    }
}
