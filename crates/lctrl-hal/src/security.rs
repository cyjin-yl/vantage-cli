//! Read-only security posture: Secure Boot, TPM, and BitLocker.
//!
//! The contract is deliberately narrower than "security centre": it reports the
//! state the operating system itself can observe from three documented sources,
//! names the source behind every value, and never defaults an unreadable
//! component. No firmware variable is written, no TPM operation is issued, and
//! no BitLocker key, recovery password, or key protector is read.
//!
//! See `docs/evidence/re-security-20261105.md` for the recovered owner binaries,
//! the live receipts, and the value maps taken from the on-disk provider MOFs.

use lctrl_core::{FirmwareType, Result};
use serde::Serialize;

/// The exact read-only source that produced one observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecuritySource {
    /// `kernel32!GetFirmwareType`: boot mode only (UEFI or legacy BIOS).
    FirmwareType,
    /// `kernel32!GetFirmwareEnvironmentVariableExW` on a UEFI variable.
    UefiFirmwareVariable,
    /// `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State!UEFISecureBootEnabled`.
    SecureBootStateRegistry,
    /// `root\CIMV2\Security\MicrosoftTpm` : `Win32_Tpm` (property query only).
    Win32Tpm,
    /// `root\CIMV2\Security\MicrosoftVolumeEncryption` : `Win32_EncryptableVolume`.
    Win32EncryptableVolume,
}

impl SecuritySource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirmwareType => "get_firmware_type",
            Self::UefiFirmwareVariable => "uefi_firmware_variable",
            Self::SecureBootStateRegistry => "secure_boot_state_registry",
            Self::Win32Tpm => "win32_tpm",
            Self::Win32EncryptableVolume => "win32_encryptable_volume",
        }
    }
}

/// Secure Boot posture.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureBootState {
    /// UEFI boot and the `SecureBoot` variable is `1`.
    Enabled,
    /// UEFI boot and the `SecureBoot` variable is `0`.
    Disabled,
    /// The machine boots legacy BIOS, where Secure Boot does not exist.
    NotUefi,
    /// The firmware mode or the `SecureBoot` variable could not be read; the
    /// reason is carried in [`SecureBootReport::detail`] instead of a guess.
    Unavailable,
}

impl SecureBootState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::NotUefi => "not_uefi",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Secure Boot state plus the reads that corroborate it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SecureBootReport {
    pub state: SecureBootState,
    pub firmware_type: FirmwareType,
    /// The read that produced `state`.
    pub source: SecuritySource,
    /// UEFI `SecureBoot` (`{8be4df61-93ca-11d2-aa0d-00e098032b8c}`), 1 byte:
    /// `1` enabled, `0` disabled.
    pub secure_boot_variable: Option<bool>,
    /// UEFI `SetupMode`: `true` while the platform runs without enrolled keys.
    pub setup_mode: Option<bool>,
    /// Whether the `PK` platform key is enrolled. Its bytes are never retained.
    pub platform_key_present: Option<bool>,
    /// Length of the `PK` variable; presence evidence only, never the bytes.
    pub platform_key_bytes: Option<usize>,
    /// The operating system's own mirror of the firmware state.
    pub registry_secure_boot_enabled: Option<bool>,
    /// Why `state` is unavailable, and why any corroborating read stayed unknown.
    pub detail: Option<String>,
}

impl SecureBootReport {
    /// A report whose primary read failed.
    #[must_use]
    pub fn unavailable(
        source: SecuritySource,
        firmware_type: FirmwareType,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            state: SecureBootState::Unavailable,
            firmware_type,
            source,
            secure_boot_variable: None,
            setup_mode: None,
            platform_key_present: None,
            platform_key_bytes: None,
            registry_secure_boot_enabled: None,
            detail: Some(detail.into()),
        }
    }

    /// A machine that does not boot UEFI, so the variable set does not exist.
    #[must_use]
    pub fn not_uefi() -> Self {
        Self {
            state: SecureBootState::NotUefi,
            firmware_type: FirmwareType::Bios,
            source: SecuritySource::FirmwareType,
            secure_boot_variable: None,
            setup_mode: None,
            platform_key_present: None,
            platform_key_bytes: None,
            registry_secure_boot_enabled: None,
            detail: Some(
                "the machine boots legacy BIOS: the UEFI SecureBoot variable does not exist".into(),
            ),
        }
    }
}

/// TPM availability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TpmState {
    /// `Win32_Tpm` answered with one instance.
    Present,
    /// `Win32_Tpm` answered successfully with no instance.
    Absent,
    /// The namespace, class, or query could not be read; see `detail`.
    Unavailable,
}

impl TpmState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unavailable => "unavailable",
        }
    }
}

