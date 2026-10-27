use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Operating system identity read from `Win32_OperatingSystem`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct OperatingSystemInventory {
    pub caption: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<String>,
    pub architecture: Option<String>,
}

/// Motherboard identity read from `Win32_BaseBoard`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MotherboardInventory {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial_number: Option<String>,
    pub version: Option<String>,
}

/// System/product identity from `Win32_ComputerSystemProduct` and
/// `Win32_ComputerSystem`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ProductInventory {
    pub name: Option<String>,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub identifying_number: Option<String>,
    pub uuid: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub family: Option<String>,
}

/// BIOS and embedded-controller firmware identity read from `Win32_BIOS`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct BiosInventory {
    pub manufacturer: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub smbios_bios_version: Option<String>,
    pub version_strings: Option<Vec<String>>,
    /// The CIM datetime text, preserved without an implicit timezone conversion.
    pub release_date: Option<String>,
    pub serial_number: Option<String>,
    pub status: Option<String>,
    pub smbios_major_version: Option<u16>,
    pub smbios_minor_version: Option<u16>,
    pub embedded_controller_major_version: Option<u8>,
    pub embedded_controller_minor_version: Option<u8>,
}

/// Processor identity and fixed topology read from `Win32_Processor`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CpuInventory {
    pub device_id: Option<String>,
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub processor_id: Option<String>,
    pub socket_designation: Option<String>,
    pub number_of_cores: Option<u32>,
    pub number_of_logical_processors: Option<u32>,
    pub max_clock_speed_mhz: Option<u32>,
}

/// One physical memory module read from `Win32_PhysicalMemory`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct MemoryModuleInventory {
    pub device_locator: Option<String>,
    pub bank_label: Option<String>,
    pub capacity_bytes: Option<u64>,
    pub speed_mhz: Option<u32>,
    pub configured_clock_speed_mhz: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub serial_number: Option<String>,
}

/// One graphics controller read from `Win32_VideoController`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct GpuInventory {
    pub pnp_device_id: Option<String>,
    pub name: Option<String>,
    pub adapter_compatibility: Option<String>,
    pub video_processor: Option<String>,
    pub driver_version: Option<String>,
    pub status: Option<String>,
}

/// One physical disk read from `Win32_DiskDrive`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct DiskInventory {
    pub device_id: Option<String>,
    pub pnp_device_id: Option<String>,
    pub model: Option<String>,
    pub size_bytes: Option<u64>,
    pub status: Option<String>,
    pub serial_number: Option<String>,
    pub firmware_revision: Option<String>,
    pub interface_type: Option<String>,
}

/// One physical network adapter read from `Win32_NetworkAdapter`.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NetworkAdapterInventory {
    pub device_id: Option<String>,
    pub pnp_device_id: Option<String>,
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub mac_address: Option<String>,
    pub connection_id: Option<String>,
    pub physical_adapter: Option<bool>,
    pub net_enabled: Option<bool>,
}

/// One firmware-class device driver read from `Win32_PnPSignedDriver`.
///
/// `driver_version` is the installed firmware update driver's package version;
/// it is not represented as independent readback of the device's flash contents.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FirmwareInventory {
    pub device_id: Option<String>,
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub class_guid: Option<String>,
    pub driver_provider: Option<String>,
    pub driver_version: Option<String>,
    pub inf_name: Option<String>,
    pub status: Option<String>,
}

/// A complete, bounded inventory snapshot.
///
/// Optional properties remain `None` when the provider omits them or returns a
/// value that cannot be represented by the field's type. Serial numbers and
/// other identifiers use the same conventions as the existing battery inventory.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InventorySnapshot {
    pub operating_system: Option<OperatingSystemInventory>,
    pub motherboard: Option<MotherboardInventory>,
    pub product: Option<ProductInventory>,
    pub bios: Option<BiosInventory>,
    pub cpus: Vec<CpuInventory>,
    pub memory_modules: Vec<MemoryModuleInventory>,
    pub gpus: Vec<GpuInventory>,
    pub disks: Vec<DiskInventory>,
    pub network_adapters: Vec<NetworkAdapterInventory>,
    pub firmware: Vec<FirmwareInventory>,
}

