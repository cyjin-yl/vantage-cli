//! Read-only storage identity and health facts.
//!
//! Every field carries the exact request that produced it, and a value the
//! device refused stays absent instead of becoming a zero. Nothing in this
//! module writes to a drive: there is no self-test trigger, no sanitize and no
//! firmware-slot operation, because the recovered channel
//! (`IOCTL_STORAGE_QUERY_PROPERTY` on a handle opened with no requested access)
//! only answers queries (docs/evidence/re-storage-20261105.md).

use serde::Serialize;

/// Name of the request that produced a value.
///
/// A label is present even when the value is absent: it names the request that
/// was issued and refused, so a report never shows a bare number.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct StorageFieldProvenance {
    /// NVMe Identify Controller, model number (MN) field.
    pub model: &'static str,
    /// NVMe Identify Controller, serial number (SN) field.
    pub serial_number: &'static str,
    /// `STORAGE_DEVICE_DESCRIPTOR`, serial-number offset.
    pub device_serial: &'static str,
    /// NVMe Identify Controller, firmware revision (FR) field.
    pub firmware_version: &'static str,
    /// `STORAGE_DEVICE_DESCRIPTOR`, bus-type byte.
    pub bus_type: &'static str,
    /// Bus type plus the seek-penalty descriptor for non-NVMe devices.
    pub media_type: &'static str,
    /// `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX`, `DiskSize`.
    pub size_bytes: &'static str,
    /// `STORAGE_DEVICE_MANAGEMENT_STATUS`, health field.
    pub health: &'static str,
    /// `StorageDeviceTemperatureProperty` descriptor, composite entry.
    pub temperature: &'static str,
    /// NVMe SMART/Health log page (0x02), percentage-used byte.
    pub percentage_used: &'static str,
    /// NVMe SMART/Health log page (0x02), power-on-hours counter.
    pub power_on_hours: &'static str,
}

/// `STORAGE_BUS_TYPE` as reported by the device descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBusType {
    Unknown,
    Scsi,
    Atapi,
    Ata,
    Ieee1394,
    Ssa,
    Usb,
    Fibre,
    Raid,
    Iscsi,
    Sas,
    Sata,
    Sd,
    Mmc,
    Virtual,
    FileBackedVirtual,
    Spaces,
    Nvme,
    Scm,
    Ufs,
    /// A value newer than this table; the raw number is kept beside it.
    Other,
}

impl StorageBusType {
    /// Map the raw `STORAGE_BUS_TYPE` number; unknown values stay `Other`
    /// instead of being folded into a neighbour.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Unknown,
            1 => Self::Scsi,
            2 => Self::Atapi,
            3 => Self::Ata,
            4 => Self::Ieee1394,
            5 => Self::Ssa,
            6 => Self::Fibre,
            7 => Self::Usb,
            8 => Self::Raid,
            9 => Self::Iscsi,
            10 => Self::Sas,
            11 => Self::Sata,
            12 => Self::Sd,
            13 => Self::Mmc,
            14 => Self::Virtual,
            15 => Self::FileBackedVirtual,
            16 => Self::Spaces,
            17 => Self::Nvme,
            18 => Self::Scm,
            19 => Self::Ufs,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Scsi => "scsi",
            Self::Atapi => "atapi",
            Self::Ata => "ata",
            Self::Ieee1394 => "ieee1394",
            Self::Ssa => "ssa",
            Self::Usb => "usb",
            Self::Fibre => "fibre",
            Self::Raid => "raid",
            Self::Iscsi => "iscsi",
            Self::Sas => "sas",
            Self::Sata => "sata",
            Self::Sd => "sd",
            Self::Mmc => "mmc",
            Self::Virtual => "virtual",
            Self::FileBackedVirtual => "file_backed_virtual",
            Self::Spaces => "spaces",
            Self::Nvme => "nvme",
            Self::Scm => "scm",
            Self::Ufs => "ufs",
            Self::Other => "other",
        }
    }
}