/// TPM identity and state from `Win32_Tpm` properties.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TpmReport {
    pub state: TpmState,
    pub source: SecuritySource,
    pub enabled: Option<bool>,
    pub activated: Option<bool>,
    pub owned: Option<bool>,
    /// `SpecVersion`, e.g. `2.0, 0, 1.59`.
    pub spec_version: Option<String>,
    pub manufacturer_id: Option<u32>,
    pub manufacturer_id_text: Option<String>,
    pub manufacturer_version: Option<String>,
    pub manufacturer_version_full: Option<String>,
    pub manufacturer_version_info: Option<String>,
    pub physical_presence_version: Option<String>,
    pub detail: Option<String>,
}

impl TpmReport {
    /// `Win32_Tpm` answered successfully with no instance.
    #[must_use]
    pub fn absent() -> Self {
        Self {
            state: TpmState::Absent,
            source: SecuritySource::Win32Tpm,
            enabled: None,
            activated: None,
            owned: None,
            spec_version: None,
            manufacturer_id: None,
            manufacturer_id_text: None,
            manufacturer_version: None,
            manufacturer_version_full: None,
            manufacturer_version_info: None,
            physical_presence_version: None,
            detail: Some(
                "the TPM provider answered with no Win32_Tpm instance: the firmware exposes no TPM"
                    .into(),
            ),
        }
    }

    #[must_use]
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            state: TpmState::Unavailable,
            source: SecuritySource::Win32Tpm,
            enabled: None,
            activated: None,
            owned: None,
            spec_version: None,
            manufacturer_id: None,
            manufacturer_id_text: None,
            manufacturer_version: None,
            manufacturer_version_full: None,
            manufacturer_version_info: None,
            physical_presence_version: None,
            detail: Some(detail.into()),
        }
    }
}

/// BitLocker observation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerState {
    /// The provider answered; `volumes` may legitimately be empty.
    Observed,
    /// The namespace, class, or query could not be read; see `detail`.
    Unavailable,
}

impl BitLockerState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `Win32_EncryptableVolume.ProtectionStatus` value map
/// (`GetProtectionStatus` out-parameter, provider MOF).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerProtectionStatus {
    Unprotected,
    Protected,
    Unknown,
}

impl BitLockerProtectionStatus {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Unprotected,
            1 => Self::Protected,
            2 => Self::Unknown,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unprotected => "unprotected",
            Self::Protected => "protected",
            Self::Unknown => "unknown",
        }
    }
}

/// `Win32_EncryptableVolume.ConversionStatus` value map
/// (`GetConversionStatus` out-parameter, provider MOF).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerConversionStatus {
    FullyDecrypted,
    FullyEncrypted,
    EncryptionInProgress,
    DecryptionInProgress,
    EncryptionPaused,
    DecryptionPaused,
}

impl BitLockerConversionStatus {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::FullyDecrypted,
            1 => Self::FullyEncrypted,
            2 => Self::EncryptionInProgress,
            3 => Self::DecryptionInProgress,
            4 => Self::EncryptionPaused,
            5 => Self::DecryptionPaused,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullyDecrypted => "fully_decrypted",
            Self::FullyEncrypted => "fully_encrypted",
            Self::EncryptionInProgress => "encryption_in_progress",
            Self::DecryptionInProgress => "decryption_in_progress",
            Self::EncryptionPaused => "encryption_paused",
            Self::DecryptionPaused => "decryption_paused",
        }
    }
}

/// `Win32_EncryptableVolume.EncryptionMethod` value map
/// (`GetEncryptionMethod` out-parameter, provider MOF).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerEncryptionMethod {
    None,
    Aes128WithDiffuser,
    Aes256WithDiffuser,
    Aes128,
    Aes256,
    HardwareEncryption,
    XtsAes128,
    XtsAes256,
}

impl BitLockerEncryptionMethod {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::Aes128WithDiffuser,
            2 => Self::Aes256WithDiffuser,
            3 => Self::Aes128,
            4 => Self::Aes256,
            5 => Self::HardwareEncryption,
            6 => Self::XtsAes128,
            7 => Self::XtsAes256,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Aes128WithDiffuser => "aes_128_with_diffuser",
            Self::Aes256WithDiffuser => "aes_256_with_diffuser",
            Self::Aes128 => "aes_128",
            Self::Aes256 => "aes_256",
            Self::HardwareEncryption => "hardware_encryption",
            Self::XtsAes128 => "xts_aes_128",
            Self::XtsAes256 => "xts_aes_256",
        }
    }
}

/// `Win32_EncryptableVolume.VolumeType` value map (declared on the property).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerVolumeType {
    OsVolume,
    FixedDataVolume,
    PortableDataVolume,
}

