//! Firmware identity and the UEFI firmware-resource table, read-only.
//!
//! This module is the cross-platform shape of `vantage firmware status`. It
//! carries no write operation of any kind: a firmware capsule is never staged,
//! flashed or cleared from here, and there is no method that could be mistaken
//! for one.
//!
//! Windows projects every EFI System Resource Table (ESRT) entry as a
//! firmware-class device `UEFI\RES_{<FirmwareClass>}\0`. The documented
//! platform contract (Windows hardware documentation, `QueryVersionAndStatus`
//! reference script) derives an entry's current firmware version from the
//! revision encoded in its hardware id (`UEFI\RES_{...}&REV_<hex>`), and the
//! last update attempt from the firmware update driver's record under
//! `HKLM\SYSTEM\CurrentControlSet\Control\FirmwareResources\{<FirmwareClass>}`.
//! Both are inventory reads; neither is a firmware-capsule interface.
//!
//! Contract and live values on 21VG: `docs/evidence/re-firmware-20261105.md`.

use std::fmt;

use serde::Serialize;

/// How the machine was booted, from the platform firmware type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareType {
    /// Legacy PC/AT BIOS (no configuration tables, no ESRT).
    Bios,
    /// UEFI firmware.
    Uefi,
    /// The platform did not answer, or returned a value this build cannot name.
    Unknown,
}

impl fmt::Display for FirmwareType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bios => "BIOS",
            Self::Uefi => "UEFI",
            Self::Unknown => "unknown",
        })
    }
}

/// BIOS and embedded-controller identity, as the platform reports it.
///
/// Field provenance is documented per field; nothing is derived from another
/// value when the platform omits it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FirmwareIdentity {
    pub firmware_type: FirmwareType,
    /// Firmware vendor (`Win32_BIOS.Manufacturer`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// BIOS version string (`Win32_BIOS.SMBIOSBIOSVersion`); `Win32_BIOS.Version`
    /// is a vendor/ordinal string, never the version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Release date exactly as delivered (`Win32_BIOS.ReleaseDate`, DMTF text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// BIOS release major/minor (`Win32_BIOS.SystemBiosMajorVersion`/`.MinorVersion`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bios_release_major: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bios_release_minor: Option<u16>,
    /// Embedded-controller firmware major/minor
    /// (`Win32_BIOS.EmbeddedControllerMajorVersion`/`.MinorVersion`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec_firmware_major: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ec_firmware_minor: Option<u16>,
    /// SMBIOS implementation version (`Win32_BIOS.SMBIOSMajorVersion`/`.MinorVersion`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smbios_major: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smbios_minor: Option<u16>,
    /// `Win32_BIOS.BIOSVersion` verbatim: the firmware-described version list
    /// (vendor ordinal string, SMBIOS BIOS version, and the independent
    /// code-base build, in that order on this SKU).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub version_strings: Vec<String>,
}

impl FirmwareIdentity {
    /// BIOS release as `major.minor`, only when the provider published both.
    #[must_use]
    pub fn bios_release(&self) -> Option<String> {
        pair(self.bios_release_major, self.bios_release_minor)
    }

    /// Embedded-controller firmware version as `major.minor`, only when both
    /// halves were published.
    #[must_use]
    pub fn ec_firmware(&self) -> Option<String> {
        pair(self.ec_firmware_major, self.ec_firmware_minor)
    }

    /// SMBIOS implementation version as `major.minor`, only when both halves
    /// were published.
    #[must_use]
    pub fn smbios_version(&self) -> Option<String> {
        pair(self.smbios_major, self.smbios_minor)
    }
}

fn pair(major: Option<u16>, minor: Option<u16>) -> Option<String> {
    match (major, minor) {
        (Some(major), Some(minor)) => Some(format!("{major}.{minor}")),
        _ => None,
    }
}

