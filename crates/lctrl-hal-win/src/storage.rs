//! Read-only storage identity and health over documented Win32 storage IOCTLs.
//!
//! The recovered channel on 21VG is `IOCTL_STORAGE_QUERY_PROPERTY` (device
//! descriptor, NVMe Identify Controller, NVMe SMART/Health log page,
//! temperature descriptor, device-management status, seek penalty) plus
//! `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` for length, all on a handle opened with
//! `dwDesiredAccess = 0`. That combination answers every reported field in both
//! an elevated and a restricted token, whereas the `root\Microsoft\Windows\Storage`
//! namespace is denied to a restricted token on this SKU — which is why no WMI
//! provider is consulted here (docs/evidence/re-storage-20261105.md §6.1, §7).
//!
//! Degradation is per request: a request the device refuses with
//! `ERROR_INVALID_FUNCTION`/`ERROR_NOT_SUPPORTED` (for example NVMe-only log
//! pages on a SATA drive) becomes a recorded issue, while a reply that is short
//! or structurally unexpected is a `ChannelUnavailable` error so a drive is
//! never described from a partially decoded structure.

use lctrl_hal::StorageStatusControl;
use lctrl_core::{
    LctrlError, Result, StorageBusType, StorageCriticalWarning, StorageDevice,
    StorageFieldProvenance, StorageHealth, StorageMediaType, StorageSourceIssue, StorageStatus,
    StorageTemperatureSensor,
};

/// One `IOCTL_STORAGE_QUERY_PROPERTY` request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageQuery {
    /// `StorageDeviceProperty` (`PropertyId` 0).
    DeviceDescriptor,
    /// `StorageDeviceSeekPenaltyProperty` (`PropertyId` 7).
    SeekPenalty,
    /// `StorageDeviceManagementStatus` (`PropertyId` 56).
    DeviceManagementStatus,
    /// `StorageDeviceTemperatureProperty` (`PropertyId` 52).
    DeviceTemperature,
    /// Protocol-specific NVMe Identify Controller (`DataType` 1, CNS 1).
    NvmeIdentifyController,
    /// Protocol-specific NVMe SMART/Health log page (`DataType` 2, page 0x02).
    NvmeSmartLog,
}

impl StorageQuery {
    /// `STORAGE_PROPERTY_ID` this request asks for.
    #[must_use]
    pub const fn property_id(self) -> u32 {
        match self {
            Self::DeviceDescriptor => 0,
            Self::SeekPenalty => 7,
            // Adapter-scoped protocol requests are the form that answered on
            // this SKU (`StorageAdapterProtocolSpecificProperty` = 49).
            Self::NvmeIdentifyController | Self::NvmeSmartLog => 49,
            Self::DeviceTemperature => 52,
            Self::DeviceManagementStatus => 56,
        }
    }

    /// NVMe log/identify page number for protocol-specific requests.
    const fn protocol_page(self) -> Option<(u32, u32)> {
        match self {
            Self::NvmeIdentifyController => Some((1, 1)),
            Self::NvmeSmartLog => Some((2, 2)),
            _ => None,
        }
    }

    /// Bytes the provider must place behind the protocol descriptor.
    const fn payload_length(self) -> usize {
        match self {
            Self::NvmeIdentifyController => 4096,
            Self::NvmeSmartLog => 512,
            _ => 0,
        }
    }

    /// Response buffer size that covers the structure this request returns.
    const fn response_length(self) -> usize {
        match self {
            Self::DeviceDescriptor => 1024,
            Self::SeekPenalty => 32,
            Self::DeviceTemperature => 512,
            Self::DeviceManagementStatus => 256,
            Self::NvmeIdentifyController | Self::NvmeSmartLog => 48 + self.payload_length(),
        }
    }
}

/// A physical disk the storage stack enumerated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageDisk {
    /// `GUID_DEVINTERFACE_DISK` interface path, when the enumeration produced one.
    pub interface_path: Option<String>,
    /// Device path opened for the queries, e.g. `\\.\PHYSICALDRIVE0`.
    pub device_path: String,
    /// Disk ordinal from `IOCTL_STORAGE_GET_DEVICE_NUMBER`.
    pub disk_number: Option<u32>,
}

/// Absolute length and logical sector size of a disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskGeometryFacts {
    pub size_bytes: u64,
    pub logical_sector_size: Option<u32>,
}

/// The storage queries this service needs from the platform.
///
/// A fake implementation in tests answers from recorded structure bytes; the
/// native implementation opens the device with no requested access, which is
/// the access level the requests were verified at.
pub trait StorageProbeApi: Send + Sync {
    /// Disks known to the storage stack. An empty list is a valid observation.
    fn disks(&self) -> Result<Vec<StorageDisk>>;
    /// Issue exactly one query against an already enumerated disk.
    fn query(&self, disk: &StorageDisk, query: StorageQuery) -> Result<Vec<u8>>;
    /// `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` length facts.
    fn geometry(&self, disk: &StorageDisk) -> Result<DiskGeometryFacts>;
}

const SRC_IDENTIFY: &str = "NVMe Identify Controller (CNS 0x01)";
const SRC_DESCRIPTOR_SERIAL: &str = "STORAGE_DEVICE_DESCRIPTOR serial number offset";
const SRC_BUS: &str = "STORAGE_DEVICE_DESCRIPTOR bus type";
const SRC_MEDIA_SEEK: &str = "StorageDeviceSeekPenaltyProperty incurs-seek-penalty";
const SRC_MEDIA_NVME: &str = "STORAGE_DEVICE_DESCRIPTOR bus type = nvme";
const SRC_GEOMETRY: &str = "IOCTL_DISK_GET_DRIVE_GEOMETRY_EX DiskSize";
const SRC_HEALTH: &str = "STORAGE_DEVICE_MANAGEMENT_STATUS health";
const SRC_TEMPERATURE_PROPERTY: &str = "StorageDeviceTemperatureProperty composite entry";
const SRC_TEMPERATURE_LOG: &str = "NVMe SMART/Health log page 0x02 composite temperature";
const SRC_WEAR: &str = "NVMe SMART/Health log page 0x02 percentage used";
const SRC_POWER_ON_HOURS: &str = "NVMe SMART/Health log page 0x02 power-on hours";

const NVME_BUS_TYPE: u32 = 17;

/// `STORAGE_DEVICE_DESCRIPTOR` fields this service reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceDescriptorFacts {
    pub bus_type: u32,
    pub removable_media: bool,
    pub product: Option<String>,
    pub revision: Option<String>,
    pub serial: Option<String>,
}

/// NVMe Identify Controller identity fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NvmeIdentity {
    pub model: String,
    pub serial: String,
    pub firmware: String,
}

/// NVMe SMART/Health log page (0x02) counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NvmeSmartLog {
    pub critical_warning: u8,
    pub temperature_kelvin: u16,
    pub available_spare_percent: u8,
    pub available_spare_threshold_percent: u8,
    pub percentage_used: u8,
    pub data_units_read: u64,
    pub data_units_written: u64,
    pub host_read_commands: u64,
    pub host_write_commands: u64,
    pub controller_busy_minutes: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
}

