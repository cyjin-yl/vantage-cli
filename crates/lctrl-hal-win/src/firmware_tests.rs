use std::collections::BTreeMap;
use std::sync::Mutex;

use lctrl_core::{FirmwareResource, LctrlError, Result};
use lctrl_hal::FirmwareControl;

use super::{
    BIOS_QUERY, FIRMWARE_DEVICE_QUERY, ROOT_CIMV2, WindowsFirmware, apply_stored_versions,
};
use crate::{WmiObject, WmiTransport, WmiValue};

#[derive(Default)]
struct FakeWmi {
    bios: Vec<WmiObject>,
    devices: Vec<WmiObject>,
    namespaces: Mutex<Vec<String>>,
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        self.namespaces
            .lock()
            .expect("namespace log lock")
            .push(format!("{namespace}|{wql}"));
        match (namespace, wql) {
            (ROOT_CIMV2, BIOS_QUERY) => Ok(self.bios.clone()),
            (ROOT_CIMV2, FIRMWARE_DEVICE_QUERY) => Ok(self.devices.clone()),
            _ => Err(LctrlError::Unsupported {
                feature: format!("unexpected query {namespace} {wql}"),
            }),
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        Err(LctrlError::Unsupported {
            feature: "firmware inventory never invokes a method".into(),
        })
    }
}

