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

/// Decoded read-only view of the dispatcher's `Policy_*` `PowerSlider` values.
///
/// Every field is `None` when the value is absent from the key. The packing
/// and clamping rules are reimplementations of the exact code in
/// `LNVDispatcherService.exe` (see `docs/evidence/re-its-policy-20261105.md`);
/// a value the binary does not decode stays raw instead of being named, and a
/// value that does not decode cleanly is reported through [`ItsPolicy::issues`]
/// rather than being dropped or shortened.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ItsPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plx_set_mode: Option<ItsPlxSetMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_speed_address: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_min_std_fan: Option<ItsApMinStdFan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turbo_value: Option<ItsTurboValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turbo_time: Option<ItsTurboTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epp_turbo: Option<ItsEppTurbo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_bucket: Option<ItsSlotTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cppc: Option<ItsSlotTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cfqc: Option<ItsSlotTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cfqc1: Option<ItsSlotTable>,
    /// Values that could not be decoded, with the raw source kept verbatim.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ItsPolicyIssue>,
}

/// One `Policy_*` value the decode refused to interpret.
///
/// `raw` carries the value exactly as published (the `REG_SZ` text for a
/// table) so nothing is lost when the decode cannot be trusted; it is `None`
/// when the value itself could not be read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ItsPolicyIssue {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    pub detail: String,
}

impl ItsPolicyIssue {
    /// A value was read but does not match the packing the dispatcher implies.
    #[must_use]
    pub fn unparsed(value: &str, raw: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            value: value.to_owned(),
            raw: Some(raw.into()),
            detail: detail.into(),
        }
    }

    /// A value could not be read at all.
    #[must_use]
    pub fn unreadable(value: &str, detail: impl Into<String>) -> Self {
        Self {
            value: value.to_owned(),
            raw: None,
            detail: detail.into(),
        }
    }
}

/// `Policy_PLxSetMode`: one 32-bit value carrying two 16-bit lanes.
///
/// The lane is selected by AC line state: the dispatcher reads
/// `GetSystemPowerStatus`'s `ACLineStatus` byte and takes the low half on AC,
/// the high half on DC. Each lane picks which write procedure applies the PLx
/// limits; the member names inside the vendor union are not published by the
/// binary, so the lanes stay `ac`/`dc` with the raw value beside them.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItsPlxSetMode {
    pub raw: u32,
    /// Low 16 bits of the value — the lane the dispatcher uses on AC.
    pub ac: u16,
    /// High 16 bits of the value — the lane the dispatcher uses on DC.
    pub dc: u16,
}

impl ItsPlxSetMode {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            ac: (raw & 0xffff) as u16,
            dc: (raw >> 16) as u16,
        }
    }
}

/// `Policy_APMinSTDFan` (enable) plus its `_GPU`/`_CPU` thresholds.
///
/// The dispatcher reads each threshold as a `REG_DWORD`, keeps values up to
/// 100 and replaces anything larger with 50; a failed read of the enable value
/// skips both thresholds, so `enable` is what makes the record present.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItsApMinStdFan {
    /// Low byte of the `Policy_APMinSTDFan` DWORD; the binary only tests it.
    pub enable: bool,
    pub gpu_threshold_pct: u8,
    pub cpu_threshold_pct: u8,
}

impl ItsApMinStdFan {
    /// Highest threshold the dispatcher accepts (`cmp $0x64`).
    pub const MAX_THRESHOLD_PCT: u32 = 100;
    /// Value substituted when a stored threshold is above the maximum.
    pub const DEFAULT_THRESHOLD_PCT: u32 = 50;

    /// Apply the dispatcher's own clamp to one stored threshold.
    #[must_use]
    pub const fn clamp_threshold(raw: u32) -> u8 {
        if raw <= Self::MAX_THRESHOLD_PCT {
            raw as u8
        } else {
            Self::DEFAULT_THRESHOLD_PCT as u8
        }
    }

    #[must_use]
    pub const fn from_raws(enable: u32, gpu: u32, cpu: u32) -> Self {
        Self {
            enable: enable & 0xff != 0,
            gpu_threshold_pct: Self::clamp_threshold(gpu),
            cpu_threshold_pct: Self::clamp_threshold(cpu),
        }
    }
}

/// `Policy_TurboValue`: the turbo EPP value.
///
/// The dispatcher compares the value against `0xff` and logs
/// `_Policy_TurboValue_Union.TurboValue == uDefaultEPP` when it matches, so
/// `0xff` means "use the default EPP" rather than a usable EPP step. The
/// dispatcher seeds the field with 10 before reading the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItsTurboValue {
    pub raw: u32,
    /// `true` when the value is the `uDefaultEPP` sentinel (`0xff`).
    pub default_epp: bool,
}

impl ItsTurboValue {
    pub const DEFAULT_EPP: u32 = 0xff;
    /// Value the dispatcher writes into the field before reading the registry.
    pub const SEED: u32 = 10;

    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            default_epp: raw == Self::DEFAULT_EPP,
        }
    }
}

/// `Policy_TurboTime`: four packed bytes (`_Policy_TurboParam_UnionValue`).
///
/// The dispatcher seeds the field with bytes `{3, 3, 3, 5}` before reading.
/// Only two lanes are consumed by the binary: `turbo_time` is compared against
/// `Policy_EPPTurbo` bits 16..24 (the larger of the two wins) and `byte0` feeds
/// a maximum tracker. `byte1`/`byte3` have no reader, so they stay raw.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItsTurboTime {
    pub raw: u32,
    pub byte0: u8,
    pub byte1: u8,
    /// Turbo time; the dispatcher uses `max(this, Policy_EPPTurbo bits 16..24)`.
    pub turbo_time: u8,
    pub byte3: u8,
}

