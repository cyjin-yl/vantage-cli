//! Operating-system sensors: identity, one read per sensor, and the freshness rule.
//!
//! The only sensor channel verified on the reference target is the Windows
//! Sensor API (`SensorsApi.dll`, `CLSID_SensorManager`); the recovered contract,
//! the live inventory of this SKU, and why every absent sensor is reported as
//! absent instead of as a zero reading live in
//! `docs/evidence/re-sensors-20261105.md`.
//!
//! Two rules are structural here rather than per-platform:
//!
//! * a descriptor is built only from what the platform published; a property the
//!   platform rejected stays `None` (`RANGE_MINIMUM`, `RESOLUTION`, … return
//!   `ERROR_NOT_FOUND` on every sensor of the reference target),
//! * a reading is decided by the report timestamp and the sensor's minimum
//!   report interval, never by the value. An ambient-light sensor reporting
//!   `0 lx` in a dark room is a reading; a sensor that returns no report at all
//!   is "no data" and must not become `0`.

use serde::{Serialize, Serializer};

use crate::{LctrlError, Result};

/// A sensor category or type identifier, stored in canonical (network) byte
/// order so the same value prints the same way on every platform.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SensorGuid([u8; 16]);

impl SensorGuid {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Build from the text form `AABBCCDD-EEFF-1122-3344-556677889900`, which is
    /// also the byte order used on the wire.
    #[must_use]
    pub const fn from_parts(high: u128) -> Self {
        Self(high.to_be_bytes())
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }

    /// Canonical braced form, upper case, the spelling Windows uses for the same
    /// identifier in device paths and registry keys.
    #[must_use]
    pub fn to_canonical(self) -> String {
        use std::fmt::Write as _;

        let mut out = String::with_capacity(Self::CANONICAL_LEN);
        out.push('{');
        for (index, byte) in self.0.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                out.push('-');
            }
            // Writing into a String cannot fail.
            let _ = write!(out, "{byte:02X}");
        }
        out.push('}');
        out
    }

    const CANONICAL_LEN: usize = 38;
}

impl std::fmt::Display for SensorGuid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_canonical())
    }
}

impl Serialize for SensorGuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_canonical())
    }
}

/// `SensorState` from the Sensor API, with the documented numeric values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SensorState {
    /// `SENSOR_STATE_READY`
    Ready,
    /// `SENSOR_STATE_NOT_AVAILABLE`
    NotAvailable,
    /// `SENSOR_STATE_NO_DATA`
    NoData,
    /// `SENSOR_STATE_INITIALIZING`
    Initializing,
    /// `SENSOR_STATE_ACCESS_DENIED`
    AccessDenied,
    /// `SENSOR_STATE_ERROR`
    Error,
    /// A value the platform returned that is outside the documented range.
    Unknown(i32),
}

impl SensorState {
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Ready,
            1 => Self::NotAvailable,
            2 => Self::NoData,
            3 => Self::Initializing,
            4 => Self::AccessDenied,
            5 => Self::Error,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NotAvailable => "not_available",
            Self::NoData => "no_data",
            Self::Initializing => "initializing",
            Self::AccessDenied => "access_denied",
            Self::Error => "error",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// One value as the platform delivered it. `Unsupported` keeps the raw vartype so
/// an unexpected type is visible instead of being coerced into a number.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SensorValue {
    Real { value: f64 },
    Integer { value: i64 },
    Unsigned { value: u64 },
    Boolean { value: bool },
    Text { value: String },
    /// UTC timestamp rendered from a `VT_FILETIME` value.
    Timestamp { utc: String, filetime: u64 },
    Guid { value: SensorGuid },
    /// The platform returned `VT_EMPTY`/`VT_NULL` for a supported field.
    Empty,
    /// A vartype this build does not decode; `raw_hex` is the 16-byte payload.
    Unsupported { vartype: u32, raw_hex: String },
}

/// One supported data field of a sensor, keyed by the platform's property key.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SensorField {
    /// Canonical `{FMTID},pid` key, so an unnamed vendor field stays addressable.
    pub key: String,
    /// Standard name when the key is one of the documented data types.
    pub name: Option<&'static str>,
    pub value: SensorValue,
}

impl SensorField {
    /// Human label: the documented name when there is one, else the raw key.
    #[must_use]
    pub fn label(&self) -> &str {
        self.name.unwrap_or(&self.key)
    }
}

