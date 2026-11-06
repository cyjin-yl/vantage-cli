//! Unit tests for the read-only security observation.
//!
//! These run without Windows: the WMI transport and the firmware reads are
//! faked, so the contract that matters here (per-component state, source,
//! unavailable-with-reason, MOF-derived value maps, getter failure handling) is
//! exercised on any host. Live acceptance belongs to the target receipts in
//! `docs/evidence/re-security-20261105.md`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use lctrl_core::{FirmwareType, LctrlError, Result};
use lctrl_hal::{
    BitLockerConversionStatus, BitLockerEncryptionMethod, BitLockerLockStatus,
    BitLockerProtectionStatus, BitLockerState, BitLockerVolumeType,
    MAX_BITLOCKER_VOLUMES, SecureBootState, SecurityAvailability, SecurityControl, SecuritySource,
    TpmState,
};

use super::{
    BITLOCKER_NAMESPACE, FirmwareEnvironment, FirmwareVariableState, PLATFORM_KEY_VARIABLE,
    SECURE_BOOT_VARIABLE, SETUP_MODE_VARIABLE, TPM_NAMESPACE, WindowsSecurity, observe_security,
};
use crate::{WmiTransport, WmiValue};

#[derive(Default)]
struct FakeWmi {
    tpm: Vec<BTreeMap<String, WmiValue>>,
    tpm_failure: Option<String>,
    volumes: Vec<BTreeMap<String, WmiValue>>,
    volume_failure: Option<String>,
    method_outputs: BTreeMap<String, BTreeMap<String, WmiValue>>,
    method_failures: BTreeMap<String, String>,
    calls: Mutex<Vec<(String, String)>>,
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, _wql: &str) -> Result<Vec<BTreeMap<String, WmiValue>>> {
        match namespace {
            TPM_NAMESPACE => match &self.tpm_failure {
                Some(channel) => Err(LctrlError::ChannelUnavailable {
                    channel: channel.clone(),
                }),
                None => Ok(self.tpm.clone()),
            },
            BITLOCKER_NAMESPACE => match &self.volume_failure {
                Some(channel) => Err(LctrlError::ChannelUnavailable {
                    channel: channel.clone(),
                }),
                None => Ok(self.volumes.clone()),
            },
            _ => Ok(Vec::new()),
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        object_path: &str,
        method: &str,
        _input: &BTreeMap<String, WmiValue>,
    ) -> Result<BTreeMap<String, WmiValue>> {
        self.calls
            .lock()
            .expect("calls lock")
            .push((method.to_string(), object_path.to_string()));
        if let Some(channel) = self.method_failures.get(method) {
            return Err(LctrlError::ChannelUnavailable {
                channel: channel.clone(),
            });
        }
        Ok(self.method_outputs.get(method).cloned().unwrap_or_default())
    }
}

#[derive(Default)]
struct FakeFirmware {
    firmware_type: Option<FirmwareType>,
    firmware_type_failure: Option<String>,
    variables: BTreeMap<String, FirmwareVariableState>,
    variable_failures: BTreeMap<String, String>,
    registry: Option<u32>,
    registry_failure: Option<String>,
    reads: Mutex<Vec<String>>,
}

impl FirmwareEnvironment for FakeFirmware {
    fn firmware_type(&self) -> Result<FirmwareType> {
        match &self.firmware_type_failure {
            Some(channel) => Err(LctrlError::ChannelUnavailable {
                channel: channel.clone(),
            }),
            None => Ok(self.firmware_type.unwrap_or(FirmwareType::Uefi)),
        }
    }

    fn read_variable(&self, name: &str, _guid: &str) -> Result<FirmwareVariableState> {
        self.reads
            .lock()
            .expect("reads lock")
            .push(name.to_string());
        if let Some(channel) = self.variable_failures.get(name) {
            return Err(LctrlError::ChannelUnavailable {
                channel: channel.clone(),
            });
        }
        Ok(self
            .variables
            .get(name)
            .copied()
            .unwrap_or(FirmwareVariableState::Absent))
    }

