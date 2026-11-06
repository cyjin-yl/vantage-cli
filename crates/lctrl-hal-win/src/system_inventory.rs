//! Bounded, read-only Windows inventory over documented public CIM classes.
//!
//! A capture performs a fixed set of eleven property-limited `SELECT` queries in
//! `root\CIMV2`. It never invokes CIM methods, vendor interfaces, firmware
//! variables, or diagnostics providers. Empty classes produce an absent module
//! (or an empty collection); missing/non-convertible properties remain `None`.

use lctrl_core::{
    BiosInventory, CpuInventory, DiskInventory, FirmwareInventory, GpuInventory, InventorySnapshot,
    MemoryModuleInventory, MotherboardInventory, NetworkAdapterInventory, OperatingSystemInventory,
    ProductInventory, Result, SystemIdentity,
};

use crate::{WmiObject, WmiTransport, WmiValue};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";

const OPERATING_SYSTEM_QUERY: &str =
    "SELECT Caption, Version, BuildNumber, OSArchitecture FROM Win32_OperatingSystem";
const PRODUCT_QUERY: &str =
    "SELECT Name, Vendor, Version, IdentifyingNumber, UUID FROM Win32_ComputerSystemProduct";
const COMPUTER_SYSTEM_QUERY: &str =
    "SELECT Manufacturer, Model, SystemFamily FROM Win32_ComputerSystem";
const MOTHERBOARD_QUERY: &str =
    "SELECT Manufacturer, Product, SerialNumber, Version FROM Win32_BaseBoard";
const BIOS_QUERY: &str = "SELECT Manufacturer, Name, Version, SMBIOSBIOSVersion, BIOSVersion, ReleaseDate, SerialNumber, Status, SMBIOSMajorVersion, SMBIOSMinorVersion, EmbeddedControllerMajorVersion, EmbeddedControllerMinorVersion FROM Win32_BIOS";
const CPU_QUERY: &str = "SELECT DeviceID, Name, Manufacturer, ProcessorId, SocketDesignation, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed FROM Win32_Processor";
const MEMORY_QUERY: &str = "SELECT DeviceLocator, BankLabel, Capacity, Speed, ConfiguredClockSpeed, Manufacturer, PartNumber, SerialNumber FROM Win32_PhysicalMemory";
const GPU_QUERY: &str = "SELECT PNPDeviceID, Name, AdapterCompatibility, VideoProcessor, DriverVersion, Status FROM Win32_VideoController";
const DISK_QUERY: &str = "SELECT DeviceID, PNPDeviceID, Model, Size, Status, SerialNumber, FirmwareRevision, InterfaceType FROM Win32_DiskDrive";
const NETWORK_QUERY: &str = "SELECT DeviceID, PNPDeviceID, Name, Manufacturer, MACAddress, NetConnectionID, PhysicalAdapter, NetEnabled FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE";
const FIRMWARE_QUERY: &str = "SELECT DeviceID, DeviceName, Manufacturer, ClassGuid, DriverProviderName, DriverVersion, InfName, Status FROM Win32_PnPSignedDriver WHERE DeviceClass = 'FIRMWARE'";

/// A standalone rich inventory service.
///
/// This intentionally does not alter the existing `WindowsSystemInventory`
/// diagnostics/update service: inventory records are facts, not scan outcomes.
#[derive(Debug)]
pub struct WindowsInventory<W> {
    wmi: W,
}