/// A stable, typed field identifier used by inventory comparisons.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryField {
    OperatingSystemCaption,
    OperatingSystemVersion,
    OperatingSystemBuildNumber,
    OperatingSystemArchitecture,
    MotherboardManufacturer,
    MotherboardProduct,
    MotherboardSerialNumber,
    MotherboardVersion,
    ProductName,
    ProductVendor,
    ProductVersion,
    ProductIdentifyingNumber,
    ProductUuid,
    ProductManufacturer,
    ProductModel,
    ProductFamily,
    BiosManufacturer,
    BiosName,
    BiosVersion,
    BiosSmbiosBiosVersion,
    BiosVersionStrings,
    BiosReleaseDate,
    BiosSerialNumber,
    BiosStatus,
    BiosSmbiosMajorVersion,
    BiosSmbiosMinorVersion,
    BiosEmbeddedControllerMajorVersion,
    BiosEmbeddedControllerMinorVersion,
    CpuDeviceId,
    CpuName,
    CpuManufacturer,
    CpuProcessorId,
    CpuSocketDesignation,
    CpuNumberOfCores,
    CpuNumberOfLogicalProcessors,
    CpuMaxClockSpeedMhz,
    MemoryDeviceLocator,
    MemoryBankLabel,
    MemoryCapacityBytes,
    MemorySpeedMhz,
    MemoryConfiguredClockSpeedMhz,
    MemoryManufacturer,
    MemoryPartNumber,
    MemorySerialNumber,
    GpuPnpDeviceId,
    GpuName,
    GpuAdapterCompatibility,
    GpuVideoProcessor,
    GpuDriverVersion,
    GpuStatus,
    DiskDeviceId,
    DiskPnpDeviceId,
    DiskModel,
    DiskSizeBytes,
    DiskStatus,
    DiskSerialNumber,
    DiskFirmwareRevision,
    DiskInterfaceType,
    NetworkAdapterDeviceId,
    NetworkAdapterPnpDeviceId,
    NetworkAdapterName,
    NetworkAdapterManufacturer,
    NetworkAdapterMacAddress,
    NetworkAdapterConnectionId,
    NetworkAdapterPhysicalAdapter,
    NetworkAdapterNetEnabled,
    FirmwareDeviceId,
    FirmwareName,
    FirmwareManufacturer,
    FirmwareClassGuid,
    FirmwareDriverProvider,
    FirmwareDriverVersion,
    FirmwareInfName,
    FirmwareStatus,
}

/// Changed fields and typed before/after records for one inventory identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InventoryRecordDiff<T> {
    pub key: String,
    pub changed_fields: Vec<InventoryField>,
    pub baseline: T,
    pub current: T,
}

/// Comparison of one inventory module.
///
/// Equal modules have empty `added`, `removed`, and `changed` collections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InventoryModuleDiff<T> {
    pub equal: bool,
    pub added: Vec<T>,
    pub removed: Vec<T>,
    pub changed: Vec<InventoryRecordDiff<T>>,
}

impl<T> InventoryModuleDiff<T> {
    #[must_use]
    pub const fn is_equal(&self) -> bool {
        self.equal
    }

    fn new(added: Vec<T>, removed: Vec<T>, changed: Vec<InventoryRecordDiff<T>>) -> Self {
        let equal = added.is_empty() && removed.is_empty() && changed.is_empty();
        Self {
            equal,
            added,
            removed,
            changed,
        }
    }
}

/// Typed comparison of two complete inventory snapshots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InventoryDiff {
    pub equal: bool,
    pub operating_system: InventoryModuleDiff<OperatingSystemInventory>,
    pub motherboard: InventoryModuleDiff<MotherboardInventory>,
    pub product: InventoryModuleDiff<ProductInventory>,
    pub bios: InventoryModuleDiff<BiosInventory>,
    pub cpus: InventoryModuleDiff<CpuInventory>,
    pub memory_modules: InventoryModuleDiff<MemoryModuleInventory>,
    pub gpus: InventoryModuleDiff<GpuInventory>,
    pub disks: InventoryModuleDiff<DiskInventory>,
    pub network_adapters: InventoryModuleDiff<NetworkAdapterInventory>,
    pub firmware: InventoryModuleDiff<FirmwareInventory>,
}

