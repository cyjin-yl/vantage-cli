//! Battery domain: charge modes, telemetry decoding, and adapter authentication.
//!
//! Pure data and pure logic per `docs/02-power-battery.md`. No Windows API
//! names, IOCTL codes, or raw command bytes appear here; the HAL layer owns
//! all transport-level encoding and capability probing.

use serde::Serialize;

use crate::error::{LctrlError, Result};

/// One unavailable, failed or ambiguous source within a partial observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BatterySourceIssue {
    pub source: &'static str,
    pub detail: String,
}

/// Electrical values from one independently admitted operating-system battery.
/// `rate_mw` is power, not a measured current; relative-rate values are absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryElectricalObservation {
    pub voltage_mv: Option<u32>,
    pub rate_mw: Option<i32>,
    pub power_state: u32,
    pub temperature_deci_kelvin: Option<u32>,
    pub source_issues: Vec<BatterySourceIssue>,
}

/// One identity-bound OS battery observation. Each field may be unavailable.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OsBatterySnapshot {
    pub remaining_capacity_mwh: Option<u32>,
    pub full_charge_capacity_mwh: Option<u32>,
    pub design_capacity_mwh: Option<u32>,
    /// Full/design capacity percentage derived from this same OS battery.
    pub capacity_ratio_percent: Option<u16>,
    pub voltage_mv: Option<u16>,
    pub remaining_percent: Option<u16>,
    pub cycle_count: Option<u16>,
    pub estimated_runtime_min: Option<u16>,
    pub power_online: Option<bool>,
    pub charging: Option<bool>,
    pub discharging: Option<bool>,
    pub critical: Option<bool>,
    /// Per-source failures or ambiguities retained alongside partial data.
    pub source_issues: Vec<BatterySourceIssue>,
}

/// Exact record or node that supplied each raw capacity/cycle number, so a
/// report never presents a number without its own provenance. The labels are
/// filled by the HAL channel that read them; an unavailable field keeps its
/// label because that is still the record which was asked and refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryFieldProvenance {
    pub design_capacity: &'static str,
    pub full_charge_capacity: &'static str,
    pub cycle_count: &'static str,
}

/// Wear and cycle facts derived from one identity-bound battery observation.
///
/// Capacities are milliwatt-hours only. The platform reports absolute `mWh`
/// while `BATTERY_CAPACITY_RELATIVE` (`0x4000_0000`) is clear; relative
/// readings are withheld by the owning channel instead of being relabelled
/// (docs/02 §7.4). No milliampere-hour figure is derived here: that conversion
/// needs a measured terminal voltage, and the only voltage this target exposes
/// is nominal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BatteryWearReport {
    /// The channel that produced these facts. Never merged with another one.
    pub source: &'static str,
    /// Per-field provenance inside that channel.
    pub fields: BatteryFieldProvenance,
    pub design_capacity_mwh: Option<u32>,
    pub full_charge_capacity_mwh: Option<u32>,
    pub cycle_count: Option<u16>,
    /// `floor(100 × full / design)`; remains observable above 100.
    pub capacity_ratio_percent: Option<u16>,
    /// `floor(100 × (design − full) / design)`. Withheld when the capacities
    /// cannot be read as wear: design capacity absent/zero, or full above
    /// design. The two floored percentages need not sum to 100.
    pub wear_percent: Option<u16>,
    /// Why a capacity, cycle or wear value is withheld.
    pub issues: Vec<BatterySourceIssue>,
}

