//! Read-only Secure Boot / TPM / BitLocker observation on Windows.
//!
//! Three sources, no vendor component, no mutation:
//!
//! * UEFI firmware variables through `GetFirmwareEnvironmentVariableExW`
//!   (`{8be4df61-93ca-11d2-aa0d-00e098032b8c}:SecureBoot`, `:SetupMode`, `:PK`)
//!   plus `GetFirmwareType` for the boot mode. Nothing is written; the only
//!   privileged step is enabling `SeSystemEnvironmentPrivilege` on this process
//!   token so the read is permitted.
//! * `root\CIMV2\Security\MicrosoftTpm: Win32_Tpm`, property-only (no provider
//!   method, so no ownership/clear/attestation path is reachable).
//! * `root\CIMV2\Security\MicrosoftVolumeEncryption: Win32_EncryptableVolume`,
//!   properties plus exactly two read-only getters (`GetLockStatus`,
//!   `GetConversionStatus`) because lock state and conversion percentage are not
//!   class properties. No key protector, recovery password, or state change is
//!   touched.
//!
//! A component that cannot be read is reported as unavailable with the reason;
//! nothing is defaulted to "off" or "unlocked". Recovered contract, live
//! receipts, and the MOF-derived value maps: `docs/evidence/re-security-20261105.md`.

use lctrl_core::{FirmwareType, Result};
use lctrl_hal::{
    BitLockerConversionStatus, BitLockerEncryptionMethod, BitLockerLockStatus,
    BitLockerProtectionStatus, BitLockerReport, BitLockerState, BitLockerVolume,
    BitLockerVolumeType, MAX_BITLOCKER_VOLUMES, SecureBootReport, SecureBootState,
    SecurityControl, SecuritySource, SecurityStatusReport, TpmReport, TpmState,
};

use crate::{WmiObject, WmiTransport, WmiValue};

/// UEFI global variable namespace (`EFI_GLOBAL_VARIABLE`).
pub const SECURE_BOOT_GUID: &str = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}";
/// `SecureBoot`: one byte, `0` disabled, `1` enabled.
pub const SECURE_BOOT_VARIABLE: &str = "SecureBoot";
/// `SetupMode`: one byte, `1` while the platform has no enrolled platform key.
pub const SETUP_MODE_VARIABLE: &str = "SetupMode";
/// `PK`: the platform key. Only presence and length are reported.
pub const PLATFORM_KEY_VARIABLE: &str = "PK";
/// Key that mirrors the firmware Secure Boot state in the operating system.
pub const SECURE_BOOT_STATE_KEY: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot\State";
/// `UEFISecureBootEnabled`, `REG_DWORD`.
pub const SECURE_BOOT_STATE_VALUE: &str = "UEFISecureBootEnabled";

pub const TPM_NAMESPACE: &str = "ROOT\\CIMV2\\Security\\MicrosoftTpm";
pub const TPM_CLASS: &str = "Win32_Tpm";
pub const TPM_QUERY: &str = "SELECT IsActivated_InitialValue, IsEnabled_InitialValue, IsOwned_InitialValue, ManufacturerId, ManufacturerIdTxt, ManufacturerVersion, ManufacturerVersionFull20, ManufacturerVersionInfo, PhysicalPresenceVersionInfo, SpecVersion FROM Win32_Tpm";

pub const BITLOCKER_NAMESPACE: &str = "ROOT\\CIMV2\\Security\\MicrosoftVolumeEncryption";
pub const BITLOCKER_CLASS: &str = "Win32_EncryptableVolume";
pub const BITLOCKER_QUERY: &str = "SELECT __Path, DeviceID, DriveLetter, PersistentVolumeID, ProtectionStatus, ConversionStatus, EncryptionMethod, VolumeType, IsVolumeInitializedForProtection FROM Win32_EncryptableVolume";

/// Upper bound on the buffer used for one firmware variable. `PK` on the target
/// is 852 bytes; the image-signature databases live in another GUID and are
/// never read here, so a small bound is enough.
pub const FIRMWARE_VARIABLE_BUFFER_BYTES: usize = 16 * 1024;