    fn registry_dword(&self, _key: &str, _value: &str) -> Result<Option<u32>> {
        match &self.registry_failure {
            Some(channel) => Err(LctrlError::ChannelUnavailable {
                channel: channel.clone(),
            }),
            None => Ok(self.registry),
        }
    }
}

fn byte_variable(value: u8) -> FirmwareVariableState {
    FirmwareVariableState::Present {
        length: Some(1),
        first_byte: Some(value),
    }
}

fn secure_firmware() -> FakeFirmware {
    FakeFirmware {
        firmware_type: Some(FirmwareType::Uefi),
        variables: BTreeMap::from([
            (SECURE_BOOT_VARIABLE.to_string(), byte_variable(1)),
            (SETUP_MODE_VARIABLE.to_string(), byte_variable(0)),
            (
                PLATFORM_KEY_VARIABLE.to_string(),
                FirmwareVariableState::Present {
                    length: Some(852),
                    first_byte: Some(0xa1),
                },
            ),
        ]),
        registry: Some(1),
        ..FakeFirmware::default()
    }
}

fn volume_object() -> BTreeMap<String, WmiValue> {
    BTreeMap::from([
        (
            "DeviceID".to_string(),
            WmiValue::String(r"\\?\Volume{abc}\".to_string()),
        ),
        ("DriveLetter".to_string(), WmiValue::String("C:".to_string())),
        ("ProtectionStatus".to_string(), WmiValue::U32(0)),
        ("ConversionStatus".to_string(), WmiValue::U32(0)),
        ("EncryptionMethod".to_string(), WmiValue::U32(6)),
        ("VolumeType".to_string(), WmiValue::U32(0)),
        (
            "IsVolumeInitializedForProtection".to_string(),
            WmiValue::Bool(true),
        ),
    ])
}

fn unlocked_output() -> BTreeMap<String, WmiValue> {
    BTreeMap::from([("LockStatus".to_string(), WmiValue::U32(0))])
}

fn conversion_output(percentage: u32) -> BTreeMap<String, WmiValue> {
    BTreeMap::from([
        ("ConversionStatus".to_string(), WmiValue::U32(1)),
        ("EncryptionPercentage".to_string(), WmiValue::U32(percentage)),
    ])
}

fn volume_wmi() -> FakeWmi {
    FakeWmi {
        volumes: vec![volume_object()],
        method_outputs: BTreeMap::from([
            ("GetLockStatus".to_string(), unlocked_output()),
            ("GetConversionStatus".to_string(), conversion_output(100)),
        ]),
        ..FakeWmi::default()
    }
}

#[test]
fn secure_boot_enabled_reports_every_corroborating_read() {
    let report = observe_security(&FakeWmi::default(), &secure_firmware());

    assert_eq!(report.secure_boot.state, SecureBootState::Enabled);
    assert_eq!(report.secure_boot.firmware_type, FirmwareType::Uefi);
    assert_eq!(
        report.secure_boot.source,
        SecuritySource::UefiFirmwareVariable
    );
    assert_eq!(report.secure_boot.secure_boot_variable, Some(true));
    assert_eq!(report.secure_boot.setup_mode, Some(false));
    assert_eq!(report.secure_boot.platform_key_present, Some(true));
    assert_eq!(report.secure_boot.platform_key_bytes, Some(852));
    assert_eq!(report.secure_boot.registry_secure_boot_enabled, Some(true));
    assert_eq!(report.secure_boot.detail, None);
}

#[test]
fn secure_boot_disabled_and_not_uefi_are_distinct_states() {
    let mut firmware = secure_firmware();
    firmware.variables.insert(
        SECURE_BOOT_VARIABLE.to_string(),
        byte_variable(0),
    );
    let report = observe_security(&FakeWmi::default(), &firmware);
    assert_eq!(report.secure_boot.state, SecureBootState::Disabled);
    assert_eq!(report.secure_boot.secure_boot_variable, Some(false));

    let legacy = FakeFirmware {
        firmware_type: Some(FirmwareType::Bios),
        ..FakeFirmware::default()
    };
    let report = observe_security(&FakeWmi::default(), &legacy);
    assert_eq!(report.secure_boot.state, SecureBootState::NotUefi);
    assert_eq!(report.secure_boot.firmware_type, FirmwareType::Bios);
    assert_eq!(report.secure_boot.source, SecuritySource::FirmwareType);
    assert!(
        legacy.reads.lock().expect("reads lock").is_empty(),
        "legacy BIOS must not attempt a UEFI variable read"
    );
}

#[test]
fn unreadable_secure_boot_variable_is_unavailable_with_the_reason() {
    let firmware = FakeFirmware {
        variable_failures: BTreeMap::from([(
            SECURE_BOOT_VARIABLE.to_string(),
            "access denied (SeSystemEnvironmentPrivilege)".to_string(),
        )]),
        ..secure_firmware()
    };
    let report = observe_security(&FakeWmi::default(), &firmware);

    assert_eq!(report.secure_boot.state, SecureBootState::Unavailable);
    assert_eq!(
        report.secure_boot.secure_boot_variable, None,
        "a failed read must never be reported as disabled"
    );
    assert!(
        report
            .secure_boot
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("access denied")),
        "expected the read failure in the detail, got {:?}",
        report.secure_boot.detail
    );
}

