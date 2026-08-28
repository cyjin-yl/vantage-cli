//! Battery domain: charge modes, telemetry decoding, and adapter authentication.
//!
//! Pure data and pure logic per `docs/02-power-battery.md`. No Windows API
//! names, IOCTL codes, or raw command bytes appear here; the HAL layer owns
//! all transport-level encoding and capability probing.

use serde::Serialize;

use crate::error::{LctrlError, Result};

/// A user-selected charging mode (`BatteryChargeModeType`, docs/02 §3.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeMode {
    Normal,
    Conservation,
    Rapid,
}

impl std::fmt::Display for ChargeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => f.write_str("normal"),
            Self::Conservation => f.write_str("conservation"),
            Self::Rapid => f.write_str("rapid"),
        }
    }
}

/// The mode read back from the firmware as a Storage/Express bitmask
/// (docs/02 §3.3). Unrecognized combinations are preserved, not remapped.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeModeActual {
    Normal,
    Conservation,
    Rapid,
    /// Both Storage and Express bits set: abnormal combination (docs/02 §3.3).
    Conflict,
    /// An unrecognized bit combination, retained verbatim.
    Unknown(u32),
}

impl std::fmt::Display for ChargeModeActual {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => f.write_str("normal"),
            Self::Conservation => f.write_str("conservation"),
            Self::Rapid => f.write_str("rapid"),
            Self::Conflict => f.write_str("conflict"),
            Self::Unknown(raw) => write!(f, "unknown ({raw})"),
        }
    }
}

/// Decode a raw charge-mode bitmask (docs/02 §3.3).
///
/// `u32::MAX` is the firmware's read-failure sentinel and is reported as an
/// unavailable channel rather than being confused with an unknown mode.
pub fn decode_charge_mode(raw: u32) -> Result<ChargeModeActual> {
    match raw {
        0 => Ok(ChargeModeActual::Normal),
        1 => Ok(ChargeModeActual::Conservation),
        2 => Ok(ChargeModeActual::Rapid),
        3 => Ok(ChargeModeActual::Conflict),
        u32::MAX => Err(LctrlError::ChannelUnavailable {
            channel: "battery charge mode".into(),
        }),
        other => Ok(ChargeModeActual::Unknown(other)),
    }
}

/// One semantic charging action: open or close a feature (docs/02 §3.3).
///
/// Deliberately carries no raw GBMD subcommand byte; the HAL layer maps each
/// primitive to the verified subcommand sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargePrimitive {
    /// Toggle conservation (storage) mode.
    Conservation(bool),
    /// Toggle rapid-charge (express) mode.
    Rapid(bool),
}

/// The two-step transition plan for a target mode. The opposite mode is
/// always disabled before the target mode is enabled (docs/02 §3.3):
/// mutual-exclusion is guaranteed by construction.
#[must_use]
pub fn plan_charge_mode(target: ChargeMode) -> [ChargePrimitive; 2] {
    match target {
        ChargeMode::Normal => [
            ChargePrimitive::Conservation(false),
            ChargePrimitive::Rapid(false),
        ],
        ChargeMode::Conservation => [
            ChargePrimitive::Rapid(false),
            ChargePrimitive::Conservation(true),
        ],
        ChargeMode::Rapid => [
            ChargePrimitive::Conservation(false),
            ChargePrimitive::Rapid(true),
        ],
    }
}

/// A battery date decoded from the bitfield
/// `day[4:0] | month[8:5] | (year − 1980)[15:9]` (docs/02 §7.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl BatteryDate {
    /// Decode the date bitfield. `0xFFFF` marks the field as unsupported;
    /// out-of-range month/day bit patterns are rejected rather than mapped.
    pub fn decode(raw: u16) -> Result<Self> {
        if raw == u16::MAX {
            return Err(LctrlError::InvalidArgument {
                detail: "battery date 0xffff marks an unsupported field".into(),
            });
        }
        let day = (raw & 0x1f) as u8;
        let month = ((raw >> 5) & 0x0f) as u8;
        let year = 1980 + (raw >> 9);
        if day == 0 || month == 0 || month > 12 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("battery date bitfield {raw:#06x} is out of range"),
            });
        }
        Ok(Self { year, month, day })
    }
}

impl std::fmt::Display for BatteryDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Charge status mapping (docs/02 §7.2). Unrecognized codes are preserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeStatus {
    NoActivity,
    Charging,
    Discharging,
    DischargingWithAc,
    Error,
    Detached,
    /// An unrecognized status code, retained verbatim.
    Unknown(u16),
}

impl std::fmt::Display for ChargeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoActivity => f.write_str("no activity"),
            Self::Charging => f.write_str("charging"),
            Self::Discharging => f.write_str("discharging"),
            Self::DischargingWithAc => f.write_str("discharging with ac"),
            Self::Error => f.write_str("error"),
            Self::Detached => f.write_str("detached"),
            Self::Unknown(raw) => write!(f, "unknown ({raw})"),
        }
    }
}