impl BatteryWearReport {
    /// Derive wear facts from capacities that are already in absolute mWh.
    ///
    /// A zero capacity is the documented "unsupported" encoding, so it is
    /// never treated as a measured zero and never divides.
    #[must_use]
    pub fn from_capacities(
        source: &'static str,
        fields: BatteryFieldProvenance,
        design_capacity_mwh: Option<u32>,
        full_charge_capacity_mwh: Option<u32>,
        cycle_count: Option<u16>,
        issues: Vec<BatterySourceIssue>,
    ) -> Self {
        let mut issues = issues;
        let design = design_capacity_mwh.filter(|value| *value > 0);
        let full = full_charge_capacity_mwh.filter(|value| *value > 0);
        if design.is_none() {
            issues.push(BatterySourceIssue {
                source,
                detail: "design capacity is absent or zero; capacity ratio and wear percent are not computed".into(),
            });
        }
        if full.is_none() {
            issues.push(BatterySourceIssue {
                source,
                detail: "full-charge capacity is absent or zero; capacity ratio and wear percent are not computed".into(),
            });
        }
        let capacity_ratio_percent = design.zip(full).and_then(|(design, full)| {
            u16::try_from(u64::from(full) * 100 / u64::from(design)).ok()
        });
        if design.zip(full).is_some() && capacity_ratio_percent.is_none() {
            issues.push(BatterySourceIssue {
                source,
                detail: "capacity ratio is not representable as a percentage".into(),
            });
        }
        let wear_percent = match (design, full) {
            (Some(design), Some(full)) if full <= design => {
                u16::try_from(u64::from(design - full) * 100 / u64::from(design)).ok()
            }
            (Some(_), Some(_)) => {
                issues.push(BatterySourceIssue {
                    source,
                    detail: "full-charge capacity exceeds design capacity; wear percent is undefined".into(),
                });
                None
            }
            _ => None,
        };
        Self {
            source,
            fields,
            design_capacity_mwh: design,
            full_charge_capacity_mwh: full,
            cycle_count,
            capacity_ratio_percent,
            wear_percent,
            issues,
        }
    }

    /// Whether any capacity or cycle fact survived, i.e. whether the report
    /// carries an observation rather than only refusals.
    #[must_use]
    pub const fn has_observation(&self) -> bool {
        self.design_capacity_mwh.is_some()
            || self.full_charge_capacity_mwh.is_some()
            || self.cycle_count.is_some()
    }
}

/// The operating system's aggregate power status.
///
/// It is reported separately and is not attributed to a specific pack or
/// adapter unless an independent identity contract establishes that link.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SystemPowerStatus {
    /// `true` when running on external power, `false` on battery, `None`
    /// when the platform did not report the line state.
    pub ac_line_status: Option<bool>,
    pub battery_flag: Option<u8>,
    pub battery_life_percent: Option<u8>,
    pub battery_life_time: Option<u32>,
    pub battery_full_life_time: Option<u32>,
}

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

/// Runtime USB charging control state, from the EnergyDrv `0x831020e8` flags
/// word (`PowerBattery.dll` `CUSBCharger`/`CUSBBatteryCharger`, RE-proven in
/// docs/interfaces.md §11).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UsbChargingState {
    pub ac_supported: bool,
    pub ac_enabled: bool,
    pub battery_supported: bool,
    pub battery_enabled: bool,
}

impl UsbChargingState {
    /// Decode the vendor flags DWORD. Bit positions are proven by static
    /// disassembly of the exact target binaries; the EC-side meaning of the
    /// capability/state split follows the vendor class names and log strings.
    #[must_use]
    pub const fn from_flags(raw: u32) -> Self {
        Self {
            ac_supported: raw & (1 << 6) != 0,
            ac_enabled: raw & (1 << 7) != 0,
            battery_supported: raw & (1 << 14) != 0,
            battery_enabled: raw & (1 << 15) != 0,
        }
    }
}

impl std::fmt::Display for UsbChargingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ac {} ({}), battery-mode {} ({})",
            if self.ac_enabled { "on" } else { "off" },
            if self.ac_supported {
                "supported"
            } else {
                "unsupported"
            },
            if self.battery_enabled { "on" } else { "off" },
            if self.battery_supported {
                "supported"
            } else {
                "unsupported"
            },
        )
    }
}