/// Temperature descriptor facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemperatureFacts {
    pub critical_celsius: i16,
    pub warning_celsius: i16,
    pub sensors: Vec<StorageTemperatureSensor>,
}

fn channel(context: &str, detail: String) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("{context}: {detail}"),
    }
}

fn read_u32(bytes: &[u8], offset: usize, context: &str) -> Result<u32> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| {
            channel(
                context,
                format!("reply is shorter than {offset}+4 bytes"),
            )
        })
}

fn read_u64(bytes: &[u8], offset: usize, context: &str) -> Result<u64> {
    bytes
        .get(offset..offset + 8)
        .and_then(|slice| <[u8; 8]>::try_from(slice).ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| {
            channel(
                context,
                format!("reply is shorter than {offset}+8 bytes"),
            )
        })
}

fn read_u16(bytes: &[u8], offset: usize, context: &str) -> Result<u16> {
    bytes
        .get(offset..offset + 2)
        .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| {
            channel(
                context,
                format!("reply is shorter than {offset}+2 bytes"),
            )
        })
}

/// Trim an ASCII field at its first NUL and strip the drive's space padding.
fn ascii_field(bytes: &[u8], offset: usize, length: usize, context: &str) -> Result<String> {
    let slice = bytes.get(offset..offset + length).ok_or_else(|| {
        channel(
            context,
            format!("reply is shorter than {offset}+{length} bytes"),
        )
    })?;
    let text: String = slice
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| char::from(*byte))
        .collect();
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return Err(channel(
            context,
            format!("ASCII field at offset {offset} is empty"),
        ));
    }
    Ok(trimmed.to_string())
}

/// Decode `STORAGE_DEVICE_DESCRIPTOR`.
///
/// `Version` is the structure size the provider used (40 on this build) and
/// `Size` is the whole reply including the string area, so both are checked
/// before any offset is used.
pub fn decode_device_descriptor(bytes: &[u8]) -> Result<DeviceDescriptorFacts> {
    const CONTEXT: &str = "STORAGE_DEVICE_DESCRIPTOR";
    let version = read_u32(bytes, 0, CONTEXT)?;
    let size = read_u32(bytes, 4, CONTEXT)?;
    if version < 32 {
        return Err(channel(
            CONTEXT,
            format!("declared structure version {version} cannot hold a bus type at offset 28"),
        ));
    }
    if usize::try_from(size).map(|size| size > bytes.len()).unwrap_or(true) {
        return Err(channel(
            CONTEXT,
            format!(
                "declared size {size} exceeds the {} bytes returned",
                bytes.len()
            ),
        ));
    }
    let bus_type = read_u32(bytes, 28, CONTEXT)?;
    let removable_media = bytes
        .get(10)
        .map(|value| *value != 0)
        .ok_or_else(|| channel(CONTEXT, "reply has no removable-media byte".into()))?;
    let product_offset = read_u32(bytes, 16, CONTEXT)?;
    let revision_offset = read_u32(bytes, 20, CONTEXT)?;
    let serial_offset = read_u32(bytes, 24, CONTEXT)?;
    // A zero offset is the documented "field not present" encoding; the SCSI
    // string areas are fixed width, and a truncated value is what the provider
    // reports rather than a separate length.
    let product = optional_ascii(bytes, product_offset, 16);
    let revision = optional_ascii(bytes, revision_offset, 4);
    let serial = optional_ascii(bytes, serial_offset, 40);
    Ok(DeviceDescriptorFacts {
        bus_type,
        removable_media,
        product,
        revision,
        serial,
    })
}

fn optional_ascii(bytes: &[u8], offset: u32, length: usize) -> Option<String> {
    let offset = usize::try_from(offset).ok()?;
    if offset == 0 {
        return None;
    }
    let slice = bytes.get(offset..offset + length)?;
    let text: String = slice
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| char::from(*byte))
        .collect();
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Validate the `STORAGE_PROTOCOL_DATA_DESCRIPTOR` wrapper and return its
/// protocol payload.
fn protocol_payload<'a>(
    bytes: &'a [u8],
    expected_data_type: u32,
    expected_length: usize,
    context: &str,
) -> Result<&'a [u8]> {
    const DESCRIPTOR_SIZE: usize = 48;
    const PROTOCOL_TYPE_NVME: u32 = 3;
    if bytes.len() < DESCRIPTOR_SIZE {
        return Err(channel(
            context,
            format!("reply is {} bytes, shorter than the 48-byte descriptor", bytes.len()),
        ));
    }
    let version = read_u32(bytes, 0, context)?;
    let size = read_u32(bytes, 4, context)?;
    if version != DESCRIPTOR_SIZE as u32 || size < DESCRIPTOR_SIZE as u32 {
        return Err(channel(
            context,
            format!("protocol descriptor version {version} / size {size} is not the documented 48/48"),
        ));
    }
    let protocol_type = read_u32(bytes, 8, context)?;
    if protocol_type != PROTOCOL_TYPE_NVME {
        return Err(channel(
            context,
            format!("protocol type {protocol_type} is not NVMe (3)"),
        ));
    }
    let data_type = read_u32(bytes, 12, context)?;
    if data_type != expected_data_type {
        return Err(channel(
            context,
            format!("data type {data_type} is not the requested {expected_data_type}"),
        ));
    }
    let offset = read_u32(bytes, 24, context)?;
    let length = read_u32(bytes, 28, context)?;
    if usize::try_from(length).ok() != Some(expected_length) {
        return Err(channel(
            context,
            format!("protocol payload length {length} is not the requested {expected_length}"),
        ));
    }
    // The offset is measured from the start of `STORAGE_PROTOCOL_SPECIFIC_DATA`,
    // which sits at byte 8 of the descriptor; the live reply echoes 40.
    let start = 8usize
        .checked_add(usize::try_from(offset).map_err(|_| channel(context, format!("protocol offset {offset} does not fit")))?)
        .ok_or_else(|| channel(context, "protocol offset overflows".into()))?;
    let end = start
        .checked_add(expected_length)
        .ok_or_else(|| channel(context, "protocol length overflows".into()))?;
    bytes.get(start..end).ok_or_else(|| {
        channel(
            context,
            format!(
                "protocol payload {start}..{end} lies outside the {} bytes returned",
                bytes.len()
            ),
        )
    })
}

/// Decode NVMe Identify Controller (`DataType` 1, CNS 1).
pub fn decode_nvme_identify_controller(bytes: &[u8]) -> Result<NvmeIdentity> {
    const CONTEXT: &str = "NVMe Identify Controller";
    let payload = protocol_payload(bytes, 1, 4096, CONTEXT)?;
    Ok(NvmeIdentity {
        serial: ascii_field(payload, 4, 20, CONTEXT)?,
        model: ascii_field(payload, 24, 40, CONTEXT)?,
        firmware: ascii_field(payload, 64, 8, CONTEXT)?,
    })
}