/// Outcome of one UEFI variable read. The bytes themselves are never returned:
/// only the length and the first byte survive the call, so key material cannot
/// leave the reader.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareVariableState {
    /// The variable exists. `length` is `None` when the variable is larger than
    /// the read buffer, so its size is present but not its value.
    Present {
        length: Option<usize>,
        first_byte: Option<u8>,
    },
    /// `ERROR_ENVVAR_NOT_FOUND`: the platform does not expose this variable.
    Absent,
}

/// The three reads the Secure Boot component needs, abstracted so the analysis
/// is testable without firmware.
pub trait FirmwareEnvironment: Send + Sync {
    fn firmware_type(&self) -> Result<FirmwareType>;
    fn read_variable(&self, name: &str, guid: &str) -> Result<FirmwareVariableState>;
    /// One `REG_DWORD` under `HKEY_LOCAL_MACHINE`; `Ok(None)` when absent.
    fn registry_dword(&self, key: &str, value: &str) -> Result<Option<u32>>;
}

impl<T: FirmwareEnvironment + ?Sized> FirmwareEnvironment for &T {
    fn firmware_type(&self) -> Result<FirmwareType> {
        (**self).firmware_type()
    }

    fn read_variable(&self, name: &str, guid: &str) -> Result<FirmwareVariableState> {
        (**self).read_variable(name, guid)
    }

    fn registry_dword(&self, key: &str, value: &str) -> Result<Option<u32>> {
        (**self).registry_dword(key, value)
    }
}

/// Read-only security posture over the public WMI transport plus firmware reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsSecurity<W, F> {
    wmi: W,
    firmware: F,
}

