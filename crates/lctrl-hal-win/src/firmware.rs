//! Read-only firmware inventory: BIOS/EC identity, the platform's EFI System
//! Resource Table (ESRT) projection, and the firmware update driver's last
//! attempt.
//!
//! Nothing in this module stages, flashes or clears a capsule. The only
//! firmware-facing calls are reads: `Win32_BIOS`, `Win32_PnPEntity`,
//! `GetFirmwareType`, the firmware-resource property store, and the
//! `FirmwareResources` registry record.
//!
//! Contract and live values on 21VG: `docs/evidence/re-firmware-20261105.md`.

use std::collections::BTreeMap;

use lctrl_core::{
    FirmwareIdentity, FirmwareResource, FirmwareStatus, FirmwareType, LctrlError, Result,
    firmware_class, firmware_revision,
};
use lctrl_hal::FirmwareControl;

use crate::wmi_contract::{WmiObject, WmiTransport, WmiValue};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";

/// BIOS identity. `Win32_BIOS.Version` is deliberately absent: it is the
/// firmware vendor/ordinal string, never the version.
const BIOS_QUERY: &str = "SELECT Manufacturer, SMBIOSBIOSVersion, ReleaseDate, SMBIOSMajorVersion, SMBIOSMinorVersion, SystemBiosMajorVersion, SystemBiosMinorVersion, EmbeddedControllerMajorVersion, EmbeddedControllerMinorVersion, BIOSVersion FROM Win32_BIOS";

/// Every ESRT entry is projected as a device of the `Firmware` PnP class.
const FIRMWARE_DEVICE_QUERY: &str =
    "SELECT DeviceID, Name, HardwareID, CompatibleID, Status FROM Win32_PnPEntity WHERE PNPClass = 'Firmware'";

/// A standalone read-only firmware inventory service.
///
/// The WMI transport is the injectable part; the two platform-only reads
/// (firmware-resource property store, `FirmwareResources` record) live in
/// [`native`] and are absent off Windows.
#[derive(Debug)]
pub struct WindowsFirmware<W> {
    wmi: W,
}

impl<W> WindowsFirmware<W> {
    #[must_use]
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }
}

/// Probe whether the platform projects at least one firmware-class (ESRT)
/// device.
///
/// This is the capability-matrix probe behind `firmware.status`: it answers
/// only "does the firmware table exist here", and a legacy-BIOS machine
/// correctly answers no.
#[cfg(windows)]
#[must_use]
pub(crate) fn probe_firmware_status<W: WmiTransport>(wmi: &W) -> bool {
    matches!(
        wmi.query(ROOT_CIMV2, FIRMWARE_DEVICE_QUERY),
        Ok(objects) if !objects.is_empty()
    )
}

impl<W> FirmwareControl for WindowsFirmware<W>
where
    W: WmiTransport,
{
    fn firmware_status(&self) -> Result<FirmwareStatus> {
        let mut issues = Vec::new();
        let identity = identity_from_wmi(&self.wmi, &mut issues)?;
        let mut resources = resources_from_wmi(&self.wmi, &mut issues)?;
        if resources.is_empty() && identity.firmware_type == FirmwareType::Uefi {
            issues.push(
                "UEFI platform published no firmware-class device, so no ESRT entry is visible"
                    .into(),
            );
        }
        let stored = native::stored_versions(&mut issues);
        let matched = apply_stored_versions(&mut resources, &stored);
        if matched < stored.len() {
            issues.push(format!(
                "the firmware-resource property store listed {} entries the PnP enumeration did not",
                stored.len() - matched
            ));
        }
        let attempts = native::read_attempts(&mut issues);
        Ok(FirmwareStatus {
            identity,
            resources,
            attempts,
            issues,
        })
    }
}

/// Attach each resource's independently-read version and record a
/// disagreement. Returns how many store entries matched a resource.
///
/// The documented hardware-id derivation stays authoritative
/// (`current_version`); the property store's value is comparison only, so a
/// disagreement is visible instead of one value replacing the other.
fn apply_stored_versions(
    resources: &mut [FirmwareResource],
    stored: &BTreeMap<String, u32>,
) -> usize {
    let mut matched = 0;
    for resource in resources.iter_mut() {
        let Some(stored_version) = stored.get(&resource.firmware_class) else {
            continue;
        };
        matched += 1;
        resource.stored_version = Some(*stored_version);
        let disagreement = resource
            .current_version
            .filter(|current| current != stored_version);
        if let Some(current) = disagreement {
            resource.issues.push(format!(
                "hardware-id revision 0x{current:X} disagrees with the firmware-resource property store 0x{stored_version:X}"
            ));
        }
    }
    matched
}