impl InventoryDiff {
    #[must_use]
    pub const fn is_equal(&self) -> bool {
        self.equal
    }
}

impl InventorySnapshot {
    /// Compare this baseline with a current snapshot.
    ///
    /// Collection order is ignored. Records are grouped by stable public-CIM
    /// identity, with deterministic fallback identities for providers that omit
    /// those identifiers. Duplicate identities are retained rather than
    /// collapsed.
    #[must_use]
    pub fn diff(&self, current: &Self) -> InventoryDiff {
        let operating_system = compare_optional(
            self.operating_system.as_ref(),
            current.operating_system.as_ref(),
        );
        let motherboard = compare_optional(self.motherboard.as_ref(), current.motherboard.as_ref());
        let product = compare_optional(self.product.as_ref(), current.product.as_ref());
        let bios = compare_optional(self.bios.as_ref(), current.bios.as_ref());
        let cpus = compare_collection(&self.cpus, &current.cpus);
        let memory_modules = compare_collection(&self.memory_modules, &current.memory_modules);
        let gpus = compare_collection(&self.gpus, &current.gpus);
        let disks = compare_collection(&self.disks, &current.disks);
        let network_adapters =
            compare_collection(&self.network_adapters, &current.network_adapters);
        let firmware = compare_collection(&self.firmware, &current.firmware);
        let equal = operating_system.equal
            && motherboard.equal
            && product.equal
            && bios.equal
            && cpus.equal
            && memory_modules.equal
            && gpus.equal
            && disks.equal
            && network_adapters.equal
            && firmware.equal;

        InventoryDiff {
            equal,
            operating_system,
            motherboard,
            product,
            bios,
            cpus,
            memory_modules,
            gpus,
            disks,
            network_adapters,
            firmware,
        }
    }
}

pub trait InventoryRecord: Clone + Ord {
    fn key(&self) -> String;
    fn changed_fields(&self, current: &Self) -> Vec<InventoryField>;
}

impl<T> InventoryRecordDiff<T>
where
    T: InventoryRecord,
{
    fn new(baseline: &T, current: &T) -> Self {
        Self {
            key: baseline.key(),
            changed_fields: baseline.changed_fields(current),
            baseline: baseline.clone(),
            current: current.clone(),
        }
    }
}

fn compare_optional<T>(baseline: Option<&T>, current: Option<&T>) -> InventoryModuleDiff<T>
where
    T: InventoryRecord,
{
    match (baseline, current) {
        (None, None) => InventoryModuleDiff::new(Vec::new(), Vec::new(), Vec::new()),
        (None, Some(current)) => {
            InventoryModuleDiff::new(vec![current.clone()], Vec::new(), Vec::new())
        }
        (Some(baseline), None) => {
            InventoryModuleDiff::new(Vec::new(), vec![baseline.clone()], Vec::new())
        }
        (Some(baseline), Some(current)) if baseline == current => {
            InventoryModuleDiff::new(Vec::new(), Vec::new(), Vec::new())
        }
        (Some(baseline), Some(current)) => InventoryModuleDiff::new(
            Vec::new(),
            Vec::new(),
            vec![InventoryRecordDiff::new(baseline, current)],
        ),
    }
}