#[test]
fn malformed_secure_boot_variable_is_unavailable_not_interpreted() {
    let mut firmware = secure_firmware();
    firmware.variables.insert(
        SECURE_BOOT_VARIABLE.to_string(),
        FirmwareVariableState::Present {
            length: Some(2),
            first_byte: Some(1),
        },
    );
    let report = observe_security(&FakeWmi::default(), &firmware);
    assert_eq!(report.secure_boot.state, SecureBootState::Unavailable);
    assert_eq!(report.secure_boot.secure_boot_variable, None);
}

#[test]
fn secure_boot_variable_zero_is_disabled_but_a_failed_firmware_type_is_not() {
    let unknown = FakeFirmware {
        firmware_type_failure: Some("GetFirmwareType failed".to_string()),
        ..FakeFirmware::default()
    };
    let report = observe_security(&FakeWmi::default(), &unknown);
    assert_eq!(report.secure_boot.state, SecureBootState::Unavailable);
    assert_eq!(report.secure_boot.firmware_type, FirmwareType::Unknown);
    assert!(
        unknown.reads.lock().expect("reads lock").is_empty(),
        "an unknown boot mode must not read firmware variables"
    );
}

#[test]
fn tpm_present_decodes_properties_and_absent_is_not_unavailable() {
    let wmi = FakeWmi {
        tpm: vec![BTreeMap::from([
            ("IsEnabled_InitialValue".to_string(), WmiValue::Bool(true)),
            ("IsActivated_InitialValue".to_string(), WmiValue::Bool(true)),
            ("IsOwned_InitialValue".to_string(), WmiValue::Bool(true)),
            ("ManufacturerId".to_string(), WmiValue::U32(0x494E_5443)),
            (
                "ManufacturerIdTxt".to_string(),
                WmiValue::String("INTC".to_string()),
            ),
            (
                "SpecVersion".to_string(),
                WmiValue::String("2.0, 0, 1.59".to_string()),
            ),
            (
                "ManufacturerVersion".to_string(),
                WmiValue::String("700.21.2.1380".to_string()),
            ),
        ])],
        ..FakeWmi::default()
    };
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.tpm.state, TpmState::Present);
    assert_eq!(report.tpm.source, SecuritySource::Win32Tpm);
    assert_eq!(report.tpm.enabled, Some(true));
    assert_eq!(report.tpm.activated, Some(true));
    assert_eq!(report.tpm.owned, Some(true));
    assert_eq!(report.tpm.manufacturer_id_text.as_deref(), Some("INTC"));
    assert_eq!(report.tpm.spec_version.as_deref(), Some("2.0, 0, 1.59"));

    let empty = observe_security(&FakeWmi::default(), &secure_firmware());
    assert_eq!(empty.tpm.state, TpmState::Absent);
    assert_eq!(empty.tpm.enabled, None);
}