fn object(pairs: &[(&str, WmiValue)]) -> WmiObject {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

fn strings(values: &[&str]) -> WmiValue {
    WmiValue::Array(
        values
            .iter()
            .map(|value| WmiValue::String((*value).into()))
            .collect(),
    )
}

fn bios_row() -> WmiObject {
    object(&[
        ("Manufacturer", WmiValue::String("LENOVO".into())),
        ("SMBIOSBIOSVersion", WmiValue::String("SWCN22WW".into())),
        ("ReleaseDate", WmiValue::String("2026/7/15 8:00:00".into())),
        ("SMBIOSMajorVersion", WmiValue::U16(3)),
        ("SMBIOSMinorVersion", WmiValue::U16(8)),
        ("SystemBiosMajorVersion", WmiValue::U16(1)),
        ("SystemBiosMinorVersion", WmiValue::U16(22)),
        ("EmbeddedControllerMajorVersion", WmiValue::U16(1)),
        ("EmbeddedControllerMinorVersion", WmiValue::U16(22)),
        (
            "BIOSVersion",
            strings(&["LENOVO - 1", "SWCN22WW", "INSYDE Corp. - 71250022"]),
        ),
    ])
}

fn system_firmware_device() -> WmiObject {
    object(&[
        (
            "DeviceID",
            WmiValue::String("UEFI\\RES_{8163650E-C940-3926-0703-FCF1F743D4E2}\\0".into()),
        ),
        ("Name", WmiValue::String("System Firmware".into())),
        ("Status", WmiValue::String("OK".into())),
        (
            "HardwareID",
            strings(&[
                "UEFI\\RES_{8163650e-c940-3926-0703-fcf1f743d4e2}&REV_71250022",
                "UEFI\\RES_{8163650e-c940-3926-0703-fcf1f743d4e2}",
            ]),
        ),
        (
            "CompatibleID",
            strings(&["UEFI\\CC_00010001", "GenFirmwareResource"]),
        ),
    ])
}

fn revisionless_device() -> WmiObject {
    object(&[
        (
            "DeviceID",
            WmiValue::String("UEFI\\RES_{64EC2CA6-4AE3-4A6E-B542-27A7E0AA0BA6}\\0".into()),
        ),
        ("Name", WmiValue::String("Device Firmware".into())),
        (
            "HardwareID",
            strings(&["UEFI\\RES_{64ec2ca6-4ae3-4a6e-b542-27a7e0aa0ba6}"]),
        ),
    ])
}

fn fake(bios: Vec<WmiObject>, devices: Vec<WmiObject>) -> FakeWmi {
    FakeWmi {
        bios,
        devices,
        namespaces: Mutex::new(Vec::new()),
    }
}

#[test]
fn identity_maps_the_documented_bios_fields() {
    let wmi = fake(vec![bios_row()], Vec::new());
    let status = WindowsFirmware::new(&wmi)
        .firmware_status()
        .expect("firmware status");
    let identity = &status.identity;
    assert_eq!(identity.vendor.as_deref(), Some("LENOVO"));
    assert_eq!(identity.version.as_deref(), Some("SWCN22WW"));
    assert_eq!(identity.release_date.as_deref(), Some("2026/7/15 8:00:00"));
    assert_eq!(identity.bios_release().as_deref(), Some("1.22"));
    assert_eq!(identity.ec_firmware().as_deref(), Some("1.22"));
    assert_eq!(identity.smbios_version().as_deref(), Some("3.8"));
    assert_eq!(
        identity.version_strings,
        ["LENOVO - 1", "SWCN22WW", "INSYDE Corp. - 71250022"]
    );
    let logged = wmi.namespaces.lock().expect("namespace log").clone();
    assert_eq!(
        logged,
        vec![
            format!("{ROOT_CIMV2}|{BIOS_QUERY}"),
            format!("{ROOT_CIMV2}|{FIRMWARE_DEVICE_QUERY}")
        ]
    );
}

#[test]
fn resources_take_their_version_from_the_revision_hardware_id() {
    let wmi = fake(
        vec![bios_row()],
        vec![system_firmware_device(), revisionless_device()],
    );
    let status = WindowsFirmware::new(&wmi)
        .firmware_status()
        .expect("firmware status");
    assert_eq!(status.resources.len(), 2);
    let firmware = &status.resources[0];
    assert_eq!(
        firmware.firmware_class,
        "{8163650E-C940-3926-0703-FCF1F743D4E2}"
    );
    assert_eq!(firmware.current_version, Some(0x7125_0022));
    assert_eq!(firmware.description.as_deref(), Some("System Firmware"));
    assert_eq!(firmware.status.as_deref(), Some("OK"));
    assert!(firmware.issues.is_empty());
    // A device whose hardware ids carry no revision keeps the field at `None`
    // instead of dropping it from the payload.
    let revisionless = &status.resources[1];
    assert_eq!(revisionless.current_version, None);
    let json = serde_json::to_value(revisionless).expect("serializable resource");
    assert!(json.get("current_version").is_some_and(serde_json::Value::is_null));
    assert!(json.get("stored_version").is_some_and(serde_json::Value::is_null));
}

#[test]
fn a_stored_version_disagreement_is_reported_not_resolved() {
    let mut resources = vec![FirmwareResource {
        instance_id: "UEFI\\RES_{8163650E-C940-3926-0703-FCF1F743D4E2}\\0".into(),
        firmware_class: "{8163650E-C940-3926-0703-FCF1F743D4E2}".into(),
        current_version: Some(0x7125_0022),
        stored_version: None,
        description: None,
        status: None,
        hardware_ids: Vec::new(),
        compatible_ids: Vec::new(),
        issues: Vec::new(),
    }];
    let mut stored = BTreeMap::new();
    stored.insert(
        "{8163650E-C940-3926-0703-FCF1F743D4E2}".to_string(),
        0x7125_0021,
    );
    assert_eq!(apply_stored_versions(&mut resources, &stored), 1);
    assert_eq!(resources[0].current_version, Some(0x7125_0022));
    assert_eq!(resources[0].stored_version, Some(0x7125_0021));
    assert_eq!(resources[0].issues.len(), 1);
    assert!(resources[0].issues[0].contains("0x71250022"));
    assert!(resources[0].issues[0].contains("0x71250021"));
}

#[test]
fn an_agreeing_store_leaves_no_issue() {
    let mut resources = vec![FirmwareResource {
        instance_id: "UEFI\\RES_{499371BF-8950-4B3C-81B6-2C97525241D7}\\0".into(),
        firmware_class: "{499371BF-8950-4B3C-81B6-2C97525241D7}".into(),
        current_version: Some(0x0D03_3710),
        stored_version: None,
        description: None,
        status: None,
        hardware_ids: Vec::new(),
        compatible_ids: Vec::new(),
        issues: Vec::new(),
    }];
    let mut stored = BTreeMap::new();
    stored.insert(
        "{499371BF-8950-4B3C-81B6-2C97525241D7}".to_string(),
        0x0D03_3710,
    );
    assert_eq!(apply_stored_versions(&mut resources, &stored), 1);
    assert_eq!(resources[0].stored_version, Some(0x0D03_3710));
    assert!(resources[0].issues.is_empty());
}

#[test]
fn a_missing_bios_instance_is_an_error_not_an_empty_status() {
    let wmi = fake(Vec::new(), vec![system_firmware_device()]);
    let error = WindowsFirmware::new(&wmi)
        .firmware_status()
        .expect_err("no Win32_BIOS instance must fail the read");
    assert!(
        error.to_string().contains("Win32_BIOS"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_query_failure_propagates() {
    struct Failing;
    impl WmiTransport for Failing {
        fn query(&self, _namespace: &str, _wql: &str) -> Result<Vec<WmiObject>> {
            Err(LctrlError::ChannelUnavailable {
                channel: "WMI stopped responding".into(),
            })
        }
        fn invoke_instance(
            &self,
            _namespace: &str,
            _class: &str,
            _object_path: &str,
            _method: &str,
            _input: &WmiObject,
        ) -> Result<WmiObject> {
            unreachable!("no method invocation in this channel")
        }
    }
    let error = WindowsFirmware::new(Failing)
        .firmware_status()
        .expect_err("channel failure must not become a status");
    assert!(error.to_string().contains("WMI stopped responding"));
}