/// Decode the NVMe SMART/Health log page (`DataType` 2, page 0x02).
pub fn decode_nvme_smart_log(bytes: &[u8]) -> Result<NvmeSmartLog> {
    const CONTEXT: &str = "NVMe SMART/Health log page";
    let payload = protocol_payload(bytes, 2, 512, CONTEXT)?;
    Ok(NvmeSmartLog {
        critical_warning: *payload
            .first()
            .ok_or_else(|| channel(CONTEXT, "payload has no critical-warning byte".into()))?,
        temperature_kelvin: read_u16(payload, 1, CONTEXT)?,
        available_spare_percent: payload[3],
        available_spare_threshold_percent: payload[4],
        percentage_used: payload[5],
        data_units_read: read_u64(payload, 32, CONTEXT)?,
        data_units_written: read_u64(payload, 48, CONTEXT)?,
        host_read_commands: read_u64(payload, 64, CONTEXT)?,
        host_write_commands: read_u64(payload, 80, CONTEXT)?,
        controller_busy_minutes: read_u64(payload, 96, CONTEXT)?,
        power_cycles: read_u64(payload, 112, CONTEXT)?,
        power_on_hours: read_u64(payload, 128, CONTEXT)?,
        unsafe_shutdowns: read_u64(payload, 144, CONTEXT)?,
        media_errors: read_u64(payload, 160, CONTEXT)?,
        error_log_entries: read_u64(payload, 176, CONTEXT)?,
    })
}

/// Decode `STORAGE_TEMPERATURE_DATA_DESCRIPTOR`.
///
/// Entries start at offset 24 and are 16 bytes wide. `OverThreshold`/
/// `UnderThreshold` of `0xFEEE` (−274 °C) is the device's "no threshold"
/// encoding and becomes `None` instead of a limit.
pub fn decode_temperature_descriptor(bytes: &[u8]) -> Result<TemperatureFacts> {
    const CONTEXT: &str = "STORAGE_TEMPERATURE_DATA_DESCRIPTOR";
    const HEADER: usize = 24;
    const ENTRY: usize = 16;
    let version = read_u32(bytes, 0, CONTEXT)?;
    let size = read_u32(bytes, 4, CONTEXT)?;
    if version < HEADER as u32 {
        return Err(channel(
            CONTEXT,
            format!("declared version {version} cannot hold a {HEADER}-byte header"),
        ));
    }
    if usize::try_from(size).map(|size| size > bytes.len()).unwrap_or(true) {
        return Err(channel(
            CONTEXT,
            format!(
                "declared size {size} exceeds the {} bytes returned",
                bytes.len()
            ),
        ));
    }
    let critical_celsius = read_u16(bytes, 8, CONTEXT)? as i16;
    let warning_celsius = read_u16(bytes, 10, CONTEXT)? as i16;
    let count = read_u16(bytes, 12, CONTEXT)?;
    let count = usize::from(count);
    let required = HEADER + count * ENTRY;
    if required > bytes.len() {
        return Err(channel(
            CONTEXT,
            format!("{count} entries need {required} bytes but {} were returned", bytes.len()),
        ));
    }
    let mut sensors = Vec::with_capacity(count);
    for index in 0..count {
        let base = HEADER + index * ENTRY;
        let temperature_celsius = read_u16(bytes, base + 2, CONTEXT)? as i16;
        let over_threshold = read_u16(bytes, base + 4, CONTEXT)? as i16;
        sensors.push(StorageTemperatureSensor {
            index: read_u16(bytes, base, CONTEXT)?,
            temperature_celsius,
            over_threshold_celsius: (over_threshold != NO_THRESHOLD).then_some(over_threshold),
        });
    }
    Ok(TemperatureFacts {
        critical_celsius,
        warning_celsius,
        sensors,
    })
}

/// The device's "threshold not set" encoding (−274 °C).
const NO_THRESHOLD: i16 = -274;

/// Decode `STORAGE_DEVICE_MANAGEMENT_STATUS`.
pub fn decode_management_status(bytes: &[u8]) -> Result<StorageHealth> {
    const CONTEXT: &str = "STORAGE_DEVICE_MANAGEMENT_STATUS";
    let version = read_u32(bytes, 0, CONTEXT)?;
    let size = read_u32(bytes, 4, CONTEXT)?;
    if version < 16 {
        return Err(channel(
            CONTEXT,
            format!("declared version {version} cannot hold a health field"),
        ));
    }
    if usize::try_from(size).map(|size| size > bytes.len()).unwrap_or(true) {
        return Err(channel(
            CONTEXT,
            format!(
                "declared size {size} exceeds the {} bytes returned",
                bytes.len()
            ),
        ));
    }
    Ok(StorageHealth::from_raw(read_u32(bytes, 8, CONTEXT)?))
}

/// Decode `DEVICE_SEEK_PENALTY_DESCRIPTOR`.
pub fn decode_seek_penalty(bytes: &[u8]) -> Result<bool> {
    const CONTEXT: &str = "DEVICE_SEEK_PENALTY_DESCRIPTOR";
    let size = read_u32(bytes, 4, CONTEXT)?;
    if size < 9 {
        return Err(channel(
            CONTEXT,
            format!("declared size {size} cannot hold the seek-penalty flag"),
        ));
    }
    bytes
        .get(8)
        .map(|value| *value != 0)
        .ok_or_else(|| channel(CONTEXT, "reply has no seek-penalty flag".into()))
}

/// Decode `DISK_GEOMETRY_EX`: `DiskSize` follows the 24-byte `DISK_GEOMETRY`.
pub fn decode_geometry(bytes: &[u8]) -> Result<DiskGeometryFacts> {
    const CONTEXT: &str = "DISK_GEOMETRY_EX";
    let size_bytes = read_u64(bytes, 24, CONTEXT)?;
    if size_bytes == 0 {
        return Err(channel(
            CONTEXT,
            "device reported a zero length, which is not a usable capacity".into(),
        ));
    }
    let bytes_per_sector = read_u32(bytes, 20, CONTEXT)?;
    let logical_sector_size = (bytes_per_sector > 0).then_some(bytes_per_sector);
    Ok(DiskGeometryFacts {
        size_bytes,
        logical_sector_size,
    })
}

/// Read-only storage status composed from one probe.
#[derive(Clone, Copy, Debug)]
pub struct WindowsStorage<P> {
    probe: P,
}

impl<P> WindowsStorage<P> {
    #[must_use]
    pub const fn new(probe: P) -> Self {
        Self { probe }
    }

    #[must_use]
    pub const fn probe(&self) -> &P {
        &self.probe
    }
}