impl BitLockerVolumeType {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::OsVolume,
            1 => Self::FixedDataVolume,
            2 => Self::PortableDataVolume,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OsVolume => "os_volume",
            Self::FixedDataVolume => "fixed_data_volume",
            Self::PortableDataVolume => "portable_data_volume",
        }
    }
}

/// `GetLockStatus` value map.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BitLockerLockStatus {
    Unlocked,
    Locked,
}

impl BitLockerLockStatus {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Unlocked,
            1 => Self::Locked,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unlocked => "unlocked",
            Self::Locked => "locked",
        }
    }
}

/// One `Win32_EncryptableVolume` row plus the read-only getter results.
///
/// Every typed field keeps its raw value; a value the provider did not return,
/// or one outside the recovered map, leaves the typed field `None` rather than
/// defaulting to "decrypted"/"unlocked".
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BitLockerVolume {
    /// The volume's `DeviceID` (its WMI key); `None` when the row omitted it.
    pub device_id: Option<String>,
    pub drive_letter: Option<String>,
    pub persistent_volume_id: Option<String>,
    pub protection_status: Option<BitLockerProtectionStatus>,
    pub protection_status_raw: Option<u32>,
    pub conversion_status: Option<BitLockerConversionStatus>,
    pub conversion_status_raw: Option<u32>,
    pub encryption_method: Option<BitLockerEncryptionMethod>,
    pub encryption_method_raw: Option<u32>,
    pub volume_type: Option<BitLockerVolumeType>,
    pub volume_type_raw: Option<u32>,
    pub initialized_for_protection: Option<bool>,
    /// `GetLockStatus`; `None` when the getter did not answer.
    pub lock_status: Option<BitLockerLockStatus>,
    pub lock_status_raw: Option<u32>,
    /// `GetConversionStatus` percentage; `None` when the getter did not answer.
    pub encryption_percentage: Option<u32>,
    /// Which read-only getters failed for this volume, and why.
    pub detail: Option<String>,
}

/// BitLocker observation over every encryptable volume the provider enumerates.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BitLockerReport {
    pub state: BitLockerState,
    pub source: SecuritySource,
    pub volumes: Vec<BitLockerVolume>,
    pub detail: Option<String>,
}

impl BitLockerReport {
    #[must_use]
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            state: BitLockerState::Unavailable,
            source: SecuritySource::Win32EncryptableVolume,
            volumes: Vec::new(),
            detail: Some(detail.into()),
        }
    }
}

/// How much of the requested posture could actually be observed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityAvailability {
    /// All three components answered.
    Available,
    /// Some components answered and at least one is unavailable.
    Limited,
    /// No component answered; nothing here may be reported as a state.
    Unavailable,
}

/// The three independent security postures.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SecurityStatusReport {
    pub secure_boot: SecureBootReport,
    pub tpm: TpmReport,
    pub bitlocker: BitLockerReport,
}

impl SecurityStatusReport {
    #[must_use]
    pub fn availability(&self) -> SecurityAvailability {
        let observed = [
            self.secure_boot.state != SecureBootState::Unavailable,
            self.tpm.state != TpmState::Unavailable,
            self.bitlocker.state == BitLockerState::Observed,
        ]
        .into_iter()
        .filter(|observed| *observed)
        .count();
        match observed {
            3 => SecurityAvailability::Available,
            0 => SecurityAvailability::Unavailable,
            _ => SecurityAvailability::Limited,
        }
    }

    /// One-line component summary, independent of the human renderer.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "secure boot: {}; tpm: {}; bitlocker: {}",
            self.secure_boot.state.as_str(),
            self.tpm.state.as_str(),
            self.bitlocker.state.as_str()
        )
    }

    /// Every recorded reason, prefixed with its component.
    #[must_use]
    pub fn details(&self) -> Vec<String> {
        let mut details = Vec::new();
        for (component, detail) in [
            ("secure boot", &self.secure_boot.detail),
            ("tpm", &self.tpm.detail),
            ("bitlocker", &self.bitlocker.detail),
        ] {
            if let Some(detail) = detail {
                details.push(format!("{component}: {detail}"));
            }
        }
        details
    }
}

/// Bound on how many encryptable volumes one read interrogates, so a machine
/// with many volumes cannot turn one command into an unbounded sequence of
/// provider calls. Truncation is reported, never silent.
pub const MAX_BITLOCKER_VOLUMES: usize = 32;

/// Read-only security posture service.
pub trait SecurityControl: Send + Sync {
    /// Reports Secure Boot, TPM, and BitLocker state from their read-only
    /// sources.
    ///
    /// A component that could not be read is reported as unavailable with its
    /// reason; the call itself does not fail and no component is defaulted.
    fn security_status(&self) -> Result<SecurityStatusReport>;
}