/// Night-charge mode state (EnergyDrv `0x83102150`, RE-proven in
/// docs/interfaces.md §11 and docs/evidence/re-battery-adv-20261105.md:
/// support probe input `0x13`; status input `0x11` — bit4 = on; set inputs
/// `0x80000012` = on, `0x12` = off).
///
/// `PowerBattery.dll!CChargingMode::DoesSupportNightChargeMode` (RVA `0xa5c0`)
/// reports supported only when the probe word has `bit0` set **and** the word
/// shifted right by 4 has `bit0` set, so the probe's `bit4` is part of the
/// support signal, not the live state. Live evidence (2026-11-01): probe
/// `0x11` (supported) with status `0x01` (off) on the same machine — only the
/// status query reports the current mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct NightChargeState {
    pub supported: bool,
    pub enabled: bool,
}

impl NightChargeState {
    /// Decode the support-probe response word (`(raw & 1) != 0 &&
    /// ((raw >> 4) & 1) != 0`, mirroring `DoesSupportNightChargeMode`).
    /// The current state comes from the status query.
    #[must_use]
    pub const fn from_probe(raw: u32) -> Self {
        Self {
            supported: raw & 1 != 0 && raw & (1 << 4) != 0,
            enabled: false,
        }
    }

    /// Decode the status-query response word.
    #[must_use]
    pub const fn enabled_from_status(raw: u32) -> bool {
        raw & (1 << 4) != 0
    }
}

/// The temporary charging mode the firmware reports
/// (`PowerBattery.dll!CChargingMode::GetTemporaryMode`, RVA `0xa3d0`).
///
/// Values match `BatteryTemporaryModeType` in the vendor contract
/// (`BatteryManagementContract.dll`: `None=0`, `Permanent=1`, `PlugIn=2`,
/// `DynamicLifeSpan=4`). `PlugIn` and `DynamicLifeSpan` are the temporary
/// modes; `Permanent` is normal charging.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporaryChargeMode {
    Permanent,
    PlugIn,
    DynamicLifeSpan,
}

impl TemporaryChargeMode {
    /// The vendor contract value (`BatteryTemporaryModeType`).
    #[must_use]
    pub const fn contract_value(self) -> u32 {
        match self {
            Self::Permanent => 1,
            Self::PlugIn => 2,
            Self::DynamicLifeSpan => 4,
        }
    }
}

impl std::fmt::Display for TemporaryChargeMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Permanent => "permanent",
            Self::PlugIn => "plug-in",
            Self::DynamicLifeSpan => "dynamic-lifespan",
        })
    }
}

/// Temporary charge-mode observation (EnergyDrv `0x83102148`, BTMC).
///
/// The vendor word is not a plain bitmask: after the `0x13` support probe
/// (`(raw & 1) != 0 && (raw & 0x6) != 0`, `DoesSupportTemporaryMode` RVA
/// `0xa250`), `GetTemporaryMode` sends `0x11` and treats bit0 as the validity
/// flag with the mode in `(status >> 4) & 0x07ff_ffff` — `0` = Permanent,
/// `1` = PlugIn, `2` = DynamicLifeSpan; anything else is `None`. No writer
/// exists for this channel, so this is a read-only observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct TemporaryChargeState {
    pub raw: u32,
    /// The `0x13` probe reports the feature as present/active.
    pub supported: bool,
    /// Bit0 of the `0x11` status word; `false` means the status word carries
    /// no mode (native returns `None`).
    pub valid: bool,
    /// Decoded mode; `None` when the status word is invalid or reports a value
    /// outside `{0, 1, 2}`.
    pub mode: Option<TemporaryChargeMode>,
}

impl TemporaryChargeState {
    /// Decode the `0x13` support-probe word. The status query is only issued
    /// when the probe reports support, mirroring the native early return.
    #[must_use]
    pub const fn from_probe(raw: u32) -> Self {
        Self {
            raw,
            supported: raw & 1 != 0 && raw & 0x6 != 0,
            valid: false,
            mode: None,
        }
    }