impl<P> WindowsStorage<P>
where
    P: StorageProbeApi,
{
    /// Describe every enumerated disk.
    ///
    /// A device-level refusal is recorded per field; a structurally invalid
    /// reply aborts the pass because a drive must not be described from a
    /// partially decoded structure.
    pub fn describe_all(&self) -> Result<StorageStatus> {
        let disks = self.probe.disks()?;
        let mut devices = Vec::with_capacity(disks.len());
        let mut issues = Vec::new();
        if disks.is_empty() {
            issues.push(StorageSourceIssue {
                source: "GUID_DEVINTERFACE_DISK",
                detail: "the storage stack exposed no physical disk device interface".into(),
            });
        }
        for disk in &disks {
            devices.push(self.describe(disk)?);
        }
        Ok(StorageStatus { devices, issues })
    }

    fn describe(&self, disk: &StorageDisk) -> Result<StorageDevice> {
        let geometry = self.probe.geometry(disk)?;
        let descriptor = decode_device_descriptor(
            &self.probe.query(disk, StorageQuery::DeviceDescriptor)?,
        )?;
        let bus_type = StorageBusType::from_raw(descriptor.bus_type);
        let mut issues = Vec::new();
        let mut device = StorageDevice {
            device_path: disk.device_path.clone(),
            interface_path: disk.interface_path.clone(),
            disk_number: disk.disk_number,
            model: None,
            serial_number: None,
            device_serial: descriptor.serial.clone(),
            firmware_version: None,
            bus_type: Some(bus_type),
            bus_type_raw: matches!(bus_type, StorageBusType::Other).then_some(descriptor.bus_type),
            media_type: None,
            logical_sector_size: geometry.logical_sector_size,
            size_bytes: Some(geometry.size_bytes),
            health: None,
            critical_warning: None,
            temperature_celsius: None,
            temperature_warning_celsius: None,
            temperature_critical_celsius: None,
            temperature_sensors: Vec::new(),
            percentage_used: None,
            available_spare_percent: None,
            available_spare_threshold_percent: None,
            power_on_hours: None,
            power_cycles: None,
            unsafe_shutdowns: None,
            media_errors: None,
            fields: StorageFieldProvenance {
                model: SRC_IDENTIFY,
                serial_number: SRC_IDENTIFY,
                device_serial: SRC_DESCRIPTOR_SERIAL,
                firmware_version: SRC_IDENTIFY,
                bus_type: SRC_BUS,
                media_type: SRC_MEDIA_NVME,
                size_bytes: SRC_GEOMETRY,
                health: SRC_HEALTH,
                temperature: SRC_TEMPERATURE_PROPERTY,
                percentage_used: SRC_WEAR,
                power_on_hours: SRC_POWER_ON_HOURS,
            },
            issues: Vec::new(),
        };

        // Media type: NVMe is flash by protocol; anything else needs the
        // seek-penalty descriptor.
        if bus_type == StorageBusType::Nvme {
            device.media_type = Some(StorageMediaType::SolidState);
        } else {
            match self.probe.query(disk, StorageQuery::SeekPenalty) {
                Ok(bytes) => match decode_seek_penalty(&bytes) {
                    Ok(true) => device.media_type = Some(StorageMediaType::Rotational),
                    Ok(false) => device.media_type = Some(StorageMediaType::SolidState),
                    Err(error) => return Err(error),
                },
                Err(LctrlError::Unsupported { .. }) => issues.push(StorageSourceIssue {
                    source: SRC_MEDIA_SEEK,
                    detail: "device refused the seek-penalty descriptor; media type is not asserted"
                        .into(),
                }),
                Err(error) => return Err(error),
            }
            device.fields.media_type = SRC_MEDIA_SEEK;
        }

        // The SMART log page temperature is a labelled fallback for the case
        // where the device refuses the temperature descriptor below.
        let mut log_page_temperature = None;

        if bus_type == StorageBusType::Nvme {
            match self.probe.query(disk, StorageQuery::NvmeIdentifyController) {
                Ok(bytes) => {
                    let identity = decode_nvme_identify_controller(&bytes)?;
                    device.model = Some(identity.model);
                    device.serial_number = Some(identity.serial);
                    device.firmware_version = Some(identity.firmware);
                }
                Err(LctrlError::Unsupported { .. }) => issues.push(StorageSourceIssue {
                    source: SRC_IDENTIFY,
                    detail: "device refused the Identify Controller request".into(),
                }),
                Err(error) => return Err(error),
            }
            match self.probe.query(disk, StorageQuery::NvmeSmartLog) {
                Ok(bytes) => {
                    let log = decode_nvme_smart_log(&bytes)?;
                    device.critical_warning =
                        Some(StorageCriticalWarning::from_raw(log.critical_warning));
                    device.percentage_used = Some(log.percentage_used);
                    device.available_spare_percent = Some(log.available_spare_percent);
                    device.available_spare_threshold_percent =
                        Some(log.available_spare_threshold_percent);
                    device.power_on_hours = Some(log.power_on_hours);
                    device.power_cycles = Some(log.power_cycles);
                    device.unsafe_shutdowns = Some(log.unsafe_shutdowns);
                    device.media_errors = Some(log.media_errors);
                    log_page_temperature = kelvin_to_celsius(log.temperature_kelvin);
                }
                Err(LctrlError::Unsupported { .. }) => issues.push(StorageSourceIssue {
                    source: SRC_WEAR,
                    detail: "device refused the SMART/Health log page; wear and power-on hours are not reported".into(),
                }),
                Err(error) => return Err(error),
            }
        }

        match self.probe.query(disk, StorageQuery::DeviceManagementStatus) {
            Ok(bytes) => device.health = Some(decode_management_status(&bytes)?),
            Err(LctrlError::Unsupported { .. }) => issues.push(StorageSourceIssue {
                source: SRC_HEALTH,
                detail: "device refused the management-status request; health is not reported".into(),
            }),
            Err(error) => return Err(error),
        }

        match self.probe.query(disk, StorageQuery::DeviceTemperature) {
            Ok(bytes) => {
                let facts = decode_temperature_descriptor(&bytes)?;
                device.temperature_warning_celsius = Some(facts.warning_celsius);
                device.temperature_critical_celsius = Some(facts.critical_celsius);
                let composite = facts
                    .sensors
                    .iter()
                    .find(|sensor| sensor.index == 0)
                    .or_else(|| facts.sensors.first());
                if let Some(composite) = composite {
                    device.temperature_celsius = Some(composite.temperature_celsius);
                    device.fields.temperature = SRC_TEMPERATURE_PROPERTY;
                }
                device.temperature_sensors = facts.sensors;
            }
            Err(LctrlError::Unsupported { .. }) => {
                issues.push(StorageSourceIssue {
                    source: SRC_TEMPERATURE_PROPERTY,
                    detail: "device refused the temperature descriptor".into(),
                });
                if let Some(celsius) = log_page_temperature {
                    device.temperature_celsius = Some(celsius);
                    device.fields.temperature = SRC_TEMPERATURE_LOG;
                }
            }
            Err(error) => return Err(error),
        }

        device.issues = issues;
        Ok(device)
    }
}