#[test]
fn tpm_query_failure_is_unavailable_never_absent() {
    let wmi = FakeWmi {
        tpm_failure: Some("WBEM_E_INVALID_NAMESPACE".to_string()),
        ..FakeWmi::default()
    };
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.tpm.state, TpmState::Unavailable);
    assert!(
        report
            .tpm
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("WBEM_E_INVALID_NAMESPACE"))
    );
}

#[test]
fn bitlocker_volume_decodes_mof_maps_and_getter_values() {
    let report = observe_security(&volume_wmi(), &secure_firmware());
    assert_eq!(report.bitlocker.state, BitLockerState::Observed);
    assert_eq!(report.bitlocker.volumes.len(), 1);
    let volume = &report.bitlocker.volumes[0];
    assert_eq!(volume.drive_letter.as_deref(), Some("C:"));
    assert_eq!(volume.protection_status, Some(BitLockerProtectionStatus::Unprotected));
    assert_eq!(volume.protection_status_raw, Some(0));
    assert_eq!(
        volume.conversion_status,
        Some(BitLockerConversionStatus::FullyDecrypted)
    );
    assert_eq!(
        volume.encryption_method,
        Some(BitLockerEncryptionMethod::XtsAes128)
    );
    assert_eq!(volume.encryption_method_raw, Some(6));
    assert_eq!(volume.volume_type, Some(BitLockerVolumeType::OsVolume));
    assert_eq!(volume.initialized_for_protection, Some(true));
    assert_eq!(volume.lock_status, Some(BitLockerLockStatus::Unlocked));
    assert_eq!(volume.encryption_percentage, Some(100));
    assert_eq!(volume.detail, None);
}

#[test]
fn bitlocker_getter_calls_use_the_declared_instance_path_and_precision_factor() {
    let wmi = volume_wmi();
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.bitlocker.volumes.len(), 1);

    let calls = wmi.calls.lock().expect("calls lock").clone();
    assert_eq!(calls.len(), 2);
    let path = r#"Win32_EncryptableVolume.DeviceID="\\\\?\\Volume{abc}\\""#;
    assert_eq!(calls[0], ("GetLockStatus".to_string(), path.to_string()));
    assert_eq!(
        calls[1],
        ("GetConversionStatus".to_string(), path.to_string())
    );
}

#[test]
fn bitlocker_getter_failure_leaves_the_fields_unknown() {
    let wmi = FakeWmi {
        method_failures: BTreeMap::from([(
            "GetLockStatus".to_string(),
            "WBEM_E_NOT_SUPPORTED".to_string(),
        )]),
        ..volume_wmi()
    };
    let report = observe_security(&wmi, &secure_firmware());
    let volume = &report.bitlocker.volumes[0];
    assert_eq!(
        volume.lock_status, None,
        "an unanswered getter must not become Unlocked"
    );
    assert_eq!(volume.lock_status_raw, None);
    assert_eq!(volume.encryption_percentage, Some(100));
    assert!(
        volume
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("GetLockStatus"))
    );
}

#[test]
fn bitlocker_query_failure_is_unavailable_with_no_volumes() {
    let wmi = FakeWmi {
        volume_failure: Some("WBEM_E_ACCESS_DENIED".to_string()),
        ..FakeWmi::default()
    };
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.bitlocker.state, BitLockerState::Unavailable);
    assert!(report.bitlocker.volumes.is_empty());
    assert!(
        report
            .bitlocker
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("WBEM_E_ACCESS_DENIED"))
    );
}

#[test]
fn bitlocker_read_is_bounded_and_reports_truncation() {
    let wmi = FakeWmi {
        volumes: (0..MAX_BITLOCKER_VOLUMES + 3).map(|_| volume_object()).collect(),
        method_outputs: BTreeMap::from([
            ("GetLockStatus".to_string(), unlocked_output()),
            ("GetConversionStatus".to_string(), conversion_output(0)),
        ]),
        ..FakeWmi::default()
    };
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.bitlocker.volumes.len(), MAX_BITLOCKER_VOLUMES);
    assert!(
        report
            .bitlocker
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("only the first")),
        "truncation must be reported, got {:?}",
        report.bitlocker.detail
    );
    assert_eq!(
        wmi.calls.lock().expect("calls lock").len(),
        MAX_BITLOCKER_VOLUMES * 2
    );
}