/// Media type, only when a channel states it.
///
/// NVMe is flash by protocol. Other buses are classified by the seek-penalty
/// descriptor; when that descriptor is unavailable the type stays absent
/// rather than being guessed from the model string.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMediaType {
    SolidState,
    Rotational,
}

impl StorageMediaType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SolidState => "solid_state",
            Self::Rotational => "rotational",
        }
    }
}

/// `STORAGE_DISK_HEALTH_STATUS` as reported by the device.
///
/// `Unknown` is a distinct answer: an unknown health code is never promoted to
/// `Healthy`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageHealth {
    Healthy,
    Warning,
    Unhealthy,
    Unknown,
}

impl StorageHealth {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Unhealthy,
            2 => Self::Warning,
            3 => Self::Healthy,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Warning => "warning",
            Self::Unhealthy => "unhealthy",
            Self::Unknown => "unknown",
        }
    }
}

/// NVMe SMART/Health log page critical-warning word, decoded bit by bit.
///
/// Reserved bits are carried in `raw` but never interpreted.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct StorageCriticalWarning {
    pub raw: u8,
    pub available_spare_below_threshold: bool,
    pub temperature_above_threshold: bool,
    pub reliability_degraded: bool,
    pub media_read_only: bool,
    pub volatile_memory_backup_failed: bool,
    pub persistent_memory_region_read_only: bool,
}

impl StorageCriticalWarning {
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self {
            raw,
            available_spare_below_threshold: raw & 0x01 != 0,
            temperature_above_threshold: raw & 0x02 != 0,
            reliability_degraded: raw & 0x04 != 0,
            media_read_only: raw & 0x08 != 0,
            volatile_memory_backup_failed: raw & 0x10 != 0,
            persistent_memory_region_read_only: raw & 0x20 != 0,
        }
    }

    /// Any asserted warning bit. Bits 6–7 are reserved and do not count.
    #[must_use]
    pub const fn any_asserted(self) -> bool {
        self.raw & 0x3f != 0
    }
}

/// One entry of the `StorageDeviceTemperatureProperty` descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct StorageTemperatureSensor {
    pub index: u16,
    pub temperature_celsius: i16,
    /// Upper threshold, omitted when the device encodes "no threshold"
    /// (`0xFEEE`, i.e. −274 °C).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub over_threshold_celsius: Option<i16>,
}

/// Why a field is absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StorageSourceIssue {
    pub source: &'static str,
    pub detail: String,
}

/// One physical drive as observed through read-only queries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StorageDevice {
    /// Device path the queries were issued against, e.g. `\\.\PHYSICALDRIVE0`.
    pub device_path: String,
    /// PnP device interface path from `GUID_DEVINTERFACE_DISK`, when enumerated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_path: Option<String>,
    /// Disk ordinal from `IOCTL_STORAGE_GET_DEVICE_NUMBER`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// NVMe Identify Controller serial number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    /// `STORAGE_DEVICE_DESCRIPTOR` serial number; a different identifier than
    /// `serial_number` on this platform, so both are reported separately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus_type: Option<StorageBusType>,
    /// Raw `STORAGE_BUS_TYPE` number, kept when the bus is not in the table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus_type_raw: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<StorageMediaType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_sector_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<StorageHealth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_warning: Option<StorageCriticalWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_celsius: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_warning_celsius: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_critical_celsius: Option<i16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub temperature_sensors: Vec<StorageTemperatureSensor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage_used: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_spare_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_spare_threshold_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_on_hours: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_cycles: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsafe_shutdowns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_errors: Option<u64>,
    pub fields: StorageFieldProvenance,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<StorageSourceIssue>,
}

/// Every physical drive the storage stack exposed in one read-only pass.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StorageStatus {
    pub devices: Vec<StorageDevice>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<StorageSourceIssue>,
}