fn compare_collection<T>(baseline: &[T], current: &[T]) -> InventoryModuleDiff<T>
where
    T: InventoryRecord,
{
    let mut baseline_groups = group_by_key(baseline);
    let mut current_groups = group_by_key(current);
    let keys: BTreeSet<String> = baseline_groups
        .keys()
        .chain(current_groups.keys())
        .cloned()
        .collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for key in keys {
        let mut before = baseline_groups.remove(&key).unwrap_or_default();
        let mut after = current_groups.remove(&key).unwrap_or_default();
        before.sort();
        after.sort();
        let mut before_index = 0;
        let mut after_index = 0;
        let mut before_remaining = Vec::new();
        let mut after_remaining = Vec::new();

        // Pair exact records first. Otherwise removing one of several records
        // with the same fallback identity could be misreported as a mutation.
        while before_index < before.len() && after_index < after.len() {
            match before[before_index].cmp(after[after_index]) {
                std::cmp::Ordering::Equal => {
                    before_index += 1;
                    after_index += 1;
                }
                std::cmp::Ordering::Less => {
                    before_remaining.push(before[before_index]);
                    before_index += 1;
                }
                std::cmp::Ordering::Greater => {
                    after_remaining.push(after[after_index]);
                    after_index += 1;
                }
            }
        }
        before_remaining.extend_from_slice(&before[before_index..]);
        after_remaining.extend_from_slice(&after[after_index..]);

        let shared = before_remaining.len().min(after_remaining.len());
        for index in 0..shared {
            changed.push(InventoryRecordDiff::new(
                before_remaining[index],
                after_remaining[index],
            ));
        }
        removed.extend(
            before_remaining[shared..]
                .iter()
                .map(|record| T::clone(*record)),
        );
        added.extend(
            after_remaining[shared..]
                .iter()
                .map(|record| T::clone(*record)),
        );
    }

    InventoryModuleDiff::new(added, removed, changed)
}

fn group_by_key<T>(records: &[T]) -> BTreeMap<String, Vec<&T>>
where
    T: InventoryRecord,
{
    let mut groups = BTreeMap::new();
    for record in records {
        groups
            .entry(record.key())
            .or_insert_with(Vec::new)
            .push(record);
    }
    groups
}

fn record_key(primary: Option<&str>, fallback: &[(&str, Option<&str>)]) -> String {
    if let Some(primary) = primary {
        return primary.to_owned();
    }

    let mut key = String::new();
    for (name, value) in fallback {
        if !key.is_empty() {
            key.push('|');
        }
        key.push_str(name);
        key.push('=');
        key.push_str(value.unwrap_or_default());
    }
    key
}

macro_rules! changed_fields {
    ($baseline:expr, $current:expr; $($field:ident => $variant:ident),+ $(,)?) => {{
        let mut changed = Vec::new();
        $(
            if $baseline.$field != $current.$field {
                changed.push(InventoryField::$variant);
            }
        )+
        changed
    }};
}

impl InventoryRecord for OperatingSystemInventory {
    fn key(&self) -> String {
        "operating_system".into()
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            caption => OperatingSystemCaption,
            version => OperatingSystemVersion,
            build_number => OperatingSystemBuildNumber,
            architecture => OperatingSystemArchitecture,
        )
    }
}

impl InventoryRecord for MotherboardInventory {
    fn key(&self) -> String {
        "motherboard".into()
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            manufacturer => MotherboardManufacturer,
            product => MotherboardProduct,
            serial_number => MotherboardSerialNumber,
            version => MotherboardVersion,
        )
    }
}

impl InventoryRecord for ProductInventory {
    fn key(&self) -> String {
        "product".into()
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            name => ProductName,
            vendor => ProductVendor,
            version => ProductVersion,
            identifying_number => ProductIdentifyingNumber,
            uuid => ProductUuid,
            manufacturer => ProductManufacturer,
            model => ProductModel,
            family => ProductFamily,
        )
    }
}

impl InventoryRecord for BiosInventory {
    fn key(&self) -> String {
        "bios".into()
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            manufacturer => BiosManufacturer,
            name => BiosName,
            version => BiosVersion,
            smbios_bios_version => BiosSmbiosBiosVersion,
            version_strings => BiosVersionStrings,
            release_date => BiosReleaseDate,
            serial_number => BiosSerialNumber,
            status => BiosStatus,
            smbios_major_version => BiosSmbiosMajorVersion,
            smbios_minor_version => BiosSmbiosMinorVersion,
            embedded_controller_major_version => BiosEmbeddedControllerMajorVersion,
            embedded_controller_minor_version => BiosEmbeddedControllerMinorVersion,
        )
    }
}

