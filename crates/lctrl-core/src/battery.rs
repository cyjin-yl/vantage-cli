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
#[must_use]
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

/// Scalar battery telemetry decoded from the 83-byte battery response
/// (docs/02 §7.1, offsets 0..36). The conflicting string areas (offset 48+)
/// are deliberately not interpreted here.
///
/// Every 16-bit scalar uses `0xFFFF` to mark the field as unsupported; the
/// 32-bit current field uses `0xFFFF_FFFF` (the same sentinel extended).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryTelemetry {
    /// Design capacity in mWh (raw value is in units of 10 mWh).
    pub design_capacity_mwh: Option<u32>,
    /// Full-charge capacity in mWh.
    pub full_charge_capacity_mwh: Option<u32>,
    /// Remaining capacity in mWh.
    pub remaining_capacity_mwh: Option<u32>,
    /// Voltage in mV.
    pub voltage_mv: Option<u16>,
    /// Current in mA; positive charges, negative discharges.
    pub current_ma: Option<i32>,
    /// Temperature in 0.1 K.
    pub temperature_deci_kelvin: Option<u16>,
    /// Manufacture date.
    pub manufacture_date: Option<BatteryDate>,
    /// First-use date.
    pub first_used_date: Option<BatteryDate>,
    /// Design voltage in mV.
    pub design_voltage_mv: Option<u16>,
    /// Remaining charge percentage.
    pub remaining_percent: Option<u16>,
    /// Battery life percentage.
    pub life_percent: Option<u16>,
    /// Charge status; absent when the firmware returns `0xFFFF`.
    pub charge_status: Option<ChargeStatus>,
    /// Remaining runtime in minutes.
    pub remaining_time_min: Option<u16>,
    /// Time to full charge in minutes.
    pub charge_completion_time_min: Option<u16>,
    /// Wattage in W.
    pub wattage_w: Option<u16>,
    /// Charge/discharge cycle count.
    pub cycle_count: Option<u16>,
}

impl BatteryTelemetry {
    /// Parse the scalar region of an 83-byte battery response (docs/02 §7.1).
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

        let date_at = |offset: usize| -> Result<Option<BatteryDate>> {
            let raw = u16_at(offset);
            if raw == u16::MAX {
                Ok(None)
            } else {
                BatteryDate::decode(raw).map(Some)
            }
        };

        let current_raw = i32::from_le_bytes([input[12], input[13], input[14], input[15]]);

        Ok(Self {
            design_capacity_mwh: mwh(u16_at(0)),
            full_charge_capacity_mwh: mwh(u16_at(2)),
            remaining_capacity_mwh: mwh(u16_at(4)),
            voltage_mv: scalar(u16_at(10)),
            current_ma: (current_raw != -1).then_some(current_raw),
            temperature_deci_kelvin: scalar(u16_at(16)),
            manufacture_date: date_at(18)?,
            first_used_date: date_at(20)?,
            design_voltage_mv: scalar(u16_at(22)),
            remaining_percent: scalar(u16_at(24)),
            life_percent: scalar(u16_at(26)),
            charge_status: scalar(u16_at(28)).map(ChargeStatus::decode),
            remaining_time_min: scalar(u16_at(30)),
            charge_completion_time_min: scalar(u16_at(32)),
            wattage_w: scalar(u16_at(34)),
            cycle_count: scalar(u16_at(36)),
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

/// Convert a raw capacity (units of 10 mWh) to mWh; `0xFFFF` is unsupported.
fn mwh(raw: u16) -> Option<u32> {
    (raw != u16::MAX).then_some(u32::from(raw) * 10)
}

/// Keep a scalar u16 unless `0xFFFF` marks it unsupported.
fn scalar(raw: u16) -> Option<u16> {
    (raw != u16::MAX).then_some(raw)
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

/// Power/identity details read from the GAPD response (docs/02 §8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDetailValues {
    /// Product ID (`0xFFFF` = none).
    pub pid: u16,
    /// Vendor ID.
    pub vid: u16,
    /// System power demand in W.
    pub system_power_w: u16,
    /// Actual charger power in W.
    pub current_power_w: u16,
}

impl AdapterDetailValues {
    /// Whether the connected charger delivers less power than the system
    /// demands (docs/02 §8: `CurrentChargerPower < SystemChargerPower`).
    #[must_use]
    pub const fn is_underpowered(&self) -> bool {
        self.current_power_w < self.system_power_w
    }
}

/// Adapter identity and power status derived from the GBMD status word
/// (docs/02 §8).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterInfo {
    pub authentication: AdapterAuthentication,
    /// Whether GBMD status bit 24 advertises a detailed-capable charger.
    pub has_detail: bool,
    /// Detailed power/identity values, present only when the firmware
    /// advertises them (bit 24) and the GAPD read succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<AdapterDetailValues>,
}

impl AdapterInfo {
    /// Build adapter info from the GBMD status word. `detail` is supplied by
    /// the transport only when the bit-24 capability is set (docs/02 §8);
    /// the underpowered flag is computed only when detail is present.
    #[must_use]
    pub fn from_gbmd(status: u32, detail: Option<AdapterDetailValues>) -> Self {
        Self {
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