/// One EFI System Resource Table entry as the platform projects it.
///
/// `current_version` and `stored_version` are two independent reads of the
/// same ESRT field. Both are always serialized (as `null` when absent) so a
/// missing value stays visible, and a disagreement is reported in `issues`
/// instead of one value silently replacing the other.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FirmwareResource {
    /// Device instance id, `UEFI\RES_{<FirmwareClass>}\<instance>`.
    pub instance_id: String,
    /// The entry's firmware class, normalised as `{XXXXXXXX-XXXX-...}`.
    pub firmware_class: String,
    /// Current firmware version derived from the documented
    /// `&REV_<hex>` hardware-id suffix.
    pub current_version: Option<u32>,
    /// The same field read from the firmware-resource property store
    /// (`DEVPKEY_FirmwareResource_Version`), when that store answers.
    pub stored_version: Option<u32>,
    /// Device description the platform publishes for the entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Device status string, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Hardware ids verbatim, in platform order (the revision-bearing id is
    /// first on the documented layout).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hardware_ids: Vec<String>,
    /// Compatible ids verbatim.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compatible_ids: Vec<String>,
    /// Cross-check findings for this entry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

impl FirmwareResource {
    /// The version to display: the documented hardware-id derivation, falling
    /// back to the property store only when the hardware id carried no
    /// revision at all. A disagreement never falls back silently; it is
    /// recorded in `issues`.
    #[must_use]
    pub const fn reported_version(&self) -> Option<u32> {
        match self.current_version {
            Some(version) => Some(version),
            None => self.stored_version,
        }
    }
}

/// One firmware update attempt recorded by the platform's firmware update
/// driver.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FirmwareAttempt {
    /// The firmware class the attempt targeted, normalised as `{GUID}`.
    pub firmware_class: String,
    /// The version the attempted capsule carried.
    pub version: Option<u32>,
    /// The version the platform reports as last attempted.
    pub last_attempt_version: Option<u32>,
    /// The ESRT `LastAttemptStatus` value, verbatim.
    pub last_attempt_status: Option<u32>,
    /// The documented meaning of `last_attempt_status`, when this build can
    /// name it. An unnamed value stays `None` so the raw number is printed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_outcome: Option<&'static str>,
    /// The driver's phase value, verbatim: not interpreted here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<u32>,
    /// Capsule or image path the driver recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Catalog the driver recorded for the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
}

/// The complete read-only firmware status.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FirmwareStatus {
    pub identity: FirmwareIdentity,
    /// Every ESRT entry the platform projects, in platform order.
    pub resources: Vec<FirmwareResource>,
    /// Every recorded update attempt, in platform order.
    pub attempts: Vec<FirmwareAttempt>,
    /// Read-level findings that are not tied to one entry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

/// The ESRT `LastAttemptStatus` values this build can name.
///
/// The UEFI specification reserves every other value for the firmware vendor
/// or the OEM, so an unknown status is reported as its raw number: naming it
/// "success" would be an invented result.
#[must_use]
pub const fn last_attempt_status_meaning(status: u32) -> Option<&'static str> {
    match status {
        // ESRT: the last update attempt completed successfully.
        0 => Some("success"),
        // ESRT: no update attempt was made.
        0xFFFF_FFFF => Some("no attempt"),
        _ => None,
    }
}

/// The `UEFI\RES_{...}` hardware-id prefix, as the platform publishes it
/// (identifier names are case-insensitive, so matching upper-cases first).
const RESOURCE_PREFIX: &str = "UEFI\\RES_{";
/// The revision suffix appended to the first hardware id of an ESRT device.
const REVISION_SUFFIX: &str = "&REV_";

/// Extract the firmware class from an ESRT device hardware id or registry key
/// name, normalised to `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
///
/// Accepts `UEFI\RES_{guid}`, `UEFI\RES_{guid}&REV_<hex>` and a bare `{guid}`
/// key name. A value that is not a canonical GUID body yields `None` rather
/// than a partially-parsed identifier.
#[must_use]
pub fn firmware_class(value: &str) -> Option<String> {
    let upper = value.to_ascii_uppercase();
    let body = if let Some(start) = upper.find(RESOURCE_PREFIX) {
        let rest = &upper[start + RESOURCE_PREFIX.len()..];
        let end = rest.find('}')?;
        &rest[..end]
    } else {
        let start = upper.find('{')?;
        let rest = &upper[start + 1..];
        let end = rest.find('}')?;
        &rest[..end]
    };
    is_guid_body(body).then(|| format!("{{{body}}}"))
}