impl ChargeStatus {
    #[must_use]
    pub fn decode(raw: u16) -> Self {
        match raw {
            0 => Self::NoActivity,
            1 => Self::Charging,
            2 => Self::Discharging,
            3 => Self::DischargingWithAc,
            4 => Self::Error,
            5 => Self::Detached,
            other => Self::Unknown(other),
        }
    }
}

/// Battery health mapping (docs/02 §7.2). Anything outside `1..=5` is the
/// documented error state and retains the raw code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryHealth {
    Green,
    Yellow,
    Red,
    Invalid,
    NotInstalled,
    /// Firmware error state; the raw health code is preserved.
    Error(u16),
}

impl std::fmt::Display for BatteryHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Green => f.write_str("green"),
            Self::Yellow => f.write_str("yellow"),
            Self::Red => f.write_str("red"),
            Self::Invalid => f.write_str("invalid"),
            Self::NotInstalled => f.write_str("not installed"),
            Self::Error(raw) => write!(f, "error ({raw})"),
        }
    }
}

impl BatteryHealth {
    #[must_use]
    pub fn decode(raw: u16) -> Self {
        match raw {
            1 => Self::Green,
            2 => Self::Yellow,
            3 => Self::Red,
            4 => Self::Invalid,
            5 => Self::NotInstalled,
            other => Self::Error(other),
        }
    }
}

/// Battery telemetry decoded from the verified EnergyDrv scalar region or
/// native platform battery interfaces. Unavailable identity fields remain
/// explicit `None` values rather than being omitted from the contract.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryTelemetry {
    /// Design capacity in mWh (EnergyDrv raw value is in units of 10 mWh).
    pub design_capacity_mwh: Option<u32>,
    pub full_charge_capacity_mwh: Option<u32>,
    pub remaining_capacity_mwh: Option<u32>,
    pub voltage_mv: Option<u16>,
    /// Current in mA; positive charges, negative discharges.
    pub current_ma: Option<i32>,
    /// Temperature in 0.1 K.
    pub temperature_deci_kelvin: Option<u16>,
    pub manufacture_date: Option<BatteryDate>,
    pub first_used_date: Option<BatteryDate>,
    pub design_voltage_mv: Option<u16>,
    pub remaining_percent: Option<u16>,
    pub life_percent: Option<u16>,
    pub charge_status: Option<ChargeStatus>,
    pub health: Option<BatteryHealth>,
    pub remaining_time_min: Option<u16>,
    pub charge_completion_time_min: Option<u16>,
    pub wattage_w: Option<u16>,
    pub cycle_count: Option<u16>,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub chemistry: Option<String>,
}

impl BatteryTelemetry {
    /// Parse the scalar and fixed-width identity regions of an 83-byte response.
    pub fn parse(input: &[u8]) -> Result<Self> {
        if input.len() != 83 {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "battery telemetry requires the 83-byte battery response; got {} bytes",
                    input.len()
                ),
            });
        }

        let u16_at = |offset: usize| u16::from_le_bytes([input[offset], input[offset + 1]]);

        // The target's verified EnergyDrv layout (docs/01 §3.3) differs from
        // the generic BATTERY_INFORMATION_EX sketch in docs/02. Keep the
        // target offsets here; never infer dates or percentages from tail bytes.
        let status = scalar(u16_at(0x0a)).map(ChargeStatus::decode);
        let temperature = scalar(u16_at(0x0e));
        let current_ma = signed_scalar(u16_at(0x10));
        let voltage_mv = scalar(u16_at(0x14));

        Ok(Self {
            design_capacity_mwh: mwh(u16_at(0x00)),
            full_charge_capacity_mwh: mwh(u16_at(0x02)),
            remaining_capacity_mwh: mwh(u16_at(0x04)),
            voltage_mv,
            current_ma,
            temperature_deci_kelvin: temperature,
            manufacture_date: None,
            first_used_date: None,
            design_voltage_mv: None,
            remaining_percent: None,
            life_percent: None,
            charge_status: status,
            health: None,
            remaining_time_min: None,
            charge_completion_time_min: None,
            wattage_w: None,
            cycle_count: None,
            manufacturer: fixed_text(input, 0x28, 0x0c)?,
            model_name: None,
            firmware_version: None,
            serial_number: fixed_text(input, 0x34, 0x18)?,
            chemistry: fixed_text(input, 0x16, 0x12)?,
        })
    }

    /// Whether rapid charge is permitted by the 39 Wh small-battery safety
    /// policy (docs/02 §3.5). A design capacity of exactly 39000 mWh disables
    /// it. Missing capacity fails closed because the safety guard cannot be
    /// evaluated before a high-current write.
    #[must_use]
    pub fn rapid_charge_allowed(&self) -> bool {
        self.design_capacity_mwh
            .is_some_and(|capacity| capacity != 39_000)
    }

    /// Temperature in degrees Celsius: `(t − 2731.6) / 10` (docs/02 §7.1).
    #[must_use]
    pub fn temperature_celsius(&self) -> Option<f32> {
        self.temperature_deci_kelvin
            .map(|t| (f32::from(t) - 2731.6) / 10.0)
    }
}