/// Convert an NVMe composite temperature (Kelvin) to whole degrees Celsius.
///
/// A zero reading is the device's "temperature not reported" encoding and
/// stays absent rather than being printed as −273 °C.
fn kelvin_to_celsius(kelvin: u16) -> Option<i16> {
    if kelvin == 0 {
        return None;
    }
    i16::try_from(i32::from(kelvin) - 273).ok()
}

/// Let a borrowed probe be used where an owned one is expected, so capability
/// recording can probe through a shared reference.
impl<T> StorageProbeApi for &T
where
    T: StorageProbeApi + ?Sized,
{
    fn disks(&self) -> Result<Vec<StorageDisk>> {
        (**self).disks()
    }

    fn query(&self, disk: &StorageDisk, query: StorageQuery) -> Result<Vec<u8>> {
        (**self).query(disk, query)
    }

    fn geometry(&self, disk: &StorageDisk) -> Result<DiskGeometryFacts> {
        (**self).geometry(disk)
    }
}

impl<P> StorageStatusControl for WindowsStorage<P>
where
    P: StorageProbeApi,
{
    fn storage_status(&self) -> Result<StorageStatus> {
        self.describe_all()
    }
}

/// Native storage probe over the disk device interface class.
#[cfg(windows)]
pub use native::NativeStorageProbe;

/// Cheap capability probe for `storage.status`.
///
/// Capability recording must not run the full per-drive report, so this
/// enumerates the disk interface class and asks the first disk for its device
/// descriptor — the one request every reported field's prerequisites go
/// through — and returns how many disks the class exposed. Zero is a valid
/// answer: it means the class answered and holds no disk, which `storage
/// status` reports as an empty list plus an issue
/// (docs/evidence/re-storage-20261105.md §8).
#[cfg(windows)]
pub fn probe_storage() -> Result<usize> {
    let probe = native::NativeStorageProbe;
    let disks = probe.disks()?;
    if let Some(first) = disks.first() {
        probe.query(first, StorageQuery::DeviceDescriptor)?;
    }
    Ok(disks.len())
}

#[cfg(windows)]
mod native {
    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::{
        Devices::DeviceAndDriverInstallation::{
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_ListW,
            CM_Get_Device_Interface_List_SizeW,
        },
        Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };

    use lctrl_core::{LctrlError, Result};

    use super::{DiskGeometryFacts, StorageDisk, StorageProbeApi, StorageQuery, decode_geometry};
    use crate::map_win_error;

    /// `CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS)`.
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    /// `CTL_CODE(IOCTL_STORAGE_BASE, 0x0420, METHOD_BUFFERED, FILE_ANY_ACCESS)`.
    const IOCTL_STORAGE_GET_DEVICE_NUMBER: u32 = 0x002D_1080;
    /// `CTL_CODE(IOCTL_DISK_BASE, 0x0028, METHOD_BUFFERED, FILE_ANY_ACCESS)`.
    const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;

    /// Upper bound on one enumerated interface path. The list is sized by the
    /// configuration manager; this cap keeps a corrupt size from allocating.
    const MAX_INTERFACE_LIST_BYTES: usize = 64 * 1024;
    /// `STORAGE_PROPERTY_QUERY` including its 4-byte padding.
    const PROPERTY_QUERY_HEADER: usize = 12;
    /// `STORAGE_PROTOCOL_SPECIFIC_DATA` follows the 8-byte query header.
    const PROTOCOL_DATA_START: usize = 8;
    /// `STORAGE_PROTOCOL_SPECIFIC_DATA` size on x64.
    const PROTOCOL_DATA_SIZE: usize = 40;

    /// `GUID_DEVINTERFACE_DISK`.
    const DISK_INTERFACE_GUID: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(
        0x53f5_6307_b6bf_11d0_94f2_00a0c91efb8b,
    );

    /// Native storage probe over the disk device interface class.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeStorageProbe;

    impl StorageProbeApi for NativeStorageProbe {
        fn disks(&self) -> Result<Vec<StorageDisk>> {
            let paths = disk_interface_paths()?;
            let mut disks = Vec::with_capacity(paths.len());
            for path in paths {
                let handle = DiskHandle::open(&path, "disk device interface")?;
                let number = handle.device_number("disk device interface")?;
                disks.push(StorageDisk {
                    interface_path: Some(path),
                    device_path: format!(r"\\.\PHYSICALDRIVE{number}"),
                    disk_number: Some(number),
                });
            }
            Ok(disks)
        }

        fn query(&self, disk: &StorageDisk, query: StorageQuery) -> Result<Vec<u8>> {
            let handle = DiskHandle::open(&disk.device_path, "physical drive")?;
            let input = query_input(query);
            handle.ioctl(
                IOCTL_STORAGE_QUERY_PROPERTY,
                &input,
                query.response_length(),
                &format!("{} {:?}", disk.device_path, query),
            )
        }

        fn geometry(&self, disk: &StorageDisk) -> Result<DiskGeometryFacts> {
            let handle = DiskHandle::open(&disk.device_path, "physical drive")?;
            let bytes = handle.ioctl(
                IOCTL_DISK_GET_DRIVE_GEOMETRY_EX,
                &[],
                128,
                &format!("{} geometry", disk.device_path),
            )?;
            decode_geometry(&bytes)
        }
    }

    /// Build the `STORAGE_PROPERTY_QUERY` input for one request.
    ///
    /// Standard requests send the 12-byte query structure. Protocol-specific
    /// requests append `STORAGE_PROTOCOL_SPECIFIC_DATA` with
    /// `ProtocolType = 3` (NVMe), the identify/log-page selector as
    /// `ProtocolDataRequestValue`, a 40-byte protocol-data offset and the
    /// requested payload length — the shape that answered on this SKU.
    fn query_input(query: StorageQuery) -> Vec<u8> {
        let property_id = query.property_id();
        if let Some((data_type, page)) = query.protocol_page() {
            let mut input = vec![0u8; PROTOCOL_DATA_START + PROTOCOL_DATA_SIZE];
            input[0..4].copy_from_slice(&property_id.to_le_bytes());
            // QueryType = PropertyStandardQuery (0)
            input[PROTOCOL_DATA_START..PROTOCOL_DATA_START + 4]
                .copy_from_slice(&3u32.to_le_bytes()); // ProtocolTypeNvme
            input[PROTOCOL_DATA_START + 4..PROTOCOL_DATA_START + 8]
                .copy_from_slice(&data_type.to_le_bytes());
            input[PROTOCOL_DATA_START + 8..PROTOCOL_DATA_START + 12]
                .copy_from_slice(&page.to_le_bytes());
            // ProtocolDataOffset is measured from STORAGE_PROTOCOL_SPECIFIC_DATA.
            input[PROTOCOL_DATA_START + 16..PROTOCOL_DATA_START + 20]
                .copy_from_slice(&(PROTOCOL_DATA_SIZE as u32).to_le_bytes());
            input[PROTOCOL_DATA_START + 20..PROTOCOL_DATA_START + 24]
                .copy_from_slice(&(query.payload_length() as u32).to_le_bytes());
            return input;
        }
        let mut input = vec![0u8; PROPERTY_QUERY_HEADER];
        input[0..4].copy_from_slice(&property_id.to_le_bytes());
        input
    }