/// Extract the current firmware version from an ESRT device hardware id.
///
/// The documented layout encodes it as `&REV_<hex>` at the end of the revision
/// hardware id. More than eight hexadecimal digits cannot be an ESRT
/// `UINT32` version and yields `None`.
#[must_use]
pub fn firmware_revision(hardware_id: &str) -> Option<u32> {
    let upper = hardware_id.to_ascii_uppercase();
    let start = upper.rfind(REVISION_SUFFIX)? + REVISION_SUFFIX.len();
    let digits = upper[start..].trim();
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// Render a firmware version the way the platform documents it: decimal with
/// the hexadecimal form the hardware id carries.
#[must_use]
pub fn format_firmware_version(version: u32) -> String {
    format!("{version} (0x{version:X})")
}

fn is_guid_body(body: &str) -> bool {
    let bytes = body.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_is_read_from_the_documented_suffix() {
        assert_eq!(
            firmware_revision("UEFI\\RES_{8163650e-c940-3926-0703-fcf1f743d4e2}&REV_71250022"),
            Some(1_898_250_274)
        );
        assert_eq!(
            firmware_revision("UEFI\\RES_{64EC2CA6-4AE3-4A6E-B542-27A7E0AA0BA6}&REV_0"),
            Some(0)
        );
        assert_eq!(
            firmware_revision("UEFI\\RES_{8163650E-C940-3926-0703-FCF1F743D4E2}"),
            None
        );
        assert_eq!(firmware_revision("UEFI\\RES_{x}&REV_"), None);
        assert_eq!(firmware_revision("UEFI\\RES_{x}&REV_123456789"), None);
        assert_eq!(firmware_revision("UEFI\\RES_{x}&REV_ZZ"), None);
    }

    #[test]
    fn firmware_class_normalises_both_forms() {
        assert_eq!(
            firmware_class("UEFI\\RES_{499371bf-8950-4b3c-81b6-2c97525241d7}&REV_D033710")
                .as_deref(),
            Some("{499371BF-8950-4B3C-81B6-2C97525241D7}")
        );
        assert_eq!(
            firmware_class("{499371BF-8950-4b3c-81B6-2C97525241D7}").as_deref(),
            Some("{499371BF-8950-4B3C-81B6-2C97525241D7}")
        );
        assert_eq!(firmware_class("UEFI\\RES_{not-a-guid}"), None);
        assert_eq!(firmware_class("ACPI_HAL\\UEFI\\0"), None);
    }

    #[test]
    fn only_documented_attempt_statuses_are_named() {
        assert_eq!(last_attempt_status_meaning(0), Some("success"));
        assert_eq!(last_attempt_status_meaning(0xFFFF_FFFF), Some("no attempt"));
        for status in [1, 2, 0x1000, 0x4000, 0xDEAD_BEEF] {
            assert_eq!(last_attempt_status_meaning(status), None);
        }
    }

    #[test]
    fn identity_versions_need_both_halves() {
        let identity = FirmwareIdentity {
            firmware_type: FirmwareType::Uefi,
            vendor: Some("LENOVO".into()),
            version: Some("SWCN22WW".into()),
            release_date: None,
            bios_release_major: Some(1),
            bios_release_minor: Some(22),
            ec_firmware_major: Some(1),
            ec_firmware_minor: None,
            smbios_major: Some(3),
            smbios_minor: Some(8),
            version_strings: Vec::new(),
        };
        assert_eq!(identity.bios_release().as_deref(), Some("1.22"));
        assert_eq!(identity.ec_firmware(), None);
        assert_eq!(identity.smbios_version().as_deref(), Some("3.8"));
        assert_eq!(format_firmware_version(0x7125_0022), "1898250274 (0x71250022)");
    }

    #[test]
    fn reported_version_prefers_the_documented_derivation() {
        let mut resource = FirmwareResource {
            instance_id: "UEFI\\RES_{8163650E-C940-3926-0703-FCF1F743D4E2}\\0".into(),
            firmware_class: "{8163650E-C940-3926-0703-FCF1F743D4E2}".into(),
            current_version: Some(1_898_250_274),
            stored_version: Some(1_898_250_274),
            description: None,
            status: None,
            hardware_ids: Vec::new(),
            compatible_ids: Vec::new(),
            issues: Vec::new(),
        };
        assert_eq!(resource.reported_version(), Some(1_898_250_274));
        resource.current_version = None;
        assert_eq!(resource.reported_version(), Some(1_898_250_274));
    }
}