fn fixed_text(input: &[u8], offset: usize, width: usize) -> Result<Option<String>> {
    let end = offset
        .checked_add(width)
        .ok_or_else(|| LctrlError::FirmwareRejected {
            detail: "battery identity field offset overflow".into(),
        })?;
    let bytes = input
        .get(offset..end)
        .ok_or_else(|| LctrlError::FirmwareRejected {
            detail: format!("battery identity field {offset}..{end} exceeds response"),
        })?;
    let trimmed = bytes
        .split(|byte| *byte == 0 || *byte == u8::MAX)
        .next()
        .unwrap_or_default();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if let Ok(value) = std::str::from_utf8(trimmed) {
        let value = value.trim();
        return Ok((!value.is_empty()).then(|| value.to_owned()));
    }
    if trimmed.len() % 2 != 0 {
        return Err(LctrlError::FirmwareRejected {
            detail: format!("battery identity field {offset} is neither UTF-8 nor UTF-16LE"),
        });
    }
    let units = trimmed
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let value = String::from_utf16(&units).map_err(|error| LctrlError::FirmwareRejected {
        detail: format!("battery identity field {offset} has invalid UTF-16LE: {error}"),
    })?;
    let value = value.trim();
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

/// Convert a raw capacity (units of 10 mWh) to mWh; `0xFFFF` is unsupported.
fn mwh(raw: u16) -> Option<u32> {
    (raw != u16::MAX).then_some(u32::from(raw) * 10)
}

/// Keep a scalar u16 unless `0xFFFF` marks it unsupported.
fn scalar(raw: u16) -> Option<u16> {
    (raw != u16::MAX).then_some(raw)
}

/// Decode the target's signed 16-bit current field; `0xFFFF` is unsupported.
fn signed_scalar(raw: u16) -> Option<i32> {
    (raw != u16::MAX).then_some(i32::from(i16::from_le_bytes(raw.to_le_bytes())))
}

/// Adapter type from the GBMD status word bits 15..16 (docs/02 §3.2, §8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterAuthentication {
    Inbox,
    Lenovo,
    Unknown,
    SlowCharger,
}

impl std::fmt::Display for AdapterAuthentication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inbox => f.write_str("inbox"),
            Self::Lenovo => f.write_str("lenovo"),
            Self::Unknown => f.write_str("unknown"),
            Self::SlowCharger => f.write_str("slow charger"),
        }
    }
}

impl AdapterAuthentication {
    /// Decode the 2-bit adapter type from the GBMD status word.
    #[must_use]
    pub fn from_gbmd(status: u32) -> Self {
        match (status >> 15) & 0b11 {
            0 => Self::Inbox,
            1 => Self::Lenovo,
            2 => Self::Unknown,
            _ => Self::SlowCharger,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    Full,
    Limited,
    Disconnected,
    UnsupportedDetection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterConnectorType {
    UsbC,
    Legacy,
}

/// Power/identity details read from the GAPD response (docs/02 §8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDetailValues {
    /// Product ID (`0xFFFF` maps to `None`).
    pub pid: Option<u16>,
    /// Vendor ID (`0xFFFF` maps to `None`).
    pub vid: Option<u16>,
    pub system_power_w: u16,
    pub current_power_w: u16,
}

impl AdapterDetailValues {
    #[must_use]
    pub const fn is_underpowered(&self) -> bool {
        self.current_power_w < self.system_power_w
    }
}

/// Adapter identity and power status derived from verified platform channels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterInfo {
    pub ac_connected: Option<bool>,
    pub status: Option<AdapterStatus>,
    pub connector_type: Option<AdapterConnectorType>,
    pub wattage_w: Option<u16>,
    pub authentication: AdapterAuthentication,
    /// Whether GBMD status bit 24 advertises a detailed-capable charger.
    pub has_detail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<AdapterDetailValues>,
}

impl AdapterInfo {
    #[must_use]
    pub fn from_gbmd(status: u32, detail: Option<AdapterDetailValues>) -> Self {
        let ac_connected = detail.map(|detail| detail.pid.is_some());
        let adapter_status = detail.map(|detail| {
            if detail.pid.is_none() {
                AdapterStatus::Disconnected
            } else if detail.is_underpowered() {
                AdapterStatus::Limited
            } else {
                AdapterStatus::Full
            }
        });
        let wattage_w = detail
            .and_then(|detail| (detail.current_power_w > 0).then_some(detail.current_power_w));
        Self {
            ac_connected,
            status: adapter_status,
            connector_type: None,
            wattage_w,
            authentication: AdapterAuthentication::from_gbmd(status),
            has_detail: (status >> 24) & 1 == 1,
            detail,
        }
    }

    /// Whether the charger is underpowered; `false` when no detail is present.
    #[must_use]
    pub const fn is_underpowered(&self) -> bool {
        match self.detail {
            Some(detail) => detail.is_underpowered(),
            None => false,
        }
    }
}