/// Identity of one enumerated sensor. Every field except the index and the
/// category/type identifiers is `Option` because the platform may reject the
/// query; nothing is defaulted.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SensorDescriptor {
    /// Position in this enumeration. Stable within one process run only.
    pub index: usize,
    pub friendly_name: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    /// The device-interface instance path, which carries the sensor's own
    /// reference string (the true identity of the instance).
    pub device_path: Option<String>,
    pub category: SensorGuid,
    pub category_name: Option<&'static str>,
    pub sensor_type: SensorGuid,
    pub sensor_type_name: Option<&'static str>,
    pub state: SensorState,
    pub min_report_interval_ms: Option<u32>,
    pub data_field_count: usize,
    /// Why this sensor produced no report, when it did not. Left empty for a
    /// sensor whose report was readable; never used to imply a value.
    pub report_status: Option<String>,
}

impl SensorDescriptor {
    /// Label used by `sensor list` and by ambiguity reports.
    #[must_use]
    pub fn label(&self) -> String {
        match (&self.friendly_name, &self.device_path) {
            (Some(name), _) => name.clone(),
            (None, Some(path)) => path.clone(),
            (None, None) => format!("sensor#{}", self.index),
        }
    }
}

/// How old a report is, and whether that counts as a live reading.
///
/// The window is a declared policy, not a hardware constant: a report is fresh
/// while it is no older than four minimum report intervals or two seconds,
/// whichever is longer, and no more than one second in the future (device clocks
/// run ahead of the wall clock on this target). Outside the window the age is
/// still reported, so a stale value is visible as stale rather than as current.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SensorFreshness {
    /// `now - report_timestamp`; negative when the device clock is ahead.
    pub age_ms: Option<i64>,
    pub stale_after_ms: u32,
    pub fresh: bool,
}

impl SensorFreshness {
    /// One second of tolerance for a device clock running slightly ahead.
    pub const FUTURE_TOLERANCE_MS: i64 = 1_000;
    /// Floor for the staleness window, so a 10 ms sensor is not declared stale
    /// between two reads taken by a human.
    pub const MIN_STALE_AFTER_MS: u32 = 2_000;

    #[must_use]
    pub fn stale_after_ms(min_report_interval_ms: Option<u32>) -> u32 {
        let window = min_report_interval_ms
            .unwrap_or(0)
            .saturating_mul(4)
            .max(Self::MIN_STALE_AFTER_MS);
        window
    }

    #[must_use]
    pub fn between(
        now_ms: Option<i64>,
        report_ms: Option<i64>,
        min_report_interval_ms: Option<u32>,
    ) -> Self {
        let stale_after_ms = Self::stale_after_ms(min_report_interval_ms);
        match (now_ms, report_ms) {
            (None, _) | (_, None) => Self {
                age_ms: None,
                stale_after_ms,
                fresh: false,
            },
            (Some(now_ms), Some(report_ms)) => {
                let age_ms = now_ms.saturating_sub(report_ms);
                Self {
                    age_ms: Some(age_ms),
                    stale_after_ms,
                    fresh: age_ms >= -Self::FUTURE_TOLERANCE_MS
                        && age_ms <= i64::from(stale_after_ms),
                }
            }
        }
    }
}

/// One report from one sensor.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SensorReading {
    pub descriptor: SensorDescriptor,
    /// The report timestamp as the platform published it, in UTC.
    pub report_timestamp_utc: Option<String>,
    pub freshness: SensorFreshness,
    pub fields: Vec<SensorField>,
}

impl SensorReading {
    /// The ambient-light value, when this sensor publishes one. `None` for every
    /// other sensor: no light value is derived from any other field.
    #[must_use]
    pub fn illuminance_lux(&self) -> Option<f64> {
        self.fields
            .iter()
            .find(|field| field.name == Some(SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX_NAME))
            .and_then(|field| match field.value {
                SensorValue::Real { value } => Some(value),
                SensorValue::Integer { value } => Some(value as f64),
                SensorValue::Unsigned { value } => Some(value as f64),
                _ => None,
            })
    }
}

/// Result of one enumeration: sensors that produced a report, plus the sensors
/// that were enumerated but produced none. The second set is reported instead of
/// being dropped, and is never given a value.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SensorInventory {
    pub sensors: Vec<SensorReading>,
    pub unavailable: Vec<SensorDescriptor>,
}