impl<W, F> WindowsSecurity<W, F> {
    #[must_use]
    pub const fn new(wmi: W, firmware: F) -> Self {
        Self { wmi, firmware }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn firmware(&self) -> &F {
        &self.firmware
    }
}

impl<W, F> SecurityControl for WindowsSecurity<W, F>
where
    W: WmiTransport,
    F: FirmwareEnvironment,
{
    fn security_status(&self) -> Result<SecurityStatusReport> {
        Ok(observe_security(&self.wmi, &self.firmware))
    }
}

/// Observe all three components. This never fails as a whole: each component
/// carries its own state and reason.
#[must_use]
pub fn observe_security<W, F>(wmi: &W, firmware: &F) -> SecurityStatusReport
where
    W: WmiTransport + ?Sized,
    F: FirmwareEnvironment + ?Sized,
{
    SecurityStatusReport {
        secure_boot: observe_secure_boot(firmware),
        tpm: observe_tpm(wmi),
        bitlocker: observe_bitlocker(wmi),
    }
}

fn observe_secure_boot<F: FirmwareEnvironment + ?Sized>(firmware: &F) -> SecureBootReport {
    let firmware_type = match firmware.firmware_type() {
        Ok(firmware_type) => firmware_type,
        Err(error) => {
            return SecureBootReport::unavailable(
                SecuritySource::FirmwareType,
                FirmwareType::Unknown,
                format!("GetFirmwareType failed: {error}"),
            );
        }
    };
    match firmware_type {
        FirmwareType::Bios => return SecureBootReport::not_uefi(),
        FirmwareType::Unknown => {
            return SecureBootReport::unavailable(
                SecuritySource::FirmwareType,
                FirmwareType::Unknown,
                "GetFirmwareType reported a boot mode this contract does not name",
            );
        }
        FirmwareType::Uefi => {}
    }

    let mut details = Vec::new();
    // UEFI defines SecureBoot as exactly one byte, 0 or 1. Any other shape is
    // reported as unavailable rather than interpreted.
    let (state, secure_boot_variable) =
        match firmware.read_variable(SECURE_BOOT_VARIABLE, SECURE_BOOT_GUID) {
            Ok(FirmwareVariableState::Present {
                length: Some(1),
                first_byte: Some(1),
            }) => (SecureBootState::Enabled, Some(true)),
            Ok(FirmwareVariableState::Present {
                length: Some(1),
                first_byte: Some(0),
            }) => (SecureBootState::Disabled, Some(false)),
            Ok(FirmwareVariableState::Present { length, first_byte }) => {
                details.push(format!(
                    "{SECURE_BOOT_GUID}:{SECURE_BOOT_VARIABLE} has an unexpected shape \
                     (length {length:?}, first byte {first_byte:?}); UEFI defines one byte, 0 or 1"
                ));
                (SecureBootState::Unavailable, None)
            }
            Ok(FirmwareVariableState::Absent) => {
                details.push(format!(
                    "{SECURE_BOOT_GUID}:{SECURE_BOOT_VARIABLE} is absent on a UEFI machine, so no \
                     firmware Secure Boot state is exposed"
                ));
                (SecureBootState::Unavailable, None)
            }
            Err(error) => {
                details.push(format!(
                    "reading {SECURE_BOOT_GUID}:{SECURE_BOOT_VARIABLE} failed: {error}"
                ));
                (SecureBootState::Unavailable, None)
            }
        };

    let setup_mode = read_boolean_variable(firmware, SETUP_MODE_VARIABLE, &mut details);
    let (platform_key_present, platform_key_bytes) =
        match firmware.read_variable(PLATFORM_KEY_VARIABLE, SECURE_BOOT_GUID) {
            Ok(FirmwareVariableState::Present { length, .. }) => (Some(true), length),
            Ok(FirmwareVariableState::Absent) => (Some(false), None),
            Err(error) => {
                details.push(format!(
                    "reading {SECURE_BOOT_GUID}:{PLATFORM_KEY_VARIABLE} failed: {error}"
                ));
                (None, None)
            }
        };
    let registry_secure_boot_enabled = match firmware
        .registry_dword(SECURE_BOOT_STATE_KEY, SECURE_BOOT_STATE_VALUE)
    {
        Ok(Some(0)) => Some(false),
        Ok(Some(1)) => Some(true),
        Ok(Some(other)) => {
            details.push(format!(
                "HKLM\\{SECURE_BOOT_STATE_KEY}!{SECURE_BOOT_STATE_VALUE} holds {other}, outside the \
                 documented 0/1 values"
            ));
            None
        }
        Ok(None) => {
            details.push(format!(
                "HKLM\\{SECURE_BOOT_STATE_KEY}!{SECURE_BOOT_STATE_VALUE} is absent, so the \
                 operating-system mirror is unknown"
            ));
            None
        }
        Err(error) => {
            details.push(format!(
                "reading HKLM\\{SECURE_BOOT_STATE_KEY}!{SECURE_BOOT_STATE_VALUE} failed: {error}"
            ));
            None
        }
    };

    SecureBootReport {
        state,
        firmware_type,
        source: SecuritySource::UefiFirmwareVariable,
        secure_boot_variable,
        setup_mode,
        platform_key_present,
        platform_key_bytes,
        registry_secure_boot_enabled,
        detail: (!details.is_empty()).then(|| details.join("; ")),
    }
}

fn read_boolean_variable<F: FirmwareEnvironment + ?Sized>(
    firmware: &F,
    name: &str,
    details: &mut Vec<String>,
) -> Option<bool> {
    match firmware.read_variable(name, SECURE_BOOT_GUID) {
        Ok(FirmwareVariableState::Present {
            length: Some(1),
            first_byte: Some(value @ (0 | 1)),
        }) => Some(value == 1),
        Ok(FirmwareVariableState::Present { length, first_byte }) => {
            details.push(format!(
                "{SECURE_BOOT_GUID}:{name} has an unexpected shape (length {length:?}, first byte \
                 {first_byte:?}); UEFI defines one byte, 0 or 1"
            ));
            None
        }
        Ok(FirmwareVariableState::Absent) => {
            details.push(format!("{SECURE_BOOT_GUID}:{name} is absent"));
            None
        }
        Err(error) => {
            details.push(format!("reading {SECURE_BOOT_GUID}:{name} failed: {error}"));
            None
        }
    }
}

fn observe_tpm<W: WmiTransport + ?Sized>(wmi: &W) -> TpmReport {
    let objects = match wmi.query(TPM_NAMESPACE, TPM_QUERY) {
        Ok(objects) => objects,
        Err(error) => {
            return TpmReport::unavailable(format!(
                "{TPM_NAMESPACE}:{TPM_CLASS} query failed: {error}"
            ));
        }
    };
    let Some(object) = objects.first() else {
        return TpmReport::absent();
    };
    TpmReport {
        state: TpmState::Present,
        source: SecuritySource::Win32Tpm,
        enabled: bool_field(object, "IsEnabled_InitialValue"),
        activated: bool_field(object, "IsActivated_InitialValue"),
        owned: bool_field(object, "IsOwned_InitialValue"),
        spec_version: string_field(object, "SpecVersion"),
        manufacturer_id: u32_field(object, "ManufacturerId"),
        manufacturer_id_text: string_field(object, "ManufacturerIdTxt"),
        manufacturer_version: string_field(object, "ManufacturerVersion"),
        manufacturer_version_full: string_field(object, "ManufacturerVersionFull20"),
        manufacturer_version_info: string_field(object, "ManufacturerVersionInfo"),
        physical_presence_version: string_field(object, "PhysicalPresenceVersionInfo"),
        detail: (objects.len() > 1).then(|| {
            format!(
                "the singleton class returned {} instances; the first was reported",
                objects.len()
            )
        }),
    }
}

fn observe_bitlocker<W: WmiTransport + ?Sized>(wmi: &W) -> BitLockerReport {
    let objects = match wmi.query(BITLOCKER_NAMESPACE, BITLOCKER_QUERY) {
        Ok(objects) => objects,
        Err(error) => {
            return BitLockerReport::unavailable(format!(
                "{BITLOCKER_NAMESPACE}:{BITLOCKER_CLASS} query failed: {error}"
            ));
        }
    };
    let mut details = Vec::new();
    if objects.len() > MAX_BITLOCKER_VOLUMES {
        details.push(format!(
            "{} encryptable volume(s) were enumerated; only the first {MAX_BITLOCKER_VOLUMES} were \
             interrogated (docs/evidence/re-security-20261105.md)",
            objects.len()
        ));
    }
    let volumes = objects
        .iter()
        .take(MAX_BITLOCKER_VOLUMES)
        .map(|object| observe_volume(wmi, object))
        .collect();
    BitLockerReport {
        state: BitLockerState::Observed,
        source: SecuritySource::Win32EncryptableVolume,
        volumes,
        detail: (!details.is_empty()).then(|| details.join("; ")),
    }
}

fn observe_volume<W: WmiTransport + ?Sized>(wmi: &W, object: &WmiObject) -> BitLockerVolume {
    let (protection_status_raw, protection_status) = typed_u32(
        object,
        "ProtectionStatus",
        BitLockerProtectionStatus::from_raw,
    );
    let (conversion_status_raw, conversion_status) = typed_u32(
        object,
        "ConversionStatus",
        BitLockerConversionStatus::from_raw,
    );
    let (encryption_method_raw, encryption_method) = typed_u32(
        object,
        "EncryptionMethod",
        BitLockerEncryptionMethod::from_raw,
    );
    let (volume_type_raw, volume_type) =
        typed_u32(object, "VolumeType", BitLockerVolumeType::from_raw);

    let mut details = Vec::new();
    let mut lock_status: (Option<u32>, Option<BitLockerLockStatus>) = (None, None);
    let mut encryption_percentage = None;
    match instance_path(object) {
        Some(path) => {
            lock_status = match wmi.invoke_instance(
                BITLOCKER_NAMESPACE,
                BITLOCKER_CLASS,
                &path,
                "GetLockStatus",
                &WmiObject::new(),
            ) {
                Ok(output) => {
                    let raw = u32_field(&output, "LockStatus");
                    (raw, raw.and_then(BitLockerLockStatus::from_raw))
                }
                Err(error) => {
                    details.push(format!("GetLockStatus failed: {error}"));
                    (None, None)
                }
            };
            // `GetConversionStatus` declares one IN parameter, `PrecisionFactor`
            // (provider MOF line 126). It is passed explicitly as 0, the value
            // the operating system's own cmdlet used, so the call has no unset
            // in-parameter.
            let input = WmiObject::from([("PrecisionFactor".to_string(), WmiValue::U32(0))]);
            encryption_percentage = match wmi.invoke_instance(
                BITLOCKER_NAMESPACE,
                BITLOCKER_CLASS,
                &path,
                "GetConversionStatus",
                &input,
            ) {
                Ok(output) => u32_field(&output, "EncryptionPercentage"),
                Err(error) => {
                    details.push(format!("GetConversionStatus failed: {error}"));
                    None
                }
            };
        }
        None => details.push(
            "the provider returned neither __Path nor DeviceID, so the read-only getters could not \
             be addressed"
                .into(),
        ),
    }

    BitLockerVolume {
        device_id: string_field(object, "DeviceID"),
        drive_letter: string_field(object, "DriveLetter"),
        persistent_volume_id: string_field(object, "PersistentVolumeID"),
        protection_status,
        protection_status_raw,
        conversion_status,
        conversion_status_raw,
        encryption_method,
        encryption_method_raw,
        volume_type,
        volume_type_raw,
        initialized_for_protection: bool_field(object, "IsVolumeInitializedForProtection"),
        lock_status: lock_status.1,
        lock_status_raw: lock_status.0,
        encryption_percentage,
        detail: (!details.is_empty()).then(|| details.join("; ")),
    }
}

/// The WMI object path for one volume: the provider's own `__Path` when it
/// returned one, otherwise the declared key property.
fn instance_path(object: &WmiObject) -> Option<String> {
    if let Some(WmiValue::String(path)) = object.get("__Path") {
        if !path.is_empty() {
            return Some(path.clone());
        }
    }
    let device_id = string_field(object, "DeviceID")?;
    Some(format!(
        "{BITLOCKER_CLASS}.DeviceID={}",
        quote_wql(&device_id)
    ))
}

fn quote_wql(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn bool_field(object: &WmiObject, name: &str) -> Option<bool> {
    match object.get(name)? {
        WmiValue::Bool(value) => Some(*value),
        _ => None,
    }
}

fn string_field(object: &WmiObject, name: &str) -> Option<String> {
    match object.get(name)? {
        WmiValue::String(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    match object.get(name)? {
        WmiValue::U8(value) => Some(u32::from(*value)),
        WmiValue::U16(value) => Some(u32::from(*value)),
        WmiValue::U32(value) => Some(*value),
        WmiValue::I8(value) => u32::try_from(*value).ok(),
        WmiValue::I16(value) => u32::try_from(*value).ok(),
        WmiValue::I32(value) => u32::try_from(*value).ok(),
        WmiValue::U64(value) => u32::try_from(*value).ok(),
        WmiValue::I64(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn typed_u32<T>(
    object: &WmiObject,
    name: &str,
    decode: impl Fn(u32) -> Option<T>,
) -> (Option<u32>, Option<T>) {
    let raw = u32_field(object, name);
    (raw, raw.and_then(decode))
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;

#[cfg(windows)]
mod native {
    use std::mem::{size_of, zeroed};
    use std::ptr;

    use lctrl_core::{FirmwareType, LctrlError, Result};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_ENVVAR_NOT_FOUND, ERROR_FILE_NOT_FOUND,
        ERROR_INSUFFICIENT_BUFFER, ERROR_NOT_ALL_ASSIGNED, ERROR_PATH_NOT_FOUND,
        ERROR_PRIVILEGE_NOT_HELD, GetLastError, HANDLE,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        SE_SYSTEM_ENVIRONMENT_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
    use windows_sys::Win32::System::SystemInformation::{
        FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::System::WindowsProgramming::GetFirmwareEnvironmentVariableExW;

    use super::{FIRMWARE_VARIABLE_BUFFER_BYTES, FirmwareEnvironment, FirmwareVariableState};
    use crate::map_win_error;

    /// `SeSystemEnvironmentPrivilege` check, `GetFirmwareType`, and
    /// `GetFirmwareEnvironmentVariableExW` (read) plus one read-only
    /// `RegGetValueW`. No firmware variable writer is imported.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeFirmwareEnvironment;

    impl FirmwareEnvironment for NativeFirmwareEnvironment {
        fn firmware_type(&self) -> Result<FirmwareType> {
            let mut firmware_type: FIRMWARE_TYPE = 0;
            // SAFETY: the API writes one FIRMWARE_TYPE into the out pointer.
            let ok = unsafe { GetFirmwareType(&mut firmware_type) };
            if ok == 0 {
                let error = unsafe { GetLastError() };
                return Err(map_win_error(error, "GetFirmwareType"));
            }
            Ok(match firmware_type {
                FirmwareTypeUefi => FirmwareType::Uefi,
                FirmwareTypeBios => FirmwareType::Bios,
                _ => FirmwareType::Unknown,
            })
        }

        fn read_variable(&self, name: &str, guid: &str) -> Result<FirmwareVariableState> {
            enable_system_environment_privilege()?;
            let name_wide = wide_string(name);
            let guid_wide = wide_string(guid);
            let mut buffer = vec![0u8; FIRMWARE_VARIABLE_BUFFER_BYTES];
            let mut attributes = 0u32;
            // SAFETY: both pointers are NUL-terminated UTF-16 strings, and the
            // output buffer has the exact length passed to the API.
            let length = unsafe {
                GetFirmwareEnvironmentVariableExW(
                    name_wide.as_ptr(),
                    guid_wide.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                    &mut attributes,
                )
            };
            if length == 0 {
                let error = unsafe { GetLastError() };
                return match error {
                    ERROR_ENVVAR_NOT_FOUND => Ok(FirmwareVariableState::Absent),
                    // Present but larger than the bound: report presence only.
                    ERROR_INSUFFICIENT_BUFFER => Ok(FirmwareVariableState::Present {
                        length: None,
                        first_byte: None,
                    }),
                    ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD => {
                        Err(LctrlError::PermissionDenied {
                            need: "SeSystemEnvironmentPrivilege (elevated administrator)".into(),
                        })
                    }
                    error => Err(map_win_error(
                        error,
                        &format!("reading firmware variable {guid}:{name}"),
                    )),
                };
            }
            let length = usize::try_from(length).unwrap_or(buffer.len()).min(buffer.len());
            Ok(FirmwareVariableState::Present {
                length: Some(length),
                first_byte: buffer.first().copied(),
            })
        }

        fn registry_dword(&self, key: &str, value: &str) -> Result<Option<u32>> {
            let subkey = wide_string(key);
            let value_name = wide_string(value);
            let mut data = 0u32;
            let mut data_size = u32::try_from(size_of::<u32>()).unwrap_or(4);
            // SAFETY: `data` is a u32 and `data_size` states its exact size.
            let status = unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    subkey.as_ptr(),
                    value_name.as_ptr(),
                    RRF_RT_REG_DWORD,
                    ptr::null_mut(),
                    ptr::from_mut(&mut data).cast(),
                    &mut data_size,
                )
            };
            match status {
                0 if data_size == size_of::<u32>() as u32 => Ok(Some(data)),
                0 => Ok(None),
                ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
                error => Err(map_win_error(
                    error,
                    &format!("reading HKLM\\{key}!{value}"),
                )),
            }
        }
    }

    fn wide_string(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Enable `SeSystemEnvironmentPrivilege` on this process token. Reading a
    /// UEFI variable requires it; granting it is not a firmware write.
    fn enable_system_environment_privilege() -> Result<()> {
        // SAFETY: the token handle is closed on every path, and the privilege
        // structures are plain data with the size the API expects.
        unsafe {
            let mut token: HANDLE = ptr::null_mut();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            ) == 0
            {
                let error = GetLastError();
                return Err(map_win_error(error, "OpenProcessToken"));
            }
            let mut luid = zeroed();
            if LookupPrivilegeValueW(ptr::null(), SE_SYSTEM_ENVIRONMENT_NAME, &mut luid) == 0 {
                let error = GetLastError();
                CloseHandle(token);
                return Err(map_win_error(
                    error,
                    "LookupPrivilegeValue(SeSystemEnvironmentPrivilege)",
                ));
            }
            let mut privileges: TOKEN_PRIVILEGES = zeroed();
            privileges.PrivilegeCount = 1;
            privileges.Privileges[0] = LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            };
            let adjusted = AdjustTokenPrivileges(
                token,
                0,
                &privileges,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            let error = GetLastError();
            CloseHandle(token);
            if adjusted == 0 {
                return Err(map_win_error(
                    error,
                    "AdjustTokenPrivileges(SeSystemEnvironmentPrivilege)",
                ));
            }
            // AdjustTokenPrivileges reports success even when the privilege was
            // not held; only GetLastError distinguishes that case.
            if error == ERROR_NOT_ALL_ASSIGNED {
                return Err(LctrlError::PermissionDenied {
                    need: "SeSystemEnvironmentPrivilege (elevated administrator)".into(),
                });
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
pub use native::NativeFirmwareEnvironment;
