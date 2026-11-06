//! Bluetooth adapter and device-list observation.
//!
//! The contract is enumeration-only. It reports the local radios and the
//! devices the operating system already knows about; it never starts a
//! discovery/inquiry (a radio-level scan), never pairs, connects or removes a
//! device, and never reads pairing material.
//!
//! See `docs/evidence/re-network-20261105.md` for the recovered owner
//! (`bluetoothapis` radio/device enumerators), the live receipts, the PnP and
//! WinRT cross-checks, and the read-back candidates that were tested and
//! rejected.

use lctrl_core::Result;
use serde::Serialize;

/// The Bluetooth assigned-number major device class, the `(class_of_device >>
/// 8) & 0x1f` field of the Class of Device value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothMajorClass {
    Miscellaneous,
    Computer,
    Phone,
    LanNetworkAccessPoint,
    AudioVideo,
    Peripheral,
    Imaging,
    Wearable,
    Toy,
    Health,
    Uncategorized,
    /// A major class value the assigned numbers do not define.
    Unknown,
}

impl BluetoothMajorClass {
    /// Decodes the major class out of a full Class of Device value.
    ///
    /// Only the major class is decoded: minor classes are manufacturer- and
    /// profile-specific in practice, so they stay in the raw value.
    #[must_use]
    pub const fn from_class_of_device(class_of_device: u32) -> Self {
        match (class_of_device >> 8) & 0x1f {
            0 => Self::Miscellaneous,
            1 => Self::Computer,
            2 => Self::Phone,
            3 => Self::LanNetworkAccessPoint,
            4 => Self::AudioVideo,
            5 => Self::Peripheral,
            6 => Self::Imaging,
            7 => Self::Wearable,
            8 => Self::Toy,
            9 => Self::Health,
            31 => Self::Uncategorized,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Miscellaneous => "miscellaneous",
            Self::Computer => "computer",
            Self::Phone => "phone",
            Self::LanNetworkAccessPoint => "lan_network_access_point",
            Self::AudioVideo => "audio_video",
            Self::Peripheral => "peripheral",
            Self::Imaging => "imaging",
            Self::Wearable => "wearable",
            Self::Toy => "toy",
            Self::Health => "health",
            Self::Uncategorized => "uncategorized",
            Self::Unknown => "unknown",
        }
    }
}

/// A UTC instant in the calendar fields the Bluetooth API reports.
///
/// The API returns `SYSTEMTIME` in UTC; this contract keeps the fields instead
/// of converting them, so no timezone or calendar arithmetic can shift a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct BluetoothTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl BluetoothTime {
    /// Builds a time only when every field is a plausible calendar value.
    ///
    /// The API answers "never" with an all-zero `SYSTEMTIME`; that becomes
    /// `None` here rather than being reported as an invented instant. `day` is
    /// bounded per month length only by 31 here: this is a report of what the
    /// API said, not a date computation.
    #[must_use]
    pub fn new(
        year: u16,
        month: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
    ) -> Option<Self> {
        let valid = (1970..=2200).contains(&year)
            && (1..=12).contains(&month)
            && (1..=31).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60;
        if !valid {
            return None;
        }
        let to_u8 = |value: u16| u8::try_from(value).ok();
        Some(Self {
            year,
            month: to_u8(month)?,
            day: to_u8(day)?,
            hour: to_u8(hour)?,
            minute: to_u8(minute)?,
            second: to_u8(second)?,
        })
    }
}

/// One local Bluetooth radio.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BluetoothAdapter {
    /// Lowercase colon-separated hex; the identifier this contract uses.
    pub address: String,
    /// The radio's own name (often the computer name). Never used as an
    /// identifier.
    pub name: Option<String>,
    pub class_of_device: Option<u32>,
    pub major_class: Option<BluetoothMajorClass>,
    pub lmp_subversion: Option<u16>,
    /// `BTH_MFG_*` assigned number, reported raw.
    pub manufacturer: Option<u16>,
}

/// One Bluetooth device the operating system knows about.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BluetoothDeviceRecord {
    /// Lowercase colon-separated hex.
    pub address: String,
    pub name: Option<String>,
    pub class_of_device: Option<u32>,
    pub major_class: Option<BluetoothMajorClass>,
    /// "Connected/in use" as the API reports it. A non-zero `BOOL` is `true`.
    pub connected: bool,
    /// "Remembered": the device is on this machine's device list.
    pub remembered: bool,
    /// "Authenticated/paired/bonded".
    pub authenticated: bool,
    /// Last time the device was seen, UTC. `None` when the API reports "never".
    pub last_seen: Option<BluetoothTime>,
    /// Last time the device was used for more than inquiry/SDP, UTC.
    pub last_used: Option<BluetoothTime>,
}

/// The read-only answer of `bluetooth list`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BluetoothInventory {
    pub observed_at_unix_ms: u64,
    /// Empty on a machine with no Bluetooth radio; the caller distinguishes
    /// "no radio" from "radio with no devices" by this list being empty.
    pub adapters: Vec<BluetoothAdapter>,
    /// Devices that are authenticated, remembered or connected. Devices the
    /// platform has merely seen in a past inquiry are not included: they are
    /// neither paired nor in use, and on this target they were passers-by.
    pub devices: Vec<BluetoothDeviceRecord>,
}

/// Public service contract consumed by the CLI on either platform.
///
/// Implementations enumerate the operating system's cached device records. No
/// method starts an inquiry, and nothing here changes radio or pairing state.
pub trait BluetoothControl: Send + Sync {
    fn bluetooth_devices(&self) -> Result<BluetoothInventory>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_class_decodes_the_live_class_of_device_values() {
        // Values and device kinds from docs/evidence/re-network-20261105.md §3.3/§3.4.
        assert_eq!(
            BluetoothMajorClass::from_class_of_device(0x5a_42_0c).as_str(),
            "phone"
        );
        assert_eq!(
            BluetoothMajorClass::from_class_of_device(0x24_04_04).as_str(),
            "audio_video"
        );
        assert_eq!(
            BluetoothMajorClass::from_class_of_device(0x38_01_0c).as_str(),
            "computer"
        );
        assert_eq!(
            BluetoothMajorClass::from_class_of_device(0x00_1f_00).as_str(),
            "uncategorized"
        );
        assert_eq!(
            BluetoothMajorClass::from_class_of_device(0x00_0b_00).as_str(),
            "unknown"
        );
    }

    #[test]
    fn never_seen_times_stay_absent() {
        assert_eq!(BluetoothTime::new(0, 0, 0, 0, 0, 0), None);
        assert_eq!(BluetoothTime::new(2026, 11, 4, 0, 0, 0), Some(BluetoothTime {
            year: 2026,
            month: 11,
            day: 4,
            hour: 0,
            minute: 0,
            second: 0,
        }));
        // Out-of-range components are rejected instead of being normalized.
        assert_eq!(BluetoothTime::new(2026, 13, 4, 0, 0, 0), None);
        assert_eq!(BluetoothTime::new(2026, 11, 4, 24, 0, 0), None);
    }
}