impl<W> WindowsInventory<W> {
    #[must_use]
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

impl<W> WindowsInventory<W>
where
    W: WmiTransport,
{
    /// Capture the current system and firmware inventory.
    ///
    /// Any transport/query failure is returned instead of being converted into
    /// a seemingly valid partial inventory.
    pub fn capture(&self) -> Result<InventorySnapshot> {
        let operating_system_objects = self.wmi.query(ROOT_CIMV2, OPERATING_SYSTEM_QUERY)?;
        let product_objects = self.wmi.query(ROOT_CIMV2, PRODUCT_QUERY)?;
        let computer_system_objects = self.wmi.query(ROOT_CIMV2, COMPUTER_SYSTEM_QUERY)?;
        let motherboard_objects = self.wmi.query(ROOT_CIMV2, MOTHERBOARD_QUERY)?;
        let bios_objects = self.wmi.query(ROOT_CIMV2, BIOS_QUERY)?;
        let cpu_objects = self.wmi.query(ROOT_CIMV2, CPU_QUERY)?;
        let memory_objects = self.wmi.query(ROOT_CIMV2, MEMORY_QUERY)?;
        let gpu_objects = self.wmi.query(ROOT_CIMV2, GPU_QUERY)?;
        let disk_objects = self.wmi.query(ROOT_CIMV2, DISK_QUERY)?;
        let network_objects = self.wmi.query(ROOT_CIMV2, NETWORK_QUERY)?;
        let firmware_objects = self.wmi.query(ROOT_CIMV2, FIRMWARE_QUERY)?;

        let product_object = product_objects.first();
        let computer_system_object = computer_system_objects.first();
        let product = if product_object.is_none() && computer_system_object.is_none() {
            None
        } else {
            Some(product_from_wmi(product_object, computer_system_object))
        };

        Ok(InventorySnapshot {
            operating_system: operating_system_objects
                .first()
                .map(operating_system_from_wmi),
            motherboard: motherboard_objects.first().map(motherboard_from_wmi),
            product,
            bios: bios_objects.first().map(bios_from_wmi),
            cpus: cpu_objects.iter().map(cpu_from_wmi).collect(),
            memory_modules: memory_objects.iter().map(memory_from_wmi).collect(),
            gpus: gpu_objects.iter().map(gpu_from_wmi).collect(),
            disks: disk_objects.iter().map(disk_from_wmi).collect(),
            network_adapters: network_objects.iter().map(network_from_wmi).collect(),
            firmware: firmware_objects.iter().map(firmware_from_wmi).collect(),
        })
    }

    /// Capture only the identity modules: product, baseboard, and BIOS.
    ///
    /// This runs the four identity queries of [`Self::capture`] and none of the
    /// CPU/memory/GPU/disk/network/firmware ones, so `vantage info` stays a
    /// bounded read instead of a full inventory sweep.
    pub fn capture_identity(&self) -> Result<SystemIdentity> {
        identity_from_wmi(&self.wmi)
    }
}

/// Read the identity modules through `wmi`.
///
/// Field provenance (all in `root\CIMV2`):
///
/// | Field | Source |
/// |---|---|
/// | machine type model (MTM) | `Win32_ComputerSystemProduct.Name`, mirrored by `Win32_ComputerSystem.Model` |
/// | marketing product version | `Win32_ComputerSystemProduct.Version` |
/// | serial number | `Win32_ComputerSystemProduct.IdentifyingNumber` |
/// | product UUID | `Win32_ComputerSystemProduct.UUID` |
/// | family | `Win32_ComputerSystem.SystemFamily` |
/// | baseboard product/version/serial | `Win32_BaseBoard.Product`/`.Version`/`.SerialNumber` |
/// | BIOS version | `Win32_BIOS.SMBIOSBIOSVersion` (`Win32_BIOS.Version` is a firmware vendor/ordinal string, never the version) |
/// | BIOS release date | `Win32_BIOS.ReleaseDate`, the DMTF datetime text as delivered |
/// | SMBIOS version | `Win32_BIOS.SMBIOSMajorVersion`/`.SMBIOSMinorVersion` |
/// | EC firmware major/minor | `Win32_BIOS.EmbeddedControllerMajorVersion`/`.EmbeddedControllerMinorVersion` (SMBIOS Type 0 offsets 0x16/0x17) |
///
/// A class that returns no instance leaves its record all-`None`; nothing is
/// substituted from another class or from the registry.
pub(crate) fn identity_from_wmi<W: WmiTransport>(wmi: &W) -> Result<SystemIdentity> {
    let product_objects = wmi.query(ROOT_CIMV2, PRODUCT_QUERY)?;
    let computer_system_objects = wmi.query(ROOT_CIMV2, COMPUTER_SYSTEM_QUERY)?;
    let motherboard_objects = wmi.query(ROOT_CIMV2, MOTHERBOARD_QUERY)?;
    let bios_objects = wmi.query(ROOT_CIMV2, BIOS_QUERY)?;

    Ok(SystemIdentity {
        product: product_from_wmi(product_objects.first(), computer_system_objects.first()),
        motherboard: motherboard_objects
            .first()
            .map(motherboard_from_wmi)
            .unwrap_or_default(),
        bios: bios_objects.first().map(bios_from_wmi).unwrap_or_default(),
    })
}

fn operating_system_from_wmi(object: &WmiObject) -> OperatingSystemInventory {
    OperatingSystemInventory {
        caption: string_field(object, "Caption"),
        version: string_field(object, "Version"),
        build_number: string_field(object, "BuildNumber"),
        architecture: string_field(object, "OSArchitecture"),
    }
}

fn motherboard_from_wmi(object: &WmiObject) -> MotherboardInventory {
    MotherboardInventory {
        manufacturer: string_field(object, "Manufacturer"),
        product: string_field(object, "Product"),
        serial_number: string_field(object, "SerialNumber"),
        version: string_field(object, "Version"),
    }
}

fn product_from_wmi(
    product: Option<&WmiObject>,
    computer_system: Option<&WmiObject>,
) -> ProductInventory {
    ProductInventory {
        name: optional_string_field(product, "Name"),
        vendor: optional_string_field(product, "Vendor"),
        version: optional_string_field(product, "Version"),
        identifying_number: optional_string_field(product, "IdentifyingNumber"),
        uuid: optional_string_field(product, "UUID"),
        manufacturer: optional_string_field(computer_system, "Manufacturer"),
        model: optional_string_field(computer_system, "Model"),
        family: optional_string_field(computer_system, "SystemFamily"),
    }
}

fn bios_from_wmi(object: &WmiObject) -> BiosInventory {
    BiosInventory {
        manufacturer: string_field(object, "Manufacturer"),
        name: string_field(object, "Name"),
        version: string_field(object, "Version"),
        smbios_bios_version: string_field(object, "SMBIOSBIOSVersion"),
        version_strings: string_array_field(object, "BIOSVersion"),
        release_date: string_field(object, "ReleaseDate"),
        serial_number: string_field(object, "SerialNumber"),
        status: string_field(object, "Status"),
        smbios_major_version: u16_field(object, "SMBIOSMajorVersion"),
        smbios_minor_version: u16_field(object, "SMBIOSMinorVersion"),
        embedded_controller_major_version: u8_field(object, "EmbeddedControllerMajorVersion"),
        embedded_controller_minor_version: u8_field(object, "EmbeddedControllerMinorVersion"),
    }
}

fn cpu_from_wmi(object: &WmiObject) -> CpuInventory {
    CpuInventory {
        device_id: string_field(object, "DeviceID"),
        name: string_field(object, "Name"),
        manufacturer: string_field(object, "Manufacturer"),
        processor_id: string_field(object, "ProcessorId"),
        socket_designation: string_field(object, "SocketDesignation"),
        number_of_cores: u32_field(object, "NumberOfCores"),
        number_of_logical_processors: u32_field(object, "NumberOfLogicalProcessors"),
        max_clock_speed_mhz: u32_field(object, "MaxClockSpeed"),
    }
}

fn memory_from_wmi(object: &WmiObject) -> MemoryModuleInventory {
    MemoryModuleInventory {
        device_locator: string_field(object, "DeviceLocator"),
        bank_label: string_field(object, "BankLabel"),
        capacity_bytes: u64_field(object, "Capacity"),
        speed_mhz: u32_field(object, "Speed"),
        configured_clock_speed_mhz: u32_field(object, "ConfiguredClockSpeed"),
        manufacturer: string_field(object, "Manufacturer"),
        part_number: string_field(object, "PartNumber"),
        serial_number: string_field(object, "SerialNumber"),
    }
}

fn gpu_from_wmi(object: &WmiObject) -> GpuInventory {
    GpuInventory {
        pnp_device_id: string_field(object, "PNPDeviceID"),
        name: string_field(object, "Name"),
        adapter_compatibility: string_field(object, "AdapterCompatibility"),
        video_processor: string_field(object, "VideoProcessor"),
        driver_version: string_field(object, "DriverVersion"),
        status: string_field(object, "Status"),
    }
}

fn disk_from_wmi(object: &WmiObject) -> DiskInventory {
    DiskInventory {
        device_id: string_field(object, "DeviceID"),
        pnp_device_id: string_field(object, "PNPDeviceID"),
        model: string_field(object, "Model"),
        size_bytes: u64_field(object, "Size"),
        status: string_field(object, "Status"),
        serial_number: string_field(object, "SerialNumber"),
        firmware_revision: string_field(object, "FirmwareRevision"),
        interface_type: string_field(object, "InterfaceType"),
    }
}

fn network_from_wmi(object: &WmiObject) -> NetworkAdapterInventory {
    NetworkAdapterInventory {
        device_id: string_field(object, "DeviceID"),
        pnp_device_id: string_field(object, "PNPDeviceID"),
        name: string_field(object, "Name"),
        manufacturer: string_field(object, "Manufacturer"),
        mac_address: string_field(object, "MACAddress"),
        connection_id: string_field(object, "NetConnectionID"),
        physical_adapter: bool_field(object, "PhysicalAdapter"),
        net_enabled: bool_field(object, "NetEnabled"),
    }
}

fn firmware_from_wmi(object: &WmiObject) -> FirmwareInventory {
    FirmwareInventory {
        device_id: string_field(object, "DeviceID"),
        name: string_field(object, "DeviceName"),
        manufacturer: string_field(object, "Manufacturer"),
        class_guid: string_field(object, "ClassGuid"),
        driver_provider: string_field(object, "DriverProviderName"),
        driver_version: string_field(object, "DriverVersion"),
        inf_name: string_field(object, "InfName"),
        status: string_field(object, "Status"),
    }
}

fn optional_string_field(object: Option<&WmiObject>, name: &str) -> Option<String> {
    object.and_then(|object| string_field(object, name))
}

fn string_field(object: &WmiObject, name: &str) -> Option<String> {
    match object.get(name) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn string_array_field(object: &WmiObject, name: &str) -> Option<Vec<String>> {
    match object.get(name) {
        Some(WmiValue::Array(values)) => values
            .iter()
            .map(|value| match value {
                WmiValue::String(value) if !value.is_empty() => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn bool_field(object: &WmiObject, name: &str) -> Option<bool> {
    match object.get(name) {
        Some(WmiValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn u64_field(object: &WmiObject, name: &str) -> Option<u64> {
    match object.get(name) {
        Some(WmiValue::U64(value)) => Some(*value),
        Some(WmiValue::U32(value)) => Some(u64::from(*value)),
        Some(WmiValue::U16(value)) => Some(u64::from(*value)),
        Some(WmiValue::U8(value)) => Some(u64::from(*value)),
        Some(WmiValue::I64(value)) => u64::try_from(*value).ok(),
        Some(WmiValue::I32(value)) => u64::try_from(*value).ok(),
        Some(WmiValue::I16(value)) => u64::try_from(*value).ok(),
        Some(WmiValue::I8(value)) => u64::try_from(*value).ok(),
        _ => None,
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    u64_field(object, name).and_then(|value| u32::try_from(value).ok())
}

fn u16_field(object: &WmiObject, name: &str) -> Option<u16> {
    u64_field(object, name).and_then(|value| u16::try_from(value).ok())
}

fn u8_field(object: &WmiObject, name: &str) -> Option<u8> {
    u64_field(object, name).and_then(|value| u8::try_from(value).ok())
}