impl InventoryRecord for CpuInventory {
    fn key(&self) -> String {
        record_key(
            self.device_id.as_deref(),
            &[
                ("processor_id", self.processor_id.as_deref()),
                ("socket", self.socket_designation.as_deref()),
                ("name", self.name.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            device_id => CpuDeviceId,
            name => CpuName,
            manufacturer => CpuManufacturer,
            processor_id => CpuProcessorId,
            socket_designation => CpuSocketDesignation,
            number_of_cores => CpuNumberOfCores,
            number_of_logical_processors => CpuNumberOfLogicalProcessors,
            max_clock_speed_mhz => CpuMaxClockSpeedMhz,
        )
    }
}

impl InventoryRecord for MemoryModuleInventory {
    fn key(&self) -> String {
        record_key(
            self.device_locator.as_deref(),
            &[
                ("bank_label", self.bank_label.as_deref()),
                ("serial_number", self.serial_number.as_deref()),
                ("part_number", self.part_number.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            device_locator => MemoryDeviceLocator,
            bank_label => MemoryBankLabel,
            capacity_bytes => MemoryCapacityBytes,
            speed_mhz => MemorySpeedMhz,
            configured_clock_speed_mhz => MemoryConfiguredClockSpeedMhz,
            manufacturer => MemoryManufacturer,
            part_number => MemoryPartNumber,
            serial_number => MemorySerialNumber,
        )
    }
}

impl InventoryRecord for GpuInventory {
    fn key(&self) -> String {
        record_key(
            self.pnp_device_id.as_deref(),
            &[
                ("name", self.name.as_deref()),
                (
                    "adapter_compatibility",
                    self.adapter_compatibility.as_deref(),
                ),
                ("video_processor", self.video_processor.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            pnp_device_id => GpuPnpDeviceId,
            name => GpuName,
            adapter_compatibility => GpuAdapterCompatibility,
            video_processor => GpuVideoProcessor,
            driver_version => GpuDriverVersion,
            status => GpuStatus,
        )
    }
}

impl InventoryRecord for DiskInventory {
    fn key(&self) -> String {
        record_key(
            self.pnp_device_id.as_deref(),
            &[
                ("device_id", self.device_id.as_deref()),
                ("serial_number", self.serial_number.as_deref()),
                ("model", self.model.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            device_id => DiskDeviceId,
            pnp_device_id => DiskPnpDeviceId,
            model => DiskModel,
            size_bytes => DiskSizeBytes,
            status => DiskStatus,
            serial_number => DiskSerialNumber,
            firmware_revision => DiskFirmwareRevision,
            interface_type => DiskInterfaceType,
        )
    }
}

impl InventoryRecord for NetworkAdapterInventory {
    fn key(&self) -> String {
        record_key(
            self.pnp_device_id.as_deref(),
            &[
                ("device_id", self.device_id.as_deref()),
                ("mac_address", self.mac_address.as_deref()),
                ("name", self.name.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            device_id => NetworkAdapterDeviceId,
            pnp_device_id => NetworkAdapterPnpDeviceId,
            name => NetworkAdapterName,
            manufacturer => NetworkAdapterManufacturer,
            mac_address => NetworkAdapterMacAddress,
            connection_id => NetworkAdapterConnectionId,
            physical_adapter => NetworkAdapterPhysicalAdapter,
            net_enabled => NetworkAdapterNetEnabled,
        )
    }
}

impl InventoryRecord for FirmwareInventory {
    fn key(&self) -> String {
        record_key(
            self.device_id.as_deref(),
            &[
                ("name", self.name.as_deref()),
                ("manufacturer", self.manufacturer.as_deref()),
                ("class_guid", self.class_guid.as_deref()),
            ],
        )
    }

    fn changed_fields(&self, current: &Self) -> Vec<InventoryField> {
        changed_fields!(self, current;
            device_id => FirmwareDeviceId,
            name => FirmwareName,
            manufacturer => FirmwareManufacturer,
            class_guid => FirmwareClassGuid,
            driver_provider => FirmwareDriverProvider,
            driver_version => FirmwareDriverVersion,
            inf_name => FirmwareInfName,
            status => FirmwareStatus,
        )
    }
}