impl ItsTurboTime {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            byte0: (raw & 0xff) as u8,
            byte1: ((raw >> 8) & 0xff) as u8,
            turbo_time: ((raw >> 16) & 0xff) as u8,
            byte3: ((raw >> 24) & 0xff) as u8,
        }
    }
}

/// `Policy_EPPTurbo`: packed turbo/EPP fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItsEppTurbo {
    pub raw: u32,
    /// Bits 24..28; the dispatcher compares this nibble against 5 and 1.
    pub mode_nibble: u8,
    /// Bits 16..24; the same quantity as [`ItsTurboTime::turbo_time`].
    pub turbo_time: u8,
    /// Bit 28; when set the dispatcher logs "not turbo to APM".
    pub not_turbo_to_apm: bool,
}

impl ItsEppTurbo {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            mode_nibble: ((raw >> 24) & 0xf) as u8,
            turbo_time: ((raw >> 16) & 0xff) as u8,
            not_turbo_to_apm: (raw >> 28) & 1 != 0,
        }
    }
}

/// One dispatcher curve table read from a `REG_SZ` value such as
/// `0:15;10:30;25:45;`.
///
/// The dispatcher does not index by the number before the colon: its reader
/// scans the string for decimal numbers and stores them in order into a fixed
/// array, so the `x:y;` text flattens to `[x0, y0, x1, y1, …]`. The tables are
/// opaque firmware curves — this type reports the slots and pairs verbatim and
/// assigns no physical meaning to a position.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ItsSlotTable {
    /// The value exactly as published.
    pub source: String,
    /// Number of slots the dispatcher's array holds for this value.
    pub capacity: usize,
    pub slots: Vec<u32>,
}

impl ItsSlotTable {
    /// Flatten `source` into exactly `capacity` slots.
    ///
    /// A value whose number count differs from what the dispatcher's reader
    /// can hold is refused (with the counts in the message) instead of being
    /// truncated or padded into a shorter table.
    pub fn parse(source: &str, capacity: usize) -> std::result::Result<Self, String> {
        let slots = decimal_fields(source);
        if slots.len() != capacity {
            return Err(format!(
                "expected {capacity} slot value(s), value holds {}",
                slots.len()
            ));
        }
        Ok(Self {
            source: source.to_owned(),
            capacity,
            slots,
        })
    }

    /// The `(x, y)` pairs the number run encodes.
    #[must_use]
    pub fn pairs(&self) -> Vec<(u32, u32)> {
        self.slots
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .collect()
    }
}

/// Decimal runs in `source`, saturating at `u32::MAX` so an oversized value
/// cannot wrap into a plausible number.
fn decimal_fields(source: &str) -> Vec<u32> {
    let mut values = Vec::new();
    let mut current: Option<u64> = None;
    for byte in source.bytes() {
        if byte.is_ascii_digit() {
            let next = current.unwrap_or(0) * 10 + u64::from(byte - b'0');
            current = Some(next.min(u64::from(u32::MAX)));
        } else if let Some(value) = current.take() {
            values.push(value as u32);
        }
    }
    if let Some(value) = current {
        values.push(value as u32);
    }
    values
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
    /// Intel ESIF / Dynamic Tuning participant readback
    /// (`root\WMI EsifDeviceInformation`). The class publishes one
    /// `Temperature` per participant instance and no participant name, so the
    /// sensor identity is the provider instance name and the physical location
    /// stays unknown (docs/evidence/re-thermal-20261105.md).
    WmiEsif,
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

/// GPU overclock support reported by `LENOVO_GAMEZONE_DATA.IsSupportGpuOC`
/// (docs/01 §2.6, docs/03 §5.4).
///
/// Lenovo's `Is*` probes answer through `Data`: `1` means the feature exists,
/// `0` means it does not (`ReturnValue` only reports that the provider ran).
/// Recovered and confirmed on 21VG: `ReturnValue=True, Data=0`, with no
/// instance of either OC parameter class, see
/// `evidence/re-gpu-oc-20261105.md`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GpuOcStatus {
    /// True only when the firmware answers `Data = 1`.
    pub supported: bool,
    /// The probe answer exactly as returned, so a caller can audit the decode.
    pub probe: GpuOcSupportProbe,
    /// OC parameter block, present only when the firmware instantiates
    /// `LENOVO_GAMEZONE_GPU_OC_DATA`.
    pub parameter: Option<GpuOcParameter>,
    /// Why the parameter block is absent (the provider verdict verbatim).
    pub parameter_error: Option<String>,
}

/// One `Is*` support probe call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GpuOcSupportProbe {
    /// Fully qualified method name, e.g. `LENOVO_GAMEZONE_DATA.IsSupportGpuOC`.
    pub method: String,
    /// The WMI `ReturnValue`: the provider executed the method.
    pub return_value: bool,
    /// The WMI `Data` answer: `1` supported, `0` absent.
    pub data: u32,
}

/// One `LENOVO_GAMEZONE_GPU_OC_DATA` row, property names verbatim.
///
/// The class declares these properties and nothing here assigns a unit or a
/// physical meaning to them: the vendor consumer feeds the same numbers to
/// NVAPI clock offsets, which is not established for the WMI field. Values are
/// reported as the firmware publishes them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GpuOcParameter {
    pub capability: u32,
    pub clock_id: u32,
    pub default_value: u32,
    pub interval: u32,
    pub no_oc_order_id: u32,
    pub oc_max_offset: u32,
    pub oc_min_offset: u32,
    pub oc_offset_freq: u32,
    pub oc_offset_scale: u32,
    pub oc_order_id: u32,
    pub pstate_id: u32,
}