    /// Enumerate `GUID_DEVINTERFACE_DISK` paths through the configuration manager.
    fn disk_interface_paths() -> Result<Vec<String>> {
        let mut size = 0u32;
        let status = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut size,
                &DISK_INTERFACE_GUID,
                ptr::null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                "CM_Get_Device_Interface_List_SizeW(GUID_DEVINTERFACE_DISK)",
            ));
        }
        if size == 0 {
            return Ok(Vec::new());
        }
        let bytes = usize::try_from(size)
            .map_err(|_| LctrlError::Io(std::io::Error::other("interface list size overflow")))?;
        if bytes > MAX_INTERFACE_LIST_BYTES {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "disk interface list needs {bytes} bytes, above the {MAX_INTERFACE_LIST_BYTES}-byte bound"
                ),
            });
        }
        let mut buffer = vec![0u16; bytes];
        let status = unsafe {
            CM_Get_Device_Interface_ListW(
                &DISK_INTERFACE_GUID,
                ptr::null(),
                buffer.as_mut_ptr(),
                size,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                "CM_Get_Device_Interface_ListW(GUID_DEVINTERFACE_DISK)",
            ));
        }
        Ok(split_multi_string(&buffer))
    }

    fn split_multi_string(buffer: &[u16]) -> Vec<String> {
        let mut paths = Vec::new();
        let mut start = 0usize;
        for (index, unit) in buffer.iter().enumerate() {
            if *unit == 0 {
                if index > start {
                    paths.push(String::from_utf16_lossy(&buffer[start..index]));
                }
                start = index + 1;
            }
        }
        paths
    }

    /// A physical-disk handle.
    ///
    /// The handle requests **no access**: `IOCTL_STORAGE_QUERY_PROPERTY` and
    /// `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` answer on a zero-access handle in a
    /// restricted token, while `GENERIC_READ` is denied there
    /// (docs/evidence/re-storage-20261105.md §7). Nothing here needs write
    /// access, so none is requested.
    struct DiskHandle(HANDLE);

    impl DiskHandle {
        fn open(path: &str, context: &str) -> Result<Self> {
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                let error = unsafe { GetLastError() };
                return Err(map_win_error(error, &format!("{context} open {path}")));
            }
            Ok(Self(handle))
        }

        fn ioctl(
            &self,
            code: u32,
            input: &[u8],
            output_len: usize,
            context: &str,
        ) -> Result<Vec<u8>> {
            let input_len = u32::try_from(input.len()).map_err(|_| {
                LctrlError::InvalidArgument {
                    detail: format!("{context} input exceeds u32 buffer length"),
                }
            })?;
            let requested = u32::try_from(output_len).map_err(|_| {
                LctrlError::InvalidArgument {
                    detail: format!("{context} output exceeds u32 buffer length"),
                }
            })?;
            let input_ptr = if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr().cast::<c_void>()
            };
            let mut output = vec![0u8; output_len];
            let output_ptr = if output.is_empty() {
                ptr::null_mut()
            } else {
                output.as_mut_ptr().cast::<c_void>()
            };
            let mut returned = 0u32;
            let success = unsafe {
                DeviceIoControl(
                    self.0,
                    code,
                    input_ptr,
                    input_len,
                    output_ptr,
                    requested,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if success == 0 {
                let error = unsafe { GetLastError() };
                return Err(map_win_error(error, &format!("{context} IOCTL 0x{code:08x}")));
            }
            let returned = usize::try_from(returned).map_err(|_| {
                LctrlError::Io(std::io::Error::other(format!(
                    "{context} byte count does not fit usize"
                )))
            })?;
            if returned > output.len() {
                return Err(LctrlError::FirmwareRejected {
                    detail: format!(
                        "{context} IOCTL 0x{code:08x} returned {returned} bytes into a {}-byte buffer",
                        output.len()
                    ),
                });
            }
            output.truncate(returned);
            Ok(output)
        }

        fn device_number(&self, context: &str) -> Result<u32> {
            let bytes = self.ioctl(IOCTL_STORAGE_GET_DEVICE_NUMBER, &[], 12, context)?;
            if bytes.len() < 8 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("{context}: STORAGE_DEVICE_NUMBER reply is {} bytes", bytes.len()),
                });
            }
            let mut number = [0u8; 4];
            number.copy_from_slice(&bytes[4..8]);
            Ok(u32::from_le_bytes(number))
        }
    }

    impl Drop for DiskHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use lctrl_core::{LctrlError, StorageHealth, StorageMediaType, StorageStatus};

    use super::*;

    fn descriptor(bus_type: u32, product: &str, revision: &str, serial: &str) -> Vec<u8> {
        let mut bytes = vec![0u8; 396];
        bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&396u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&bus_type.to_le_bytes());
        let product_at = 57u32;
        bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&product_at.to_le_bytes());
        bytes[product_at as usize..product_at as usize + product.len()]
            .copy_from_slice(product.as_bytes());
        let revision_at = 122u32;
        bytes[20..24].copy_from_slice(&revision_at.to_le_bytes());
        bytes[revision_at as usize..revision_at as usize + revision.len()]
            .copy_from_slice(revision.as_bytes());
        let serial_at = 139u32;
        bytes[24..28].copy_from_slice(&serial_at.to_le_bytes());
        bytes[serial_at as usize..serial_at as usize + serial.len()]
            .copy_from_slice(serial.as_bytes());
        bytes
    }

    fn protocol_descriptor(data_type: u32, payload: Vec<u8>) -> Vec<u8> {
        let mut bytes = vec![0u8; 48];
        bytes[0..4].copy_from_slice(&48u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&48u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&data_type.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&40u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn identify_controller(serial: &str, model: &str, firmware: &str) -> Vec<u8> {
        let mut payload = vec![0u8; 4096];
        payload[0..2].copy_from_slice(&0x1cc4u16.to_le_bytes());
        payload[4..4 + serial.len()].copy_from_slice(serial.as_bytes());
        payload[24..24 + model.len()].copy_from_slice(model.as_bytes());
        payload[64..64 + firmware.len()].copy_from_slice(firmware.as_bytes());
        protocol_descriptor(1, payload)
    }

    fn smart_log(temperature_kelvin: u16, percentage_used: u8) -> Vec<u8> {
        let mut payload = vec![0u8; 512];
        payload[1..3].copy_from_slice(&temperature_kelvin.to_le_bytes());
        payload[3] = 100;
        payload[4] = 10;
        payload[5] = percentage_used;
        payload[32..40].copy_from_slice(&110_518_068u64.to_le_bytes());
        payload[48..56].copy_from_slice(&68_841_344u64.to_le_bytes());
        payload[112..120].copy_from_slice(&161u64.to_le_bytes());
        payload[128..136].copy_from_slice(&5_633u64.to_le_bytes());
        payload[144..152].copy_from_slice(&51u64.to_le_bytes());
        protocol_descriptor(2, payload)
    }

    fn temperature_descriptor() -> Vec<u8> {
        let mut bytes = vec![0u8; 24 + 3 * 16];
        let declared_size = bytes.len() as u32;
        bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&declared_size.to_le_bytes());
        bytes[8..10].copy_from_slice(&87i16.to_le_bytes());
        bytes[10..12].copy_from_slice(&84i16.to_le_bytes());
        bytes[12..14].copy_from_slice(&3u16.to_le_bytes());
        for (index, (sensor, celsius, over)) in [
            (0u16, 28i16, 84i16),
            (1, 31, -274),
            (2, 28, -274),
        ]
        .into_iter()
        .enumerate()
        {
            let base = 24 + index * 16;
            bytes[base..base + 2].copy_from_slice(&sensor.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&celsius.to_le_bytes());
            bytes[base + 4..base + 6].copy_from_slice(&over.to_le_bytes());
        }
        bytes
    }

    fn management_status(health: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 84];
        bytes[0..4].copy_from_slice(&100u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&84u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&health.to_le_bytes());
        bytes
    }

    fn geometry() -> Vec<u8> {
        let mut bytes = vec![0u8; 112];
        bytes[0..8].copy_from_slice(&124_519i64.to_le_bytes());
        bytes[8..12].copy_from_slice(&12u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&512u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&1_024_209_543_168u64.to_le_bytes());
        bytes
    }

    #[derive(Default)]
    struct FakeProbe {
        answers: BTreeMap<u8, Result<Vec<u8>>>,
        disks: Vec<StorageDisk>,
    }

    fn query_key(query: StorageQuery) -> u8 {
        match query {
            StorageQuery::DeviceDescriptor => 1,
            StorageQuery::SeekPenalty => 2,
            StorageQuery::DeviceManagementStatus => 3,
            StorageQuery::DeviceTemperature => 4,
            StorageQuery::NvmeIdentifyController => 5,
            StorageQuery::NvmeSmartLog => 6,
        }
    }

    impl FakeProbe {
        fn answer(mut self, query: StorageQuery, bytes: Vec<u8>) -> Self {
            self.answers.insert(query_key(query), Ok(bytes));
            self
        }

        fn refuse(mut self, query: StorageQuery) -> Self {
            self.answers.insert(
                query_key(query),
                Err(LctrlError::Unsupported {
                    feature: format!("{query:?}"),
                }),
            );
            self
        }

        fn with_disk(mut self) -> Self {
            self.disks = vec![StorageDisk {
                interface_path: Some(r"\\?\SCSI#Disk&Ven_NVMe&Prod_UMIS#0".into()),
                device_path: r"\\.\PHYSICALDRIVE0".into(),
                disk_number: Some(0),
            }];
            self
        }
    }

    impl StorageProbeApi for FakeProbe {
        fn disks(&self) -> Result<Vec<StorageDisk>> {
            Ok(self.disks.clone())
        }

        fn query(&self, _disk: &StorageDisk, query: StorageQuery) -> Result<Vec<u8>> {
            self.answers
                .get(&query_key(query))
                .cloned()
                .unwrap_or_else(|| {
                    Err(LctrlError::Unsupported {
                        feature: format!("{query:?}"),
                    })
                })
        }

        fn geometry(&self, _disk: &StorageDisk) -> Result<DiskGeometryFacts> {
            decode_geometry(&geometry())
        }
    }

    fn nvme_probe() -> FakeProbe {
        FakeProbe::default()
            .with_disk()
            .answer(
                StorageQuery::DeviceDescriptor,
                descriptor(17, "UMIS UPJYJ1TBMNV", "1.0L", "5CDF_B806_1090_9BA5."),
            )
            .answer(
                StorageQuery::NvmeIdentifyController,
                identify_controller("FDBS26010EA61912B1", "UMIS UPJYJ1TBMNV1QWY", "1.0L06D1"),
            )
            .answer(StorageQuery::NvmeSmartLog, smart_log(302, 8))
            .answer(StorageQuery::DeviceManagementStatus, management_status(3))
            .answer(StorageQuery::DeviceTemperature, temperature_descriptor())
    }

    fn only_device(status: StorageStatus) -> StorageDevice {
        assert_eq!(status.devices.len(), 1, "expected one device");
        status.devices.into_iter().next().unwrap()
    }

    #[test]
    fn decodes_device_descriptor_bus_and_serials() {
        let facts = decode_device_descriptor(&descriptor(
            17,
            "UMIS UPJYJ1TBMNV",
            "1.0L",
            "5CDF_B806_1090_9BA5.",
        ))
        .unwrap();
        assert_eq!(facts.bus_type, 17);
        assert!(!facts.removable_media);
        assert_eq!(facts.product.as_deref(), Some("UMIS UPJYJ1TBMNV"));
        assert_eq!(facts.revision.as_deref(), Some("1.0L"));
        assert_eq!(facts.serial.as_deref(), Some("5CDF_B806_1090_9BA5."));
    }

    #[test]
    fn descriptor_without_string_offsets_reports_no_strings() {
        let mut bytes = descriptor(17, "X", "Y", "Z");
        bytes[16..20].copy_from_slice(&0u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&0u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&0u32.to_le_bytes());
        let facts = decode_device_descriptor(&bytes).unwrap();
        assert!(facts.product.is_none());
        assert!(facts.revision.is_none());
        assert!(facts.serial.is_none());
        assert_eq!(facts.bus_type, 17);
    }

    #[test]
    fn short_descriptor_is_channel_unavailable() {
        let error = decode_device_descriptor(&[0x28, 0, 0, 0]).unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn oversized_declared_descriptor_size_is_rejected() {
        let mut bytes = descriptor(17, "M", "R", "S");
        bytes[4..8].copy_from_slice(&9000u32.to_le_bytes());
        let error = decode_device_descriptor(&bytes).unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn decodes_identify_controller_identity() {
        let identity = decode_nvme_identify_controller(&identify_controller(
            "FDBS26010EA61912B1",
            "UMIS UPJYJ1TBMNV1QWY",
            "1.0L06D1",
        ))
        .unwrap();
        assert_eq!(identity.serial, "FDBS26010EA61912B1");
        assert_eq!(identity.model, "UMIS UPJYJ1TBMNV1QWY");
        assert_eq!(identity.firmware, "1.0L06D1");
    }

    #[test]
    fn protocol_reply_with_wrong_data_type_is_rejected() {
        let mut bytes = identify_controller("SN", "MN", "FR");
        bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
        let error = decode_nvme_identify_controller(&bytes).unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn protocol_reply_with_short_payload_is_rejected() {
        let mut bytes = smart_log(302, 8);
        bytes.truncate(200);
        let error = decode_nvme_smart_log(&bytes).unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn decodes_smart_log_counters() {
        let log = decode_nvme_smart_log(&smart_log(302, 8)).unwrap();
        assert_eq!(log.temperature_kelvin, 302);
        assert_eq!(log.percentage_used, 8);
        assert_eq!(log.available_spare_percent, 100);
        assert_eq!(log.available_spare_threshold_percent, 10);
        assert_eq!(log.power_on_hours, 5_633);
        assert_eq!(log.power_cycles, 161);
        assert_eq!(log.unsafe_shutdowns, 51);
        assert_eq!(log.media_errors, 0);
        assert_eq!(log.data_units_read, 110_518_068);
        assert_eq!(log.data_units_written, 68_841_344);
    }

    #[test]
    fn temperature_descriptor_drops_unset_thresholds() {
        let facts = decode_temperature_descriptor(&temperature_descriptor()).unwrap();
        assert_eq!(facts.warning_celsius, 84);
        assert_eq!(facts.critical_celsius, 87);
        assert_eq!(facts.sensors.len(), 3);
        assert_eq!(facts.sensors[0].temperature_celsius, 28);
        assert_eq!(facts.sensors[0].over_threshold_celsius, Some(84));
        assert_eq!(facts.sensors[1].over_threshold_celsius, None);
    }

    #[test]
    fn health_codes_are_not_promoted_to_healthy() {
        assert_eq!(decode_management_status(&management_status(3)).unwrap(), StorageHealth::Healthy);
        assert_eq!(decode_management_status(&management_status(2)).unwrap(), StorageHealth::Warning);
        assert_eq!(decode_management_status(&management_status(1)).unwrap(), StorageHealth::Unhealthy);
        assert_eq!(decode_management_status(&management_status(0)).unwrap(), StorageHealth::Unknown);
        assert_eq!(decode_management_status(&management_status(9)).unwrap(), StorageHealth::Unknown);
    }

    #[test]
    fn geometry_reads_disksize_after_the_geometry_block() {
        let facts = decode_geometry(&geometry()).unwrap();
        assert_eq!(facts.size_bytes, 1_024_209_543_168);
        assert_eq!(facts.logical_sector_size, Some(512));
    }

    #[test]
    fn zero_length_geometry_is_rejected() {
        let mut bytes = geometry();
        bytes[24..32].copy_from_slice(&0u64.to_le_bytes());
        let error = decode_geometry(&bytes).unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn nvme_device_reports_fields_with_their_sources() {
        let device = only_device(WindowsStorage::new(nvme_probe()).describe_all().unwrap());
        assert_eq!(device.model.as_deref(), Some("UMIS UPJYJ1TBMNV1QWY"));
        assert_eq!(device.serial_number.as_deref(), Some("FDBS26010EA61912B1"));
        assert_eq!(device.device_serial.as_deref(), Some("5CDF_B806_1090_9BA5."));
        assert_eq!(device.firmware_version.as_deref(), Some("1.0L06D1"));
        assert_eq!(device.bus_type, Some(StorageBusType::Nvme));
        assert_eq!(device.media_type, Some(StorageMediaType::SolidState));
        assert_eq!(device.size_bytes, Some(1_024_209_543_168));
        assert_eq!(device.health, Some(StorageHealth::Healthy));
        assert_eq!(device.temperature_celsius, Some(28));
        assert_eq!(device.percentage_used, Some(8));
        assert_eq!(device.power_on_hours, Some(5_633));
        assert_eq!(device.fields.serial_number, SRC_IDENTIFY);
        assert_eq!(device.fields.device_serial, SRC_DESCRIPTOR_SERIAL);
        assert_eq!(device.fields.percentage_used, SRC_WEAR);
        assert_eq!(device.fields.power_on_hours, SRC_POWER_ON_HOURS);
        assert_eq!(device.fields.temperature, SRC_TEMPERATURE_PROPERTY);
        assert_eq!(device.fields.health, SRC_HEALTH);
        assert!(device.issues.is_empty());
    }

    #[test]
    fn refused_log_page_becomes_an_issue_not_an_error_or_zero() {
        let probe = nvme_probe().refuse(StorageQuery::NvmeSmartLog);
        let device = only_device(WindowsStorage::new(probe).describe_all().unwrap());
        assert_eq!(device.percentage_used, None);
        assert_eq!(device.power_on_hours, None);
        assert_eq!(device.media_errors, None);
        assert_eq!(device.issues.len(), 1);
        assert_eq!(device.issues[0].source, SRC_WEAR);
        // The temperature descriptor still answered, so the composite reading
        // is still reported.
        assert_eq!(device.temperature_celsius, Some(28));
    }

    #[test]
    fn refused_temperature_descriptor_falls_back_to_the_log_page_label() {
        let probe = nvme_probe().refuse(StorageQuery::DeviceTemperature);
        let device = only_device(WindowsStorage::new(probe).describe_all().unwrap());
        assert_eq!(device.temperature_celsius, Some(29));
        assert_eq!(device.fields.temperature, SRC_TEMPERATURE_LOG);
        assert_eq!(device.temperature_warning_celsius, None);
    }

    #[test]
    fn malformed_reply_aborts_instead_of_describing_a_partial_device() {
        let probe = nvme_probe().answer(StorageQuery::DeviceDescriptor, vec![0u8; 8]);
        let error = WindowsStorage::new(probe).describe_all().unwrap_err();
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn empty_enumeration_is_reported_as_an_issue_not_an_error() {
        let status = WindowsStorage::new(FakeProbe::default()).describe_all().unwrap();
        assert!(status.devices.is_empty());
        assert_eq!(status.issues.len(), 1);
        assert_eq!(status.issues[0].source, "GUID_DEVINTERFACE_DISK");
    }

    #[test]
    fn non_nvme_device_uses_the_seek_penalty_descriptor_for_media_type() {
        let mut penalty = vec![0u8; 12];
        penalty[0..4].copy_from_slice(&12u32.to_le_bytes());
        penalty[4..8].copy_from_slice(&12u32.to_le_bytes());
        let probe = FakeProbe::default()
            .with_disk()
            .answer(StorageQuery::DeviceDescriptor, descriptor(11, "SATA SSD", "R1", "SN1"))
            .answer(StorageQuery::SeekPenalty, penalty)
            .answer(StorageQuery::DeviceManagementStatus, management_status(3));
        let device = only_device(WindowsStorage::new(probe).describe_all().unwrap());
        assert_eq!(device.bus_type, Some(StorageBusType::Sata));
        assert_eq!(device.media_type, Some(StorageMediaType::SolidState));
        assert_eq!(device.fields.media_type, SRC_MEDIA_SEEK);
        // No NVMe requests are issued for a non-NVMe bus.
        assert_eq!(device.model, None);
        assert_eq!(device.percentage_used, None);
        assert!(device.issues.is_empty());
    }
}