fn identity_from_wmi<W: WmiTransport>(
    wmi: &W,
    issues: &mut Vec<String>,
) -> Result<FirmwareIdentity> {
    let objects = wmi.query(ROOT_CIMV2, BIOS_QUERY)?;
    let object = objects.first().ok_or_else(|| LctrlError::ChannelUnavailable {
        channel: "Win32_BIOS returned no instance".into(),
    })?;
    Ok(FirmwareIdentity {
        firmware_type: native::firmware_type(issues),
        vendor: string_field(object, "Manufacturer"),
        version: string_field(object, "SMBIOSBIOSVersion"),
        release_date: string_field(object, "ReleaseDate"),
        bios_release_major: u16_field(object, "SystemBiosMajorVersion"),
        bios_release_minor: u16_field(object, "SystemBiosMinorVersion"),
        ec_firmware_major: u16_field(object, "EmbeddedControllerMajorVersion"),
        ec_firmware_minor: u16_field(object, "EmbeddedControllerMinorVersion"),
        smbios_major: u16_field(object, "SMBIOSMajorVersion"),
        smbios_minor: u16_field(object, "SMBIOSMinorVersion"),
        version_strings: string_list_field(object, "BIOSVersion"),
    })
}

/// Map the firmware-class devices into ESRT entries.
///
/// Field provenance: `DeviceID` is the instance id, `HardwareID` carries the
/// `&REV_<hex>` current version, and `Name`/`Status` are the device
/// description and status the platform publishes.
fn resources_from_wmi<W: WmiTransport>(
    wmi: &W,
    issues: &mut Vec<String>,
) -> Result<Vec<FirmwareResource>> {
    let objects = wmi.query(ROOT_CIMV2, FIRMWARE_DEVICE_QUERY)?;
    let mut resources = Vec::with_capacity(objects.len());
    for object in &objects {
        let Some(instance_id) = string_field(object, "DeviceID") else {
            issues.push("a Firmware-class PnP instance reported no DeviceID".into());
            continue;
        };
        let hardware_ids = string_list_field(object, "HardwareID");
        let resource_class = hardware_ids
            .iter()
            .find_map(|hardware_id| firmware_class(hardware_id))
            .or_else(|| firmware_class(&instance_id));
        let Some(resource_class) = resource_class else {
            issues.push(format!(
                "{instance_id}: no firmware-resource class in the hardware ids"
            ));
            continue;
        };
        let current_version = hardware_ids
            .iter()
            .find_map(|hardware_id| firmware_revision(hardware_id));
        resources.push(FirmwareResource {
            instance_id,
            firmware_class: resource_class,
            current_version,
            stored_version: None,
            description: string_field(object, "Name"),
            status: string_field(object, "Status"),
            hardware_ids,
            compatible_ids: string_list_field(object, "CompatibleID"),
            issues: Vec::new(),
        });
    }
    Ok(resources)
}