/// Documented name of one sensor data property key.
pub const SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX_NAME: &str = "SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX";
/// `SENSOR_DATA_TYPE_COMMON_GUID`: report-level fields.
const SENSOR_DATA_TYPE_COMMON_GUID: u128 = 0xdb5e_0cf2_cf1f_4c18_b46c_d86011d62150;
/// `SENSOR_DATA_TYPE_LIGHT_GUID`.
const SENSOR_DATA_TYPE_LIGHT_GUID: u128 = 0xe4c7_7ce2_dcb7_46e9_8439_4fec548833a6;
/// `SENSOR_DATA_TYPE_CUSTOM_GUID`: vendor-defined fields.
const SENSOR_DATA_TYPE_CUSTOM_GUID: u128 = 0xb14c_764f_07cf_41e8_9d82_ebe3d0776a6f;

/// Documented name of a `SENSOR_DATA_TYPE_*` key, or `None` when the key is not
/// one this build knows. Unknown keys are printed as `{FMTID},pid` by callers and
/// are never guessed.
#[must_use]
pub fn data_field_name(fmtid: SensorGuid, pid: u32) -> Option<&'static str> {
    let fmtid = u128::from_be_bytes(fmtid.bytes());
    match (fmtid, pid) {
        (SENSOR_DATA_TYPE_COMMON_GUID, 2) => Some("SENSOR_DATA_TYPE_TIMESTAMP"),
        (SENSOR_DATA_TYPE_LIGHT_GUID, 2) => Some(SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX_NAME),
        (SENSOR_DATA_TYPE_LIGHT_GUID, 3) => Some("SENSOR_DATA_TYPE_LIGHT_TEMPERATURE_KELVIN"),
        (SENSOR_DATA_TYPE_LIGHT_GUID, 4) => Some("SENSOR_DATA_TYPE_LIGHT_CHROMACITY"),
        (SENSOR_DATA_TYPE_CUSTOM_GUID, 5) => Some("SENSOR_DATA_TYPE_CUSTOM_USAGE"),
        (SENSOR_DATA_TYPE_CUSTOM_GUID, 6) => Some("SENSOR_DATA_TYPE_CUSTOM_BOOLEAN_ARRAY"),
        (SENSOR_DATA_TYPE_CUSTOM_GUID, 7..=31) => Some(CUSTOM_VALUE_NAMES[(pid - 7) as usize]),
        _ => None,
    }
}

/// `SENSOR_DATA_TYPE_CUSTOM_VALUE1` … `SENSOR_DATA_TYPE_CUSTOM_VALUE25` (pid 7…31).
const CUSTOM_VALUE_NAMES: [&str; 25] = [
    "SENSOR_DATA_TYPE_CUSTOM_VALUE1",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE2",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE3",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE4",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE5",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE6",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE7",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE8",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE9",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE10",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE11",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE12",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE13",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE14",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE15",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE16",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE17",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE18",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE19",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE20",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE21",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE22",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE23",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE24",
    "SENSOR_DATA_TYPE_CUSTOM_VALUE25",
];

/// `SENSOR_CATEGORY_*` names, so a category is never reported as a bare GUID
/// unless it is genuinely outside the documented set.
#[must_use]
pub fn category_name(category: SensorGuid) -> Option<&'static str> {
    let category = u128::from_be_bytes(category.bytes());
    const NAMES: [(u128, &str); 12] = [
        (0xc317c286_c468_4288_9975_d4c4587c442c, "SENSOR_CATEGORY_ALL"),
        (0xca19690f_a2c7_477d_a99e_99ec6e2b5648, "SENSOR_CATEGORY_BIOMETRIC"),
        (0xfb73fcd8_fc4a_483c_ac58_27b691c6beff, "SENSOR_CATEGORY_ELECTRICAL"),
        (0x323439aa_7f66_492b_ba0c_73e9aa0a65d5, "SENSOR_CATEGORY_ENVIRONMENTAL"),
        (0x17a665c0_9063_4216_b202_5c7a255e18ce, "SENSOR_CATEGORY_LIGHT"),
        (0xbfa794e4_f964_4fdb_90f6_51056bfe4b44, "SENSOR_CATEGORY_LOCATION"),
        (0x8d131d68_8ef7_4656_80b5_cccbd93791c5, "SENSOR_CATEGORY_MECHANICAL"),
        (0xcd09daf1_3b2e_4c3d_b598_b5e5ff93fd46, "SENSOR_CATEGORY_MOTION"),
        (0x9e6c04b6_96fe_4954_b726_68682a473f69, "SENSOR_CATEGORY_ORIENTATION"),
        (0x2c90e7a9_f4c9_4fa2_af37_56d471fe5a3d, "SENSOR_CATEGORY_OTHER"),
        (0xb000e77e_f5b5_420f_815d_0270a726f270, "SENSOR_CATEGORY_SCANNER"),
        (0x2beae7fa_19b0_48c5_a1f6_b5480dc206b0, "SENSOR_CATEGORY_UNSUPPORTED"),
    ];
    NAMES
        .iter()
        .find_map(|(id, name)| (*id == category).then_some(*name))
}