#[test]
fn each_component_keeps_its_own_state_and_the_availability_aggregates() {
    let wmi = FakeWmi {
        volume_failure: Some("namespace missing".to_string()),
        ..volume_wmi()
    };
    let report = observe_security(&wmi, &secure_firmware());
    assert_eq!(report.secure_boot.state, SecureBootState::Enabled);
    assert_eq!(report.tpm.state, TpmState::Absent);
    assert_eq!(report.bitlocker.state, BitLockerState::Unavailable);
    assert_eq!(report.availability(), SecurityAvailability::Limited);
    assert!(
        report
            .details()
            .iter()
            .any(|detail| detail.starts_with("bitlocker: "))
    );

    let all_failed = FakeWmi {
        tpm_failure: Some("denied".to_string()),
        volume_failure: Some("denied".to_string()),
        ..FakeWmi::default()
    };
    let firmware = FakeFirmware {
        firmware_type_failure: Some("denied".to_string()),
        ..FakeFirmware::default()
    };
    let report = observe_security(&all_failed, &firmware);
    assert_eq!(report.availability(), SecurityAvailability::Unavailable);

    let all_ok = observe_security(&volume_wmi(), &secure_firmware());
    assert_eq!(all_ok.availability(), SecurityAvailability::Available);
}

#[test]
fn security_control_reports_the_same_observation() {
    let security = WindowsSecurity::new(volume_wmi(), secure_firmware());
    let report = security.security_status().expect("read-only observation");
    assert_eq!(report.secure_boot.state, SecureBootState::Enabled);
    assert_eq!(report.bitlocker.volumes.len(), 1);
}

#[test]
fn value_map_boundaries_are_never_extrapolated() {
    for (raw, expected) in [
        (0, Some(BitLockerProtectionStatus::Unprotected)),
        (1, Some(BitLockerProtectionStatus::Protected)),
        (2, Some(BitLockerProtectionStatus::Unknown)),
        (3, None),
        (u32::MAX, None),
    ] {
        assert_eq!(BitLockerProtectionStatus::from_raw(raw), expected);
    }
    for (raw, expected) in [
        (0, Some(BitLockerConversionStatus::FullyDecrypted)),
        (1, Some(BitLockerConversionStatus::FullyEncrypted)),
        (2, Some(BitLockerConversionStatus::EncryptionInProgress)),
        (3, Some(BitLockerConversionStatus::DecryptionInProgress)),
        (4, Some(BitLockerConversionStatus::EncryptionPaused)),
        (5, Some(BitLockerConversionStatus::DecryptionPaused)),
        (6, None),
    ] {
        assert_eq!(BitLockerConversionStatus::from_raw(raw), expected);
    }
    for (raw, expected) in [
        (0, Some(BitLockerEncryptionMethod::None)),
        (1, Some(BitLockerEncryptionMethod::Aes128WithDiffuser)),
        (2, Some(BitLockerEncryptionMethod::Aes256WithDiffuser)),
        (3, Some(BitLockerEncryptionMethod::Aes128)),
        (4, Some(BitLockerEncryptionMethod::Aes256)),
        (5, Some(BitLockerEncryptionMethod::HardwareEncryption)),
        (6, Some(BitLockerEncryptionMethod::XtsAes128)),
        (7, Some(BitLockerEncryptionMethod::XtsAes256)),
        (8, None),
    ] {
        assert_eq!(BitLockerEncryptionMethod::from_raw(raw), expected);
    }
    for (raw, expected) in [
        (0, Some(BitLockerVolumeType::OsVolume)),
        (1, Some(BitLockerVolumeType::FixedDataVolume)),
        (2, Some(BitLockerVolumeType::PortableDataVolume)),
        (3, None),
    ] {
        assert_eq!(BitLockerVolumeType::from_raw(raw), expected);
    }
    for (raw, expected) in [
        (0, Some(BitLockerLockStatus::Unlocked)),
        (1, Some(BitLockerLockStatus::Locked)),
        (2, None),
    ] {
        assert_eq!(BitLockerLockStatus::from_raw(raw), expected);
    }
}