fn string_field(object: &WmiObject, name: &str) -> Option<String> {
    match object.get(name) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

/// String arrays, keeping the order the provider returned and dropping only
/// genuinely empty elements (an entry's revision id must not be shifted).
fn string_list_field(object: &WmiObject, name: &str) -> Vec<String> {
    match object.get(name) {
        Some(WmiValue::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                WmiValue::String(value) if !value.is_empty() => Some(value.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn u16_field(object: &WmiObject, name: &str) -> Option<u16> {
    let value = match object.get(name)? {
        WmiValue::U8(value) => u16::from(*value),
        WmiValue::U16(value) => *value,
        WmiValue::U32(value) => u16::try_from(*value).ok()?,
        WmiValue::I8(value) => u16::try_from(*value).ok()?,
        WmiValue::I16(value) => u16::try_from(*value).ok()?,
        WmiValue::I32(value) => u16::try_from(*value).ok()?,
        WmiValue::U64(value) => u16::try_from(*value).ok()?,
        WmiValue::I64(value) => u16::try_from(*value).ok()?,
        _ => return None,
    };
    Some(value)
}

#[cfg(windows)]
mod native {
    use std::collections::BTreeMap;
    use std::mem::size_of;
    use std::ptr;

    use lctrl_core::{FirmwareAttempt, FirmwareType, firmware_class, last_attempt_status_meaning};
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW,
        SetupDiGetDevicePropertyW,
    };
    use windows_sys::Win32::Devices::Properties::DEVPROP_TYPE_UINT32;
    use windows_sys::Win32::Foundation::{
        DEVPROPKEY, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, GetLastError,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegOpenKeyExW, RegQueryValueExW,
    };
    use windows_sys::Win32::System::SystemInformation::{
        FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType,
    };

    /// The firmware update driver's per-resource record.
    const FIRMWARE_RESOURCES_KEY: &str = r"SYSTEM\CurrentControlSet\Control\FirmwareResources";
    /// The ESRT devices live under the `UEFI` PnP enumerator.
    const UEFI_ENUMERATOR: &[u16] = &[b'U' as u16, b'E' as u16, b'F' as u16, b'I' as u16, 0];

    /// `DEVPKEY_FirmwareResource_Version`: property group
    /// `{80497100-8C73-48B9-AAD9-CE387E19C56E}`, pid 4.
    ///
    /// The named-key set `windows-sys` exposes for this SDK does not declare
    /// the firmware-resource group, so the key is spelled out here. Pids 2/3/4/5
    /// of the group are the resource Id, Type, Version and
    /// LowestSupportedVersion; the live cross-check of pid 4 against the
    /// hardware-id revision is in `docs/evidence/re-firmware-20261105.md`.
    const FIRMWARE_RESOURCE_VERSION: DEVPROPKEY = DEVPROPKEY {
        fmtid: windows_sys::core::GUID::from_u128(0x80497100_8c73_48b9_aad9_ce387e19c56e),
        pid: 4,
    };

    /// Ask the kernel whether the machine booted UEFI or legacy BIOS.
    pub(super) fn firmware_type(issues: &mut Vec<String>) -> FirmwareType {
        let mut firmware_type: FIRMWARE_TYPE = 0;
        if unsafe { GetFirmwareType(&mut firmware_type) } == 0 {
            let error = unsafe { GetLastError() };
            issues.push(format!("GetFirmwareType failed with {error}"));
            return FirmwareType::Unknown;
        }
        match firmware_type {
            FirmwareTypeBios => FirmwareType::Bios,
            FirmwareTypeUefi => FirmwareType::Uefi,
            _ => {
                issues.push(format!(
                    "GetFirmwareType returned an unnamed firmware type {firmware_type}"
                ));
                FirmwareType::Unknown
            }
        }
    }

    /// Read `DEVPKEY_FirmwareResource_Version` for every ESRT device, keyed by
    /// the normalised firmware class.
    ///
    /// A device whose property cannot be read is reported instead of being
    /// assumed equal to the hardware-id derivation.
    pub(super) fn stored_versions(issues: &mut Vec<String>) -> BTreeMap<String, u32> {
        let mut versions = BTreeMap::new();
        let handle = unsafe {
            SetupDiGetClassDevsW(
                ptr::null(),
                UEFI_ENUMERATOR.as_ptr(),
                ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            )
        };
        if handle == -1 {
            let error = unsafe { GetLastError() };
            issues.push(format!(
                "SetupDiGetClassDevsW(UEFI) failed with {error}; the firmware-resource property store was not read"
            ));
            return versions;
        }
        let set = DeviceSet(handle);
        let mut index = 0u32;
        let mut unreadable = Vec::new();
        loop {
            let mut data = device_data();
            if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut data) } == 0 {
                let error = unsafe { GetLastError() };
                if error != ERROR_NO_MORE_ITEMS {
                    issues.push(format!("SetupDiEnumDeviceInfo failed with {error}"));
                }
                break;
            }
            index += 1;
            let Some(instance) = device_instance_id(set.0, &data) else {
                unreadable.push("<unnamed instance>".to_string());
                continue;
            };
            let Some(class) = firmware_class(&instance) else {
                // Not an ESRT resource device: the UEFI enumerator also owns
                // the `ACPI_HAL\UEFI` HAL node itself.
                continue;
            };
            match device_u32_property(set.0, &data, &FIRMWARE_RESOURCE_VERSION) {
                Some(version) => {
                    versions.insert(class, version);
                }
                None => unreadable.push(class),
            }
        }
        if !unreadable.is_empty() {
            issues.push(format!(
                "firmware-resource version property unreadable for {}: {}",
                unreadable.len(),
                unreadable.join(", ")
            ));
        }
        versions
    }

    /// Read the firmware update driver's recorded attempts.
    ///
    /// `LastAttemptStatus` is kept verbatim and named only when this build
    /// knows the documented value; `Phase` is reported without interpretation.
    pub(super) fn read_attempts(issues: &mut Vec<String>) -> Vec<FirmwareAttempt> {
        let mut attempts = Vec::new();
        let key_w = wide(FIRMWARE_RESOURCES_KEY);
        let mut handle: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, key_w.as_ptr(), 0, KEY_READ, &mut handle) };
        if status != 0 {
            // A machine that has never staged a capsule has no such key; that
            // is an empty record, not a failure.
            if status != ERROR_FILE_NOT_FOUND {
                issues.push(format!(
                    "open {FIRMWARE_RESOURCES_KEY} failed with {status}"
                ));
            }
            return attempts;
        }
        let key = RegKey(handle);
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 128];
            let mut length = name.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != 0 {
                issues.push(format!(
                    "enumerate {FIRMWARE_RESOURCES_KEY} failed with {status}"
                ));
                break;
            }
            index += 1;
            let subkey_name = String::from_utf16_lossy(&name[..length as usize]);
            let Some(class) = firmware_class(&subkey_name) else {
                issues.push(format!(
                    "{FIRMWARE_RESOURCES_KEY}\\{subkey_name}: not a firmware class"
                ));
                continue;
            };
            let subkey_w = wide(&subkey_name);
            let mut subkey: HKEY = ptr::null_mut();
            let status =
                unsafe { RegOpenKeyExW(key.0, subkey_w.as_ptr(), 0, KEY_READ, &mut subkey) };
            if status != 0 {
                issues.push(format!(
                    "open {FIRMWARE_RESOURCES_KEY}\\{subkey_name} failed with {status}"
                ));
                continue;
            }
            let subkey = RegKey(subkey);
            let last_attempt_status = read_dword(subkey.0, "LastAttemptStatus");
            attempts.push(FirmwareAttempt {
                firmware_class: class,
                version: read_dword(subkey.0, "Version"),
                last_attempt_version: read_dword(subkey.0, "LastAttemptVersion"),
                last_attempt_status,
                last_attempt_outcome: last_attempt_status.and_then(last_attempt_status_meaning),
                phase: read_dword(subkey.0, "Phase"),
                filename: read_text(subkey.0, "Filename"),
                catalog: read_text(subkey.0, "Catalog"),
            });
        }
        attempts
    }

    /// Owned `HDEVINFO` that is destroyed on drop.
    struct DeviceSet(HDEVINFO);

    impl Drop for DeviceSet {
        fn drop(&mut self) {
            unsafe { SetupDiDestroyDeviceInfoList(self.0) };
        }
    }

    /// Owned `HKEY` that closes on drop.
    struct RegKey(HKEY);

    impl Drop for RegKey {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn device_data() -> SP_DEVINFO_DATA {
        SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..SP_DEVINFO_DATA::default()
        }
    }

    fn device_instance_id(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Option<String> {
        let mut buffer = [0u16; 512];
        let mut required = 0u32;
        let ok = unsafe {
            SetupDiGetDeviceInstanceIdW(
                set,
                data,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut required,
            )
        };
        if ok == 0 {
            return None;
        }
        let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
        (length > 0).then(|| String::from_utf16_lossy(&buffer[..length]))
    }

    fn device_u32_property(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<u32> {
        let mut property_type = 0u32;
        let mut value = 0u32;
        let ok = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                data,
                key,
                &mut property_type,
                ptr::from_mut(&mut value).cast::<u8>(),
                size_of::<u32>() as u32,
                ptr::null_mut(),
                0,
            )
        };
        if ok == 0 || property_type != DEVPROP_TYPE_UINT32 {
            return None;
        }
        Some(value)
    }

    fn read_raw(handle: HKEY, name: &str) -> Option<(u32, Vec<u8>)> {
        let name_w = wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                handle,
                name_w.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if status != 0 || size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                handle,
                name_w.as_ptr(),
                ptr::null(),
                &mut kind,
                data.as_mut_ptr(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }
        data.truncate(size as usize);
        Some((kind, data))
    }

    fn read_dword(handle: HKEY, name: &str) -> Option<u32> {
        match read_raw(handle, name) {
            Some((REG_DWORD, data)) if data.len() >= 4 => {
                Some(u32::from_le_bytes(data[..4].try_into().ok()?))
            }
            _ => None,
        }
    }

    fn read_text(handle: HKEY, name: &str) -> Option<String> {
        let Some((REG_SZ, data)) = read_raw(handle, name) else {
            return None;
        };
        if data.len() < 2 || data.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = data
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        let text = text.trim_end_matches('\0');
        (!text.is_empty()).then(|| text.to_string())
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(not(windows))]
mod native {
    use std::collections::BTreeMap;

    use lctrl_core::{FirmwareAttempt, FirmwareType};

    pub(super) fn firmware_type(issues: &mut Vec<String>) -> FirmwareType {
        issues.push("firmware type requires the Windows firmware API".into());
        FirmwareType::Unknown
    }

    pub(super) fn stored_versions(issues: &mut Vec<String>) -> BTreeMap<String, u32> {
        issues.push("the firmware-resource property store requires the Windows SetupAPI".into());
        BTreeMap::new()
    }

    pub(super) fn read_attempts(issues: &mut Vec<String>) -> Vec<FirmwareAttempt> {
        issues.push("the firmware-resource record requires the Windows registry".into());
        Vec::new()
    }
}

#[cfg(test)]
#[path = "firmware_tests.rs"]
mod tests;