    /// Attach the `0x11` status word and decode the mode.
    #[must_use]
    pub const fn with_status(self, status: u32) -> Self {
        let valid = status & 1 != 0;
        let mode = if valid {
            match (status >> 4) & 0x07ff_ffff {
                0 => Some(TemporaryChargeMode::Permanent),
                1 => Some(TemporaryChargeMode::PlugIn),
                2 => Some(TemporaryChargeMode::DynamicLifeSpan),
                _ => None,
            }
        } else {
            None
        };
        Self {
            raw: status,
            supported: self.supported,
            valid,
            mode,
        }
    }

    /// The active mode is one of the two temporary charging modes.
    #[must_use]
    pub const fn is_temporary(&self) -> bool {
        matches!(
            self.mode,
            Some(TemporaryChargeMode::PlugIn | TemporaryChargeMode::DynamicLifeSpan)
        )
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

/// Decode the GBMD `0x831020f8`/`0xFF` status bitmask into the contract
/// charge mode, mirroring `PowerBattery.dll!CChargingMode::GetChargingMode`
/// (RE-proven, docs/interfaces.md §10.3):
///
/// - storage mode is ON when bit 5 is set;
/// - express/Quick mode is ON when bit 2 is set;
/// - anything else reports Normal.
///
/// The vendor polls the same decode after writes (Sleep 50 ms, up to 10
/// tries) before recording the effective mode.
pub fn decode_gbmd_charge_mode(raw: u32) -> ChargeModeActual {
    if raw & (1 << 5) != 0 {
        ChargeModeActual::Conservation
    } else if raw & (1 << 2) != 0 {
        ChargeModeActual::Rapid
    } else {
        ChargeModeActual::Normal
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
    /// Per-source failures retained alongside otherwise usable observations.
    pub source_issues: Vec<BatterySourceIssue>,
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
        // Offset 0x0a carries an inferred vendor status/alert word, not one
        // of the documented ChargeStatus codes (docs/01 §3.3). Decoding it as
        // an enum produced meaningless values such as `unknown (17714)`, so
        // the charge status is left to the operating-system channel, which
        // reports the actual charging and discharging flags.
        let status: Option<ChargeStatus> = None;
        // The target specification identifies 0x14 as nominal voltage, not
        // present terminal voltage. The 0x0e/0x10 temperature/current
        // interpretations remain inferred and cannot be exposed as facts.
        let design_voltage_mv = scalar(u16_at(0x14));

        Ok(Self {
            design_capacity_mwh: mwh(u16_at(0x00)),
            full_charge_capacity_mwh: mwh(u16_at(0x02)),
            remaining_capacity_mwh: mwh(u16_at(0x04)),
            voltage_mv: None,
            current_ma: None,
            temperature_deci_kelvin: None,
            manufacture_date: None,
            first_used_date: None,
            design_voltage_mv,
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
            source_issues: Vec::new(),
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

    /// Temperature in degrees Celsius: `t / 10 − 273.15` (docs/02 §7.1).
    #[must_use]
    pub fn temperature_celsius(&self) -> Option<f32> {
        self.temperature_deci_kelvin
            .map(|t| (f64::from(t) / 10.0 - 273.15) as f32)
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

    /// Whether the charger is underpowered, or unknown without adapter detail.
    #[must_use]
    pub const fn is_underpowered(&self) -> Option<bool> {
        match self.detail {
            Some(detail) => Some(detail.is_underpowered()),
            None => None,
        }
    }
}

/// One read-only field the platform cannot observe, with the exact reason.
///
/// A missing number is reported here instead of being replaced by a nominal
/// rating, a design value, or a value copied from another source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PowerFieldUnavailable {
    pub field: &'static str,
    pub reason: &'static str,
}

/// Read-only AC-input view.
///
/// The AC line state is the operating system's aggregate answer, while
/// [`AdapterInfo`] is the firmware's own answer. They are reported separately
/// and never merged; the HAL/CLI composition root supplies each channel name.
/// Adapter voltage, adapter current and a rated wattage have no read-only
/// channel on the target SKU and stay absent with a recorded reason.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AcPowerObservation {
    pub ac_line_status: Option<bool>,
    /// Exact channel that supplied `ac_line_status`.
    pub ac_line_source: &'static str,
    pub adapter: Option<AdapterInfo>,
    /// Exact channel asked for the adapter observation.
    pub adapter_source: &'static str,
    /// Whether the firmware advertises detailed adapter power. `None` when no
    /// adapter observation was obtained.
    pub adapter_detail_advertised: Option<bool>,
    /// Firmware-reported instantaneous adapter output power, only when the
    /// detail gate was open.
    pub adapter_current_power_w: Option<u16>,
    /// Firmware-reported system power requirement, only when the detail gate
    /// was open.
    pub adapter_system_power_w: Option<u16>,
    pub unavailable: Vec<PowerFieldUnavailable>,
    pub source_issues: Vec<BatterySourceIssue>,
}

impl AcPowerObservation {
    /// Compose the AC view without inventing a value for a missing source.
    #[must_use]
    pub fn from_sources(
        ac_line_status: Option<bool>,
        ac_line_source: &'static str,
        adapter: Option<AdapterInfo>,
        adapter_source: &'static str,
    ) -> Self {
        let mut unavailable = Vec::new();
        let mut source_issues = Vec::new();
        if ac_line_status.is_none() {
            source_issues.push(BatterySourceIssue {
                source: ac_line_source,
                detail: "ACLineStatus was not reported; AC line state is unknown".into(),
            });
        }
        let (adapter_detail_advertised, adapter_current_power_w, adapter_system_power_w) =
            match &adapter {
                Some(info) => {
                    let detail = info.detail;
                    if detail.is_none() {
                        unavailable.push(PowerFieldUnavailable {
                            field: "adapter_power_detail",
                            reason: if info.has_detail {
                                "the firmware advertised detailed adapter power but the adapter channel returned no detail values"
                            } else {
                                "the firmware advertises no detailed adapter power; the detailed query is not issued"
                            },
                        });
                    }
                    if info.ac_connected.is_none() {
                        unavailable.push(PowerFieldUnavailable {
                            field: "adapter_ac_connected",
                            reason: "the adapter object does not assert a connection without detailed power; AC line state is reported separately",
                        });
                    }
                    if info.status.is_none() {
                        unavailable.push(PowerFieldUnavailable {
                            field: "adapter_status",
                            reason: "the adapter status word does not carry Full/Limited/None without detailed power",
                        });
                    }
                    if info.connector_type.is_none() {
                        unavailable.push(PowerFieldUnavailable {
                            field: "adapter_connector_type",
                            reason: "no verified read-only connector-type channel exists; it is not inferred from the adapter",
                        });
                    }
                    (
                        Some(info.has_detail),
                        // The adapter detail documents 0 as an unknown/absent power value.
                        detail.and_then(|values| {
                            (values.current_power_w > 0).then_some(values.current_power_w)
                        }),
                        detail.and_then(|values| {
                            (values.system_power_w > 0).then_some(values.system_power_w)
                        }),
                    )
                }
                None => {
                    unavailable.push(PowerFieldUnavailable {
                        field: "adapter",
                        reason: "the adapter channel returned no observation",
                    });
                    (None, None, None)
                }
            };
        unavailable.extend([
            PowerFieldUnavailable {
                field: "adapter_voltage_mv",
                reason: "no read-only adapter voltage channel exists; the adapter detail exposes power only",
            },
            PowerFieldUnavailable {
                field: "adapter_current_ma",
                reason: "no read-only adapter current channel exists; the adapter detail exposes power only",
            },
            PowerFieldUnavailable {
                field: "rated_adapter_wattage",
                reason: "the adapter channel reports instantaneous current power, not a rated wattage",
            },
            PowerFieldUnavailable {
                field: "usb_c_power_delivery",
                reason: "no documented user-mode USB-C power-delivery contract API exists on this platform",
            },
        ]);
        Self {
            ac_line_status,
            ac_line_source,
            adapter,
            adapter_source,
            adapter_detail_advertised,
            adapter_current_power_w,
            adapter_system_power_w,
            unavailable,
            source_issues,
        }
    }
}

/// Direction of the signed firmware battery rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryRateState {
    Discharging,
    Charging,
    /// The firmware reported a measured zero rate.
    Zero,
    /// The firmware reported no absolute rate (unknown or relative).
    NotReported,
}

/// Read-only instantaneous battery-pack power flow.
///
/// `rate_mw` is the raw signed firmware value: positive charges, negative
/// discharges. `draw_mw`/`charge_mw` are derived only from the sign of that
/// same admitted reading. A measured zero stays a zero, never a draw; an
/// absent rate stays absent. This is a pack measurement, not wall input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PowerDrawObservation {
    pub rate_mw: Option<i32>,
    pub voltage_mv: Option<u32>,
    pub power_state_bits: u32,
    pub state: BatteryRateState,
    pub draw_mw: Option<u32>,
    pub charge_mw: Option<u32>,
    pub ac_line_status: Option<bool>,
    /// Exact channel that supplied `ac_line_status`.
    pub ac_line_source: &'static str,
    /// Exact channel that supplied `rate_mw` and `voltage_mv`.
    pub source: &'static str,
    pub unavailable: Vec<PowerFieldUnavailable>,
    pub source_issues: Vec<BatterySourceIssue>,
}

impl PowerDrawObservation {
    /// Project one admitted electrical reading. `source` is the exact channel
    /// supplied by the HAL/CLI composition root; no value is carried over from
    /// the vendor telemetry channel or from the CIM battery class.
    #[must_use]
    pub fn from_sources(
        electrical: &BatteryElectricalObservation,
        ac_line_status: Option<bool>,
        ac_line_source: &'static str,
        source: &'static str,
    ) -> Self {
        let rate = electrical.rate_mw;
        let (state, draw_mw, charge_mw) = match rate {
            Some(value) if value < 0 => (
                BatteryRateState::Discharging,
                Some(value.unsigned_abs()),
                None,
            ),
            Some(value) if value > 0 => (
                BatteryRateState::Charging,
                None,
                Some(value.unsigned_abs()),
            ),
            Some(_) => (BatteryRateState::Zero, None, None),
            None => (BatteryRateState::NotReported, None, None),
        };
        let mut source_issues = electrical.source_issues.clone();
        if rate.is_none() {
            source_issues.push(BatterySourceIssue {
                source,
                detail: "the electrical channel reported no absolute rate; no draw or charge is computed"
                    .into(),
            });
        }
        if electrical.voltage_mv.is_none() {
            source_issues.push(BatterySourceIssue {
                source,
                detail: "the electrical channel reported no terminal voltage".into(),
            });
        }
        if ac_line_status == Some(true) && rate == Some(0) {
            source_issues.push(BatterySourceIssue {
                source: "AC line + electrical channel",
                detail: "on AC with a measured zero battery rate; this platform exposes no wall-input or adapter-output power meter, so system input power is not asserted".into(),
            });
        }
        Self {
            rate_mw: rate,
            voltage_mv: electrical.voltage_mv,
            power_state_bits: electrical.power_state,
            state,
            draw_mw,
            charge_mw,
            ac_line_status,
            ac_line_source,
            source,
            unavailable: vec![
                PowerFieldUnavailable {
                    field: "wall_input_power",
                    reason: "no documented wall-input or whole-system power sensor/API exists on this platform",
                },
                PowerFieldUnavailable {
                    field: "adapter_output_power",
                    reason: "adapter output power is a separate channel owned by `power ac`",
                },
                PowerFieldUnavailable {
                    field: "ac_adapter_voltage",
                    reason: "no read-only adapter voltage channel exists",
                },
                PowerFieldUnavailable {
                    field: "ac_adapter_current",
                    reason: "no read-only adapter current channel exists",
                },
            ],
            source_issues,
        }
    }
}
