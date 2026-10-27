use std::collections::BTreeMap;

use lctrl_core::{
    BiosInventory, CpuInventory, DiskInventory, FirmwareInventory, GpuInventory, InventoryDiff,
    InventoryField, InventoryModuleDiff, InventorySnapshot, MemoryModuleInventory,
    MotherboardInventory, NetworkAdapterInventory, OperatingSystemInventory, ProductInventory,
};
use lctrl_hal_win::{WindowsInventory, WmiObject, WmiTransport, WmiValue};
use parking_lot::Mutex;
use serde_json::Value;

const CLASSES: [&str; 11] = [
    "Win32_OperatingSystem",
    "Win32_ComputerSystemProduct",
    "Win32_ComputerSystem",
    "Win32_BaseBoard",
    "Win32_BIOS",
    "Win32_Processor",
    "Win32_PhysicalMemory",
    "Win32_VideoController",
    "Win32_DiskDrive",
    "Win32_NetworkAdapter",
    "Win32_PnPSignedDriver",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Query {
    namespace: String,
    wql: String,
}

#[derive(Default)]
struct FakeWmi {
    replies: BTreeMap<String, Vec<WmiObject>>,
    queries: Mutex<Vec<Query>>,
}

impl FakeWmi {
    fn new(replies: impl IntoIterator<Item = (&'static str, Vec<WmiObject>)>) -> Self {
        Self {
            replies: replies
                .into_iter()
                .map(|(class, objects)| (class.to_owned(), objects))
                .collect(),
            queries: Mutex::new(Vec::new()),
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        self.queries.lock().push(Query {
            namespace: namespace.into(),
            wql: wql.into(),
        });
        let (_, tail) = wql.split_once(" FROM ").expect("inventory SELECT query");
        let class = tail.split_whitespace().next().expect("CIM class");
        Ok(self.replies.get(class).cloned().unwrap_or_default())
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        unreachable!("read-only inventory must not invoke CIM methods")
    }
}

fn object(entries: &[(&str, WmiValue)]) -> WmiObject {
    entries
        .iter()
        .cloned()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn text(value: &str) -> WmiValue {
    WmiValue::String(value.into())
}

#[test]
fn capture_converts_public_cim_classes_and_preserves_identifiers() {
    let wmi = FakeWmi::new([
        (
            "Win32_OperatingSystem",
            vec![object(&[
                ("Caption", text("Microsoft Windows 11 Pro")),
                ("Version", text("10.0.26100")),
                ("BuildNumber", text("26100")),
                ("OSArchitecture", text("64-bit")),
            ])],
        ),
        (
            "Win32_ComputerSystemProduct",
            vec![object(&[
                ("Name", text("21VG")),
                ("Vendor", text("LENOVO")),
                ("Version", text("ThinkBook 14 G8+ IPH")),
                ("IdentifyingNumber", text("PRODUCT-SERIAL")),
                ("UUID", text("00112233-4455-6677-8899-AABBCCDDEEFF")),
            ])],
        ),
        (
            "Win32_ComputerSystem",
            vec![object(&[
                ("Manufacturer", text("LENOVO")),
                ("Model", text("THINKBOOK_14_G8+_IPH")),
                ("SystemFamily", text("ThinkBook")),
            ])],
        ),
        (
            "Win32_BaseBoard",
            vec![object(&[
                ("Manufacturer", text("LENOVO")),
                ("Product", text("21VG")),
                ("SerialNumber", text("BOARD-SERIAL")),
                ("Version", text("SDK0L77769 WIN")),
            ])],
        ),
        (
            "Win32_BIOS",
            vec![object(&[
                ("Manufacturer", text("LENOVO")),
                ("Name", text("N3GET18W")),
                ("Version", text("LENOVO - 1")),
                ("SMBIOSBIOSVersion", text("N3GET18W")),
                (
                    "BIOSVersion",
                    WmiValue::Array(vec![text("N3GET18W"), text("1.24")]),
                ),
                ("ReleaseDate", text("20260827000000.000000+000")),
                ("SerialNumber", text("BIOS-SERIAL")),
                ("Status", text("OK")),
                ("SMBIOSMajorVersion", WmiValue::U16(3)),
                ("SMBIOSMinorVersion", WmiValue::U16(8)),
                ("EmbeddedControllerMajorVersion", WmiValue::U8(1)),
                ("EmbeddedControllerMinorVersion", WmiValue::U8(24)),
            ])],
        ),
        (
            "Win32_Processor",
            vec![object(&[
                ("DeviceID", text("CPU0")),
                ("Name", text("Example Processor")),
                ("Manufacturer", text("GenuineIntel")),
                ("ProcessorId", text("CPU-IDENTIFIER")),
                ("SocketDesignation", text("Socket 0")),
                ("NumberOfCores", WmiValue::U32(8)),
                ("NumberOfLogicalProcessors", WmiValue::U32(16)),
                ("MaxClockSpeed", WmiValue::U32(4200)),
            ])],
        ),
        (
            "Win32_PhysicalMemory",
            vec![object(&[
                ("DeviceLocator", text("DIMM 0")),
                ("BankLabel", text("BANK 0")),
                ("Capacity", WmiValue::U64(17_179_869_184)),
                ("Speed", WmiValue::U32(6400)),
                ("ConfiguredClockSpeed", WmiValue::U32(5600)),
                ("Manufacturer", text("Memory Vendor")),
                ("PartNumber", text("PART-16G")),
                ("SerialNumber", text("MEMORY-SERIAL")),
            ])],
        ),
        (
            "Win32_VideoController",
            vec![object(&[
                ("PNPDeviceID", text("PCI\\VEN_1002&DEV_1111")),
                ("Name", text("Example GPU")),
                ("AdapterCompatibility", text("GPU Vendor")),
                ("VideoProcessor", text("GPU Processor")),
                ("DriverVersion", text("32.0.1.2")),
                ("Status", text("OK")),
            ])],
        ),
        (
            "Win32_DiskDrive",
            vec![object(&[
                ("DeviceID", text("\\\\.\\PHYSICALDRIVE0")),
                ("PNPDeviceID", text("NVME\\DISK0")),
                ("Model", text("Example NVMe 1TB")),
                ("Size", WmiValue::U64(1_024_204_033_024)),
                ("Status", text("Pred Fail")),
                ("SerialNumber", text("DISK-SERIAL")),
                ("FirmwareRevision", text("FW1.2")),
                ("InterfaceType", text("SCSI")),
            ])],
        ),
        (
            "Win32_NetworkAdapter",
            vec![object(&[
                ("DeviceID", text("7")),
                ("PNPDeviceID", text("PCI\\VEN_8086&DEV_1234")),
                ("Name", text("Example Wi-Fi Adapter")),
                ("Manufacturer", text("Network Vendor")),
                ("MACAddress", text("3C:52:82:00:11:22")),
                ("NetConnectionID", text("Wi-Fi")),
                ("PhysicalAdapter", WmiValue::Bool(true)),
                ("NetEnabled", WmiValue::Bool(true)),
            ])],
        ),
        (
            "Win32_PnPSignedDriver",
            vec![object(&[
                ("DeviceID", text("UEFI\\RES_{SYSTEM-FIRMWARE}")),
                ("DeviceName", text("System Firmware")),
                ("Manufacturer", text("Firmware Vendor")),
                ("ClassGuid", text("{F2E7DD72-6468-4E36-B6F1-6488F42C1B52}")),
                ("DriverProviderName", text("Firmware Provider")),
                ("DriverVersion", text("10.0.1.24")),
                ("InfName", text("firmware.inf")),
                ("Status", text("OK")),
            ])],
        ),
    ]);
    let service = WindowsInventory::new(wmi);

    let snapshot = service.capture().unwrap();
    let expected = InventorySnapshot {
        operating_system: Some(OperatingSystemInventory {
            caption: Some("Microsoft Windows 11 Pro".into()),
            version: Some("10.0.26100".into()),
            build_number: Some("26100".into()),
            architecture: Some("64-bit".into()),
        }),
        motherboard: Some(MotherboardInventory {
            manufacturer: Some("LENOVO".into()),
            product: Some("21VG".into()),
            serial_number: Some("BOARD-SERIAL".into()),
            version: Some("SDK0L77769 WIN".into()),
        }),
        product: Some(ProductInventory {
            name: Some("21VG".into()),
            vendor: Some("LENOVO".into()),
            version: Some("ThinkBook 14 G8+ IPH".into()),
            identifying_number: Some("PRODUCT-SERIAL".into()),
            uuid: Some("00112233-4455-6677-8899-AABBCCDDEEFF".into()),
            manufacturer: Some("LENOVO".into()),
            model: Some("THINKBOOK_14_G8+_IPH".into()),
            family: Some("ThinkBook".into()),
        }),
        bios: Some(BiosInventory {
            manufacturer: Some("LENOVO".into()),
            name: Some("N3GET18W".into()),
            version: Some("LENOVO - 1".into()),
            smbios_bios_version: Some("N3GET18W".into()),
            version_strings: Some(vec!["N3GET18W".into(), "1.24".into()]),
            release_date: Some("20260827000000.000000+000".into()),
            serial_number: Some("BIOS-SERIAL".into()),
            status: Some("OK".into()),
            smbios_major_version: Some(3),
            smbios_minor_version: Some(8),
            embedded_controller_major_version: Some(1),
            embedded_controller_minor_version: Some(24),
        }),
        cpus: vec![CpuInventory {
            device_id: Some("CPU0".into()),
            name: Some("Example Processor".into()),
            manufacturer: Some("GenuineIntel".into()),
            processor_id: Some("CPU-IDENTIFIER".into()),
            socket_designation: Some("Socket 0".into()),
            number_of_cores: Some(8),
            number_of_logical_processors: Some(16),
            max_clock_speed_mhz: Some(4200),
        }],
        memory_modules: vec![MemoryModuleInventory {
            device_locator: Some("DIMM 0".into()),
            bank_label: Some("BANK 0".into()),
            capacity_bytes: Some(17_179_869_184),
            speed_mhz: Some(6400),
            configured_clock_speed_mhz: Some(5600),
            manufacturer: Some("Memory Vendor".into()),
            part_number: Some("PART-16G".into()),
            serial_number: Some("MEMORY-SERIAL".into()),
        }],
        gpus: vec![GpuInventory {
            pnp_device_id: Some("PCI\\VEN_1002&DEV_1111".into()),
            name: Some("Example GPU".into()),
            adapter_compatibility: Some("GPU Vendor".into()),
            video_processor: Some("GPU Processor".into()),
            driver_version: Some("32.0.1.2".into()),
            status: Some("OK".into()),
        }],
        disks: vec![DiskInventory {
            device_id: Some("\\\\.\\PHYSICALDRIVE0".into()),
            pnp_device_id: Some("NVME\\DISK0".into()),
            model: Some("Example NVMe 1TB".into()),
            size_bytes: Some(1_024_204_033_024),
            status: Some("Pred Fail".into()),
            serial_number: Some("DISK-SERIAL".into()),
            firmware_revision: Some("FW1.2".into()),
            interface_type: Some("SCSI".into()),
        }],
        network_adapters: vec![NetworkAdapterInventory {
            device_id: Some("7".into()),
            pnp_device_id: Some("PCI\\VEN_8086&DEV_1234".into()),
            name: Some("Example Wi-Fi Adapter".into()),
            manufacturer: Some("Network Vendor".into()),
            mac_address: Some("3C:52:82:00:11:22".into()),
            connection_id: Some("Wi-Fi".into()),
            physical_adapter: Some(true),
            net_enabled: Some(true),
        }],
        firmware: vec![FirmwareInventory {
            device_id: Some("UEFI\\RES_{SYSTEM-FIRMWARE}".into()),
            name: Some("System Firmware".into()),
            manufacturer: Some("Firmware Vendor".into()),
            class_guid: Some("{F2E7DD72-6468-4E36-B6F1-6488F42C1B52}".into()),
            driver_provider: Some("Firmware Provider".into()),
            driver_version: Some("10.0.1.24".into()),
            inf_name: Some("firmware.inf".into()),
            status: Some("OK".into()),
        }],
    };
    assert_eq!(snapshot, expected);

    let queries = service.wmi().queries.lock();
    assert_eq!(queries.len(), CLASSES.len());
    assert!(
        queries
            .iter()
            .all(|query| query.namespace == "ROOT\\CIMV2")
    );
    assert!(queries.iter().all(|query| !query.wql.contains("SELECT *")));
    assert!(
        queries
            .iter()
            .any(|query| query.wql.contains("WHERE PhysicalAdapter = TRUE"))
    );
    assert!(
        queries
            .iter()
            .any(|query| query.wql.contains("WHERE DeviceClass = 'FIRMWARE'"))
    );
    drop(queries);

    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(json["product"]["identifying_number"], "PRODUCT-SERIAL");
    assert_eq!(json["motherboard"]["serial_number"], "BOARD-SERIAL");
    assert_eq!(json["bios"]["serial_number"], "BIOS-SERIAL");
    assert_eq!(json["memory_modules"][0]["serial_number"], "MEMORY-SERIAL");
    assert_eq!(json["disks"][0]["serial_number"], "DISK-SERIAL");

    let serialized = serde_json::to_string(&snapshot).unwrap();
    let decoded: InventorySnapshot = serde_json::from_str(&serialized).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn missing_optional_properties_stay_explicitly_unknown() {
    let wmi = FakeWmi::new(
        CLASSES
            .into_iter()
            .map(|class| (class, vec![object(&[])])),
    );
    let snapshot = WindowsInventory::new(wmi).capture().unwrap();

    assert_eq!(
        snapshot.operating_system,
        Some(OperatingSystemInventory::default())
    );
    assert_eq!(snapshot.motherboard, Some(MotherboardInventory::default()));
    assert_eq!(snapshot.product, Some(ProductInventory::default()));
    assert_eq!(snapshot.bios, Some(BiosInventory::default()));
    assert_eq!(snapshot.cpus, vec![CpuInventory::default()]);
    assert_eq!(
        snapshot.memory_modules,
        vec![MemoryModuleInventory::default()]
    );
    assert_eq!(snapshot.gpus, vec![GpuInventory::default()]);
    assert_eq!(snapshot.disks, vec![DiskInventory::default()]);
    assert_eq!(
        snapshot.network_adapters,
        vec![NetworkAdapterInventory::default()]
    );
    assert_eq!(snapshot.firmware, vec![FirmwareInventory::default()]);

    let json = serde_json::to_value(snapshot).unwrap();
    let memory = &json["memory_modules"][0];
    assert_eq!(memory["capacity_bytes"], Value::Null);
    assert_eq!(memory["speed_mhz"], Value::Null);
    assert_eq!(memory["manufacturer"], Value::Null);
    assert_eq!(memory["serial_number"], Value::Null);
    assert!(memory.get("serial_number").is_some());
    assert_eq!(json["product"]["identifying_number"], Value::Null);
    assert_eq!(json["bios"]["version_strings"], Value::Null);
    assert_eq!(json["network_adapters"][0]["physical_adapter"], Value::Null);
}

#[test]
fn empty_cim_classes_do_not_fabricate_inventory_records() {
    let snapshot = WindowsInventory::new(FakeWmi::default())
        .capture()
        .unwrap();

    assert_eq!(snapshot, InventorySnapshot::default());
}

fn cpu(id: &str) -> CpuInventory {
    CpuInventory {
        device_id: Some(id.into()),
        name: Some("Example CPU".into()),
        number_of_cores: Some(8),
        ..CpuInventory::default()
    }
}

fn memory(id: &str) -> MemoryModuleInventory {
    MemoryModuleInventory {
        device_locator: Some(id.into()),
        capacity_bytes: Some(16_000_000_000),
        speed_mhz: Some(6400),
        ..MemoryModuleInventory::default()
    }
}

fn gpu(id: &str) -> GpuInventory {
    GpuInventory {
        pnp_device_id: Some(id.into()),
        name: Some("Example GPU".into()),
        driver_version: Some("1.0.0".into()),
        ..GpuInventory::default()
    }
}

fn disk(id: &str) -> DiskInventory {
    DiskInventory {
        pnp_device_id: Some(id.into()),
        model: Some("Example Disk".into()),
        size_bytes: Some(1_000_000_000_000),
        status: Some("OK".into()),
        ..DiskInventory::default()
    }
}

fn network(id: &str) -> NetworkAdapterInventory {
    NetworkAdapterInventory {
        pnp_device_id: Some(id.into()),
        name: Some("Example Network Adapter".into()),
        mac_address: Some("02:00:00:00:00:01".into()),
        physical_adapter: Some(true),
        ..NetworkAdapterInventory::default()
    }
}

fn firmware(id: &str) -> FirmwareInventory {
    FirmwareInventory {
        device_id: Some(id.into()),
        name: Some("System Firmware".into()),
        driver_version: Some("1.0.0".into()),
        ..FirmwareInventory::default()
    }
}

fn sample_snapshot() -> InventorySnapshot {
    InventorySnapshot {
        operating_system: Some(OperatingSystemInventory {
            caption: Some("Windows".into()),
            build_number: Some("26100".into()),
            ..OperatingSystemInventory::default()
        }),
        motherboard: Some(MotherboardInventory {
            product: Some("21VG".into()),
            version: Some("1.0".into()),
            ..MotherboardInventory::default()
        }),
        product: Some(ProductInventory {
            name: Some("21VG".into()),
            model: Some("Example Model".into()),
            ..ProductInventory::default()
        }),
        bios: Some(BiosInventory {
            smbios_bios_version: Some("BIOS-1".into()),
            ..BiosInventory::default()
        }),
        cpus: vec![cpu("CPU0"), cpu("CPU1")],
        memory_modules: vec![memory("DIMM0"), memory("DIMM1")],
        gpus: vec![gpu("GPU0"), gpu("GPU1")],
        disks: vec![disk("DISK0"), disk("DISK1")],
        network_adapters: vec![network("NET0"), network("NET1")],
        firmware: vec![firmware("FW0"), firmware("FW1")],
    }
}

#[test]
fn collection_order_does_not_create_inventory_changes() {
    let baseline = sample_snapshot();
    let mut current = baseline.clone();
    current.cpus.reverse();
    current.memory_modules.reverse();
    current.gpus.reverse();
    current.disks.reverse();
    current.network_adapters.reverse();
    current.firmware.reverse();

    let diff = baseline.diff(&current);

    assert!(diff.equal);
    assert!(diff.operating_system.equal);
    assert!(diff.motherboard.equal);
    assert!(diff.product.equal);
    assert!(diff.bios.equal);
    assert!(diff.cpus.equal);
    assert!(diff.memory_modules.equal);
    assert!(diff.gpus.equal);
    assert!(diff.disks.equal);
    assert!(diff.network_adapters.equal);
    assert!(diff.firmware.equal);
}

fn assert_only_change<T>(module: &InventoryModuleDiff<T>, field: InventoryField) {
    assert!(!module.equal);
    assert!(module.added.is_empty());
    assert!(module.removed.is_empty());
    assert_eq!(module.changed.len(), 1);
    assert_eq!(module.changed[0].changed_fields, vec![field]);
}

#[test]
fn diff_reports_actual_typed_field_changes_in_every_module() {
    let baseline = sample_snapshot();
    let mut current = baseline.clone();
    current
        .operating_system
        .as_mut()
        .unwrap()
        .build_number = Some("26200".into());
    current.motherboard.as_mut().unwrap().version = Some("2.0".into());
    current.product.as_mut().unwrap().model = Some("New Model".into());
    current.bios.as_mut().unwrap().smbios_bios_version = Some("BIOS-2".into());
    current.cpus[0].number_of_cores = Some(16);
    current.memory_modules[0].manufacturer = Some("Samsung".into());
    current.gpus[0].driver_version = Some("2.0.0".into());
    current.disks[0].status = Some("Pred Fail".into());
    current.network_adapters[0].mac_address = Some("02:00:00:00:00:99".into());
    current.firmware[0].driver_version = Some("2.0.0".into());

    let diff = baseline.diff(&current);

    assert!(!diff.equal);
    assert_only_change(
        &diff.operating_system,
        InventoryField::OperatingSystemBuildNumber,
    );
    assert_only_change(&diff.motherboard, InventoryField::MotherboardVersion);
    assert_only_change(&diff.product, InventoryField::ProductModel);
    assert_only_change(&diff.bios, InventoryField::BiosSmbiosBiosVersion);
    assert_only_change(&diff.cpus, InventoryField::CpuNumberOfCores);
    assert_only_change(
        &diff.memory_modules,
        InventoryField::MemoryManufacturer,
    );
    assert_only_change(&diff.gpus, InventoryField::GpuDriverVersion);
    assert_only_change(&diff.disks, InventoryField::DiskStatus);
    assert_only_change(
        &diff.network_adapters,
        InventoryField::NetworkAdapterMacAddress,
    );
    assert_only_change(&diff.firmware, InventoryField::FirmwareDriverVersion);

    let json = serde_json::to_value(diff).unwrap();
    let change = &json["memory_modules"]["changed"][0];
    assert_eq!(change["changed_fields"][0], "memory_manufacturer");
    assert_eq!(change["baseline"]["manufacturer"], Value::Null);
    assert_eq!(change["current"]["manufacturer"], "Samsung");
}

#[test]
fn diff_distinguishes_added_removed_and_equal_modules() {
    let baseline = InventorySnapshot {
        memory_modules: vec![memory("DIMM-A"), memory("DIMM-B")],
        ..InventorySnapshot::default()
    };
    let current = InventorySnapshot {
        memory_modules: vec![memory("DIMM-C"), memory("DIMM-B")],
        ..InventorySnapshot::default()
    };

    let diff = baseline.diff(&current);
    assert!(!diff.memory_modules.equal);
    assert!(diff.memory_modules.changed.is_empty());
    assert_eq!(diff.memory_modules.added.len(), 1);
    assert_eq!(
        diff.memory_modules.added[0].device_locator.as_deref(),
        Some("DIMM-C")
    );
    assert_eq!(diff.memory_modules.removed.len(), 1);
    assert_eq!(
        diff.memory_modules.removed[0].device_locator.as_deref(),
        Some("DIMM-A")
    );

    let without_os = InventorySnapshot::default();
    let with_os = InventorySnapshot {
        operating_system: sample_snapshot().operating_system,
        ..InventorySnapshot::default()
    };
    let added = without_os.diff(&with_os);
    assert!(!added.operating_system.equal);
    assert_eq!(added.operating_system.added.len(), 1);
    assert!(added.operating_system.removed.is_empty());
    assert!(added.operating_system.changed.is_empty());

    let removed = with_os.diff(&without_os);
    assert!(!removed.operating_system.equal);
    assert!(removed.operating_system.added.is_empty());
    assert_eq!(removed.operating_system.removed.len(), 1);
    assert!(removed.operating_system.changed.is_empty());

    let equal: InventoryDiff = without_os.diff(&without_os);
    assert!(equal.is_equal());
}

#[test]
fn exact_matches_are_paired_before_duplicate_fallback_identities() {
    let mut first = memory("DIMM0");
    first.capacity_bytes = Some(8_000_000_000);
    let mut second = memory("DIMM0");
    second.capacity_bytes = Some(16_000_000_000);
    let baseline = InventorySnapshot {
        memory_modules: vec![first.clone(), second.clone()],
        ..InventorySnapshot::default()
    };
    let current = InventorySnapshot {
        memory_modules: vec![second],
        ..InventorySnapshot::default()
    };

    let diff = baseline.diff(&current);

    assert!(diff.memory_modules.changed.is_empty());
    assert!(diff.memory_modules.added.is_empty());
    assert_eq!(diff.memory_modules.removed, vec![first]);
}