/// `SENSOR_TYPE_*` names for the sensor types a personal computer can expose.
#[must_use]
pub fn sensor_type_name(sensor_type: SensorGuid) -> Option<&'static str> {
    let sensor_type = u128::from_be_bytes(sensor_type.bytes());
    const NAMES: [(u128, &str); 55] = [
        (0xc04d2387_7340_4cc2_991e_3b18cb8ef2f4, "SENSOR_TYPE_ACCELEROMETER_1D"),
        (0xb2c517a8_f6b5_4ba6_a423_5df560b4cc07, "SENSOR_TYPE_ACCELEROMETER_2D"),
        (0xc2fb0f5f_e2d2_4c78_bcd0_352a9582819d, "SENSOR_TYPE_ACCELEROMETER_3D"),
        (0xcdb5d8f7_3cfd_41c8_8542_cce622cf5d6e, "SENSOR_TYPE_AGGREGATED_DEVICE_ORIENTATION"),
        (0x9f81f1af_c4ab_4307_9904_c828bfb90829, "SENSOR_TYPE_AGGREGATED_QUADRANT_ORIENTATION"),
        (
            0x86a19291_0482_402c_bf4c_addac52b1c39,
            "SENSOR_TYPE_AGGREGATED_SIMPLE_DEVICE_ORIENTATION",
        ),
        (0x97f115c8_599a_4153_8894_d2d12899918a, "SENSOR_TYPE_AMBIENT_LIGHT"),
        (0x990b3d8f_85bb_45ff_914d_998c04f372df, "SENSOR_TYPE_BARCODE_SCANNER"),
        (0x9c7e371f_1041_460b_8d5c_71e4752e350c, "SENSOR_TYPE_BOOLEAN_SWITCH"),
        (0x545c8ba5_b143_4545_868f_ca7fd986b4f6, "SENSOR_TYPE_BOOLEAN_SWITCH_ARRAY"),
        (0xca2ffb1c_2317_49c0_a0b4_b63ce63461a0, "SENSOR_TYPE_CAPACITANCE"),
        (0xa415f6c5_cb50_49d0_8e62_a8270bd7a26c, "SENSOR_TYPE_COMPASS_1D"),
        (0x15655cc0_997a_4d30_84db_57caba3648bb, "SENSOR_TYPE_COMPASS_2D"),
        (0x76b5ce0d_17dd_414d_93a1_e127f40bdf6e, "SENSOR_TYPE_COMPASS_3D"),
        (0x5adc9fce_15a0_4bbe_a1ad_2d38a9ae831c, "SENSOR_TYPE_CURRENT"),
        (0xe83af229_8640_4d18_a213_e22675ebb2c3, "SENSOR_TYPE_CUSTOM"),
        (0x5f14ab2f_1407_4306_a93f_b1dbabe4f9c0, "SENSOR_TYPE_DISTANCE_1D"),
        (0x5cf9a46c_a9a2_4e55_b6a1_a04aafa95a92, "SENSOR_TYPE_DISTANCE_2D"),
        (0xa20cae31_0e25_4772_9fe5_96608a1354b2, "SENSOR_TYPE_DISTANCE_3D"),
        (0x212f10f5_14ab_4376_9a43_a7794098c2fe, "SENSOR_TYPE_ELECTRICAL_POWER"),
        (
            0x0e903829_ff8a_4a93_97df_3dcbde402288,
            "SENSOR_TYPE_ENVIRONMENTAL_ATMOSPHERIC_PRESSURE",
        ),
        (0x5c72bf67_bd7e_4257_990b_98a3ba3b400a, "SENSOR_TYPE_ENVIRONMENTAL_HUMIDITY"),
        (0x04fd0ec4_d5da_45fa_95a9_5db38ee19306, "SENSOR_TYPE_ENVIRONMENTAL_TEMPERATURE"),
        (
            0x9ef57a35_9306_434d_af09_37fa5a9c00bd,
            "SENSOR_TYPE_ENVIRONMENTAL_WIND_DIRECTION",
        ),
        (0xdd50607b_a45f_42cd_8efd_ec61761c4226, "SENSOR_TYPE_ENVIRONMENTAL_WIND_SPEED"),
        (0xc2ab2b02_1a1c_4778_a81b_954a1788cc75, "SENSOR_TYPE_FORCE"),
        (0x8cd2cbb6_73e6_4640_a709_72ae8fb60d7f, "SENSOR_TYPE_FREQUENCY"),
        (0xfa088734_f552_4584_8324_edfaf649652c, "SENSOR_TYPE_GYROMETER_1D"),
        (0x31ef4f83_919b_48bf_8de0_5d7a9d240556, "SENSOR_TYPE_GYROMETER_2D"),
        (0x09485f5a_759e_42c2_bd4b_a349b75c8643, "SENSOR_TYPE_GYROMETER_3D"),
        (0xc138c12b_ad52_451c_9375_87f518ff10c6, "SENSOR_TYPE_HUMAN_PRESENCE"),
        (0x5220dae9_3179_4430_9f90_06266d2a34de, "SENSOR_TYPE_HUMAN_PROXIMITY"),
        (0xb96f98c5_7a75_4ba7_94e9_ac868c966dd8, "SENSOR_TYPE_INCLINOMETER_1D"),
        (0xab140f6d_83eb_4264_b70b_b16a5b256a01, "SENSOR_TYPE_INCLINOMETER_2D"),
        (0xb84919fb_ea85_4976_8444_6f6f5c6d31db, "SENSOR_TYPE_INCLINOMETER_3D"),
        (0xdc1d933f_c435_4c7d_a2fe_607192a524d3, "SENSOR_TYPE_INDUCTANCE"),
        (0xd26988cf_5162_4039_bb17_4c58b698e44a, "SENSOR_TYPE_LOCATION_BROADCAST"),
        (0x1a37d538_f28b_42da_9fce_a9d0a2a6d829, "SENSOR_TYPE_LOCATION_DEAD_RECKONING"),
        (0xed4ca589_327a_4ff9_a560_91da4b48275e, "SENSOR_TYPE_LOCATION_GPS"),
        (0x3b2eae4a_72ce_436d_96d2_3c5b8570e987, "SENSOR_TYPE_LOCATION_LOOKUP"),
        (0x9b2d0566_0368_4f71_b88d_533f132031de, "SENSOR_TYPE_LOCATION_OTHER"),
        (0x095f8184_0fa9_4445_8e6e_b70f320b6b4c, "SENSOR_TYPE_LOCATION_STATIC"),
        (0x691c341a_5406_4fe1_942f_2246cbeb39e0, "SENSOR_TYPE_LOCATION_TRIANGULATION"),
        (0x5c7c1a12_30a5_43b9_a4b2_cf09ec5b7be8, "SENSOR_TYPE_MOTION_DETECTOR"),
        (0xb3ee4d76_37a4_4402_b25e_99c60a775fa1, "SENSOR_TYPE_MULTIVALUE_SWITCH"),
        (0x2b3681a9_cadc_45aa_a6ff_54957c8bb440, "SENSOR_TYPE_POTENTIOMETER"),
        (0x26d31f34_6352_41cf_b793_ea0713d53d77, "SENSOR_TYPE_PRESSURE"),
        (0x9993d2c8_c157_4a52_a7b5_195c76037231, "SENSOR_TYPE_RESISTANCE"),
        (0x44328ef5_02dd_4e8d_ad5d_9249832b2eca, "SENSOR_TYPE_RFID_SCANNER"),
        (0xc06dd92c_7feb_438e_9bf6_82207fff5bb8, "SENSOR_TYPE_SCALE"),
        (0x6bd73c1f_0bb4_4310_81b2_dfc18a52bf94, "SENSOR_TYPE_SPEEDOMETER"),
        (0xc6d1ec0e_6803_4361_ad3d_85bcc58c6d29, "SENSOR_TYPE_STRAIN"),
        (0x17db3018_06c4_4f7d_81af_9274b7599c27, "SENSOR_TYPE_TOUCH"),
        (0x10ba83e3_ef4f_41ed_9885_a87d6435a8e1, "SENSOR_TYPE_UNKNOWN"),
        (0xc5484637_4fb7_4953_98b8_a56d8aa1fb1e, "SENSOR_TYPE_VOLTAGE"),
    ];
    NAMES
        .iter()
        .find_map(|(id, name)| (*id == sensor_type).then_some(*name))
}

/// Resolve a `sensor read` target against the enumerated sensors, returning its
/// position in `descriptors`.
///
/// Accepted, in order: the list index, the exact device-interface path, the exact
/// friendly name (both case-insensitive), then a unique case-insensitive
/// substring of a friendly name. An ambiguous substring is rejected with the
/// candidates listed; no match is reported as absent rather than as an empty
/// reading.
pub fn match_sensor(descriptors: &[SensorDescriptor], target: &str) -> Result<usize> {
    let target = target.trim();
    if target.is_empty() {
        return Err(LctrlError::InvalidArgument {
            detail: "sensor target must be a list index, device path, or sensor name".into(),
        });
    }
    if target.bytes().all(|byte| byte.is_ascii_digit()) {
        let index: usize = target.parse().map_err(|_| LctrlError::InvalidArgument {
            detail: format!("sensor index '{target}' does not fit a list index"),
        })?;
        return descriptors
            .iter()
            .position(|descriptor| descriptor.index == index)
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: match descriptors.iter().map(|descriptor| descriptor.index).max() {
                    Some(max) => format!("sensor index {index} is out of range 0..={max}"),
                    None => format!("sensor index {index} is out of range: no sensors enumerated"),
                },
            });
    }
    let lowered = target.to_lowercase();
    if let Some(found) = descriptors.iter().position(|descriptor| {
        descriptor
            .device_path
            .as_deref()
            .is_some_and(|path| path.eq_ignore_ascii_case(target))
    }) {
        return Ok(found);
    }
    if let Some(found) = descriptors.iter().position(|descriptor| {
        descriptor
            .friendly_name
            .as_deref()
            .is_some_and(|name| name.to_lowercase() == lowered)
    }) {
        return Ok(found);
    }
    let matches: Vec<usize> = descriptors
        .iter()
        .enumerate()
        .filter(|descriptor| {
            descriptor
                .1
                .friendly_name
                .as_deref()
                .is_some_and(|name| name.to_lowercase().contains(&lowered))
        })
        .map(|(position, _)| position)
        .collect();
    match matches.as_slice() {
        [single] => Ok(*single),
        [] => Err(LctrlError::Unsupported {
            feature: format!(
                "sensor '{target}' is not present on this platform (enumerated: {})",
                descriptors
                    .iter()
                    .map(SensorDescriptor::label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
        many => Err(LctrlError::InvalidArgument {
            detail: format!(
                "sensor '{target}' matches {} sensors ({}); use the list index or the full name",
                many.len(),
                many.iter()
                    .map(|position| descriptors[*position].label())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

/// Milliseconds since the Unix epoch for a UTC civil timestamp, or `None` when a
/// field is outside its range (a device clock that reports month 0, for example).
#[must_use]
pub fn unix_millis_from_utc(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    millisecond: i64,
) -> Option<i64> {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
        || !(0..=999).contains(&millisecond)
        // A day the month does not have (`April 31`, `February 30`) is not a
        // date: rolling it into the next month would print a timestamp the
        // device never reported.
        || day > days_in_month(year, month)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000 + millisecond)
}

/// `VT_FILETIME` (100 ns ticks since 1601-01-01 UTC) as Unix milliseconds.
#[must_use]
pub fn unix_millis_from_filetime(filetime: u64) -> Option<i64> {
    /// 1970-01-01 minus 1601-01-01 in 100 ns ticks.
    const EPOCH_DIFFERENCE_TICKS: u64 = 116_444_736_000_000_000;
    let ticks = i128::from(filetime) - i128::from(EPOCH_DIFFERENCE_TICKS);
    let millis = ticks / 10_000;
    i64::try_from(millis).ok()
}

/// Length of a proleptic Gregorian month, so a reported day can be rejected
/// instead of being rolled into the next month.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap { 29 } else { 28 }
        }
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`), which avoids pulling a calendar crate in for timestamps.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`days_from_civil`]: `(year, month, day)` for a Unix day number.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, the shape the rest of this CLI prints.
#[must_use]
pub fn format_utc_millis(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let within_day = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = within_day / 3_600_000;
    let minute = (within_day % 3_600_000) / 60_000;
    let second = (within_day % 60_000) / 1_000;
    let millisecond = within_day % 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z")
}

