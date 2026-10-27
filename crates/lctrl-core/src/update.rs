use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf, Prefix},
};

use serde::{Deserialize, Serialize};

use crate::{
    change::ApplyMode,
    error::{LctrlError, Result},
};

pub const LOCAL_UPDATE_POLICY_SCHEMA: u32 = 1;
pub const MAX_LOCAL_UPDATE_HISTORY: usize = 256;
pub const MAX_LOCAL_UPDATE_IGNORES: usize = 256;
pub const MAX_LOCAL_UPDATE_POLICY_BYTES: u64 = 1024 * 1024;
pub const MAX_LOCAL_UPDATE_PACKAGE_FILES: usize = 256;
pub const MAX_LOCAL_UPDATE_DEVICES: usize = 4096;
pub const MAX_WINDOWS_DEVICE_ID_UTF16: usize = 200;
pub const FIRMWARE_DEVICE_CLASS_GUID: &str = "{f2e7dd72-6468-4e36-b6f1-6488f42c1b52}";

/// The four 16-bit components used by SetupAPI and driver-registry version
/// values. Keeping the components avoids lossy floating-point or 64-bit JSON
/// version conversions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DriverVersion([u16; 4]);

impl DriverVersion {
    #[must_use]
    pub const fn new(parts: [u16; 4]) -> Self {
        Self(parts)
    }

    #[must_use]
    pub const fn parts(self) -> [u16; 4] {
        self.0
    }

    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self([
            ((raw >> 48) & 0xffff) as u16,
            ((raw >> 32) & 0xffff) as u16,
            ((raw >> 16) & 0xffff) as u16,
            (raw & 0xffff) as u16,
        ])
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        ((self.0[0] as u64) << 48)
            | ((self.0[1] as u64) << 32)
            | ((self.0[2] as u64) << 16)
            | self.0[3] as u64
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut parts = [0u16; 4];
        let mut count = 0usize;
        for component in text.split('.') {
            if count == parts.len()
                || component.is_empty()
                || !component.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid(
                    "driver version must contain four unsigned 16-bit parts",
                ));
            }
            parts[count] = component.parse::<u16>().map_err(|_| {
                invalid("driver version component exceeds the unsigned 16-bit range")
            })?;
            count += 1;
        }
        if count != parts.len() {
            return Err(invalid(
                "driver version must contain four unsigned 16-bit parts",
            ));
        }
        Ok(Self(parts))
    }
}

impl fmt::Display for DriverVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}.{}",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverIdentity {
    pub provider: String,
    pub description: String,
    pub class_guid: String,
    pub driver_version: DriverVersion,
    pub driver_date: String,
    pub matching_device_id: String,
    pub inf_section: String,
}

impl DriverIdentity {
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("driver provider", &self.provider),
            ("driver description", &self.description),
            ("driver class GUID", &self.class_guid),
            ("driver date", &self.driver_date),
            ("driver INF section", &self.inf_section),
        ] {
            require_text(value, 1024, label)?;
        }
        validate_hardware_id(&self.matching_device_id)
    }

    /// Compare the complete identity exposed by the package and the selected
    /// device driver key. Windows identifier casing is insignificant; all
    /// version and identity payloads remain otherwise exact.
    #[must_use]
    pub fn same_driver(&self, other: &Self) -> bool {
        self.provider == other.provider
            && self.description == other.description
            && self.class_guid.eq_ignore_ascii_case(&other.class_guid)
            && self.driver_version == other.driver_version
            && self.driver_date == other.driver_date
            && self
                .matching_device_id
                .eq_ignore_ascii_case(&other.matching_device_id)
            && self.inf_section.eq_ignore_ascii_case(&other.inf_section)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSignature {
    pub signer: String,
    pub signer_version: String,
}

impl DriverSignature {
    pub fn validate(&self) -> Result<()> {
        require_text(&self.signer, 1024, "driver signer")?;
        if self.signer_version.len() > 1024 || self.signer_version.contains('\0') {
            return Err(invalid("driver signer version is invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentPnpDevice {
    pub device_instance_id: String,
    pub hardware_ids: Vec<String>,
    pub class_guid: String,
}

impl PresentPnpDevice {
    pub fn validate(&self) -> Result<()> {
        validate_device_instance_id(&self.device_instance_id)?;
        if self.hardware_ids.is_empty() || self.hardware_ids.len() > 64 {
            return Err(invalid(
                "present PnP device has an invalid hardware-ID count",
            ));
        }
        for hardware_id in &self.hardware_ids {
            validate_hardware_id(hardware_id)?;
        }
        require_text(&self.class_guid, 1024, "device class GUID")
    }

    #[must_use]
    pub fn has_hardware_id(&self, requested: &str) -> bool {
        self.hardware_ids
            .iter()
            .any(|actual| actual.eq_ignore_ascii_case(requested))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedDriverFile {
    pub path: PathBuf,
    pub catalog_path: PathBuf,
    pub signature: DriverSignature,
}

/// Native SetupAPI inspection output. This is deliberately not serializable:
/// source paths and catalog paths must not leak into policy or history JSON.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverPackage {
    pub inf_path: PathBuf,
    pub catalog_path: PathBuf,
    pub signature: DriverSignature,
    pub dependencies: Vec<SignedDriverFile>,
    pub identity: DriverIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InstalledDriver {
    pub identity: DriverIdentity,
    pub published_inf: String,
    pub driver_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDriverUpdateRequest {
    inf_path: PathBuf,
    hardware_id: String,
}

impl LocalDriverUpdateRequest {
    pub fn new(inf_path: impl Into<PathBuf>, hardware_id: impl Into<String>) -> Result<Self> {
        let request = Self {
            inf_path: inf_path.into(),
            hardware_id: hardware_id.into(),
        };
        validate_local_path(&request.inf_path, "inf")?;
        validate_hardware_id(&request.hardware_id)?;
        Ok(request)
    }

    #[must_use]
    pub fn inf_path(&self) -> &Path {
        &self.inf_path
    }

    #[must_use]
    pub fn hardware_id(&self) -> &str {
        &self.hardware_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDriverRollbackRequest {
    device_instance_id: String,
    expected_current: DriverIdentity,
    restore: DriverIdentity,
}

impl LocalDriverRollbackRequest {
    pub fn new(
        device_instance_id: impl Into<String>,
        expected_current: DriverIdentity,
        restore: DriverIdentity,
    ) -> Result<Self> {
        let request = Self {
            device_instance_id: device_instance_id.into(),
            expected_current,
            restore,
        };
        validate_device_instance_id(&request.device_instance_id)?;
        request.expected_current.validate()?;
        request.restore.validate()?;
        if !request
            .expected_current
            .class_guid
            .eq_ignore_ascii_case(&request.restore.class_guid)
            || !request
                .expected_current
                .matching_device_id
                .eq_ignore_ascii_case(&request.restore.matching_device_id)
        {
            return Err(invalid(
                "rollback identities do not bind to the same device class and hardware ID",
            ));
        }
        Ok(request)
    }

    #[must_use]
    pub fn device_instance_id(&self) -> &str {
        &self.device_instance_id
    }

    #[must_use]
    pub const fn expected_current(&self) -> &DriverIdentity {
        &self.expected_current
    }

    #[must_use]
    pub const fn restore(&self) -> &DriverIdentity {
        &self.restore
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DriverUpdateReport {
    pub mode: ApplyMode,
    pub device_instance_id: String,
    pub previous: Option<InstalledDriver>,
    pub requested: DriverIdentity,
    pub actual: Option<InstalledDriver>,
    pub signature: DriverSignature,
    /// `None` during dry-run: no installation was performed, so reboot need is
    /// unknown rather than a fabricated `false`.
    pub reboot_required: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DriverRollbackReport {
    pub mode: ApplyMode,
    pub device_instance_id: String,
    pub previous: InstalledDriver,
    pub requested: DriverIdentity,
    pub actual: Option<InstalledDriver>,
    /// Windows exposes no read-only API that proves the backup driver's
    /// identity. A dry-run therefore never claims that a restore point exists.
    pub backup_identity_verified: bool,
    pub reboot_required: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateHistoryOperation {
    Install,
    Rollback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateHistoryEntry {
    pub id: u64,
    pub recorded_at: String,
    pub operation: UpdateHistoryOperation,
    pub device_instance_id: String,
    pub previous: Option<DriverIdentity>,
    pub requested: DriverIdentity,
    pub actual: DriverIdentity,
    pub signature: DriverSignature,
    pub reboot_required: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnoredDriverRule {
    pub hardware_id: String,
    pub provider: Option<String>,
    pub driver_version: Option<DriverVersion>,
}

impl IgnoredDriverRule {
    pub fn new(
        hardware_id: impl Into<String>,
        provider: Option<String>,
        driver_version: Option<DriverVersion>,
    ) -> Result<Self> {
        let rule = Self {
            hardware_id: hardware_id.into(),
            provider,
            driver_version,
        };
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<()> {
        validate_hardware_id(&self.hardware_id)?;
        if let Some(provider) = &self.provider {
            require_text(provider, 1024, "ignored driver provider")?;
        }
        Ok(())
    }

    #[must_use]
    pub fn matches(&self, hardware_id: &str, identity: &DriverIdentity) -> bool {
        self.hardware_id.eq_ignore_ascii_case(hardware_id)
            && self
                .provider
                .as_ref()
                .is_none_or(|provider| *provider == identity.provider)
            && self
                .driver_version
                .is_none_or(|version| version == identity.driver_version)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateWeekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

/// Advisory scheduling preference only. No scheduler is registered and this
/// value never authorizes an automatic install or reboot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UpdateSchedule {
    Off,
    Daily {
        hour: u8,
        minute: u8,
    },
    Weekly {
        weekday: UpdateWeekday,
        hour: u8,
        minute: u8,
    },
}

impl Default for UpdateSchedule {
    fn default() -> Self {
        Self::Off
    }
}

impl UpdateSchedule {
    pub fn daily(hour: u8, minute: u8) -> Result<Self> {
        let schedule = Self::Daily { hour, minute };
        schedule.validate()?;
        Ok(schedule)
    }

    pub fn weekly(weekday: UpdateWeekday, hour: u8, minute: u8) -> Result<Self> {
        let schedule = Self::Weekly {
            weekday,
            hour,
            minute,
        };
        schedule.validate()?;
        Ok(schedule)
    }

    pub fn validate(self) -> Result<()> {
        let (hour, minute) = match self {
            Self::Off => return Ok(()),
            Self::Daily { hour, minute } | Self::Weekly { hour, minute, .. } => (hour, minute),
        };
        if hour >= 24 || minute >= 60 {
            return Err(invalid("update schedule time is outside the 24-hour clock"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUpdatePolicy {
    pub schema: u32,
    pub history: Vec<UpdateHistoryEntry>,
    pub ignored: Vec<IgnoredDriverRule>,
    pub schedule: UpdateSchedule,
}

impl Default for LocalUpdatePolicy {
    fn default() -> Self {
        Self {
            schema: LOCAL_UPDATE_POLICY_SCHEMA,
            history: Vec::new(),
            ignored: Vec::new(),
            schedule: UpdateSchedule::Off,
        }
    }
}

impl LocalUpdatePolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema != LOCAL_UPDATE_POLICY_SCHEMA {
            return Err(invalid("unsupported local update policy schema"));
        }
        if self.history.len() > MAX_LOCAL_UPDATE_HISTORY
            || self.ignored.len() > MAX_LOCAL_UPDATE_IGNORES
        {
            return Err(invalid("local update policy exceeds its entry bound"));
        }
        let mut ids = BTreeSet::new();
        for entry in &self.history {
            if entry.id == 0 || !ids.insert(entry.id) {
                return Err(invalid(
                    "local update history IDs must be nonzero and unique",
                ));
            }
            require_text(&entry.recorded_at, 128, "history timestamp")?;
            validate_device_instance_id(&entry.device_instance_id)?;
            if let Some(previous) = &entry.previous {
                previous.validate()?;
            }
            entry.requested.validate()?;
            entry.actual.validate()?;
            entry.signature.validate()?;
        }
        for rule in &self.ignored {
            rule.validate()?;
        }
        self.schedule.validate()
    }

    #[must_use]
    pub fn is_ignored(&self, hardware_id: &str, identity: &DriverIdentity) -> bool {
        self.ignored
            .iter()
            .any(|rule| rule.matches(hardware_id, identity))
    }

    fn next_history_id(&self) -> Result<u64> {
        self.history
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("local update history ID space is exhausted"))
    }
}

/// Owner-pathed, local-only JSON persistence. The constructor never discovers
/// a default, consults the environment, creates directories, or appends a file
/// name. Every read and write targets exactly the supplied path.
#[derive(Clone, Debug)]
pub struct LocalUpdatePolicyStore {
    path: PathBuf,
}

impl LocalUpdatePolicyStore {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        validate_local_path(&path, "json")?;
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<LocalUpdatePolicy> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LocalUpdatePolicy::default());
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(
                "local update policy path is not a regular non-symlink file",
            ));
        }
        if metadata.len() > MAX_LOCAL_UPDATE_POLICY_BYTES {
            return Err(invalid("local update policy exceeds its size bound"));
        }
        let mut bytes = Vec::new();
        File::open(&self.path)?
            .take(MAX_LOCAL_UPDATE_POLICY_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_LOCAL_UPDATE_POLICY_BYTES {
            return Err(invalid("local update policy exceeds its size bound"));
        }
        let policy: LocalUpdatePolicy = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(format!(
                "local update policy JSON was rejected at {}:{}: {error}",
                error.line(),
                error.column()
            ))
        })?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn save(&self, policy: &LocalUpdatePolicy) -> Result<()> {
        policy.validate()?;
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(invalid(
                    "local update policy path is not a regular non-symlink file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut bytes = serde_json::to_vec_pretty(policy)
            .map_err(|_| invalid("local update policy could not be serialized"))?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_LOCAL_UPDATE_POLICY_BYTES {
            return Err(invalid("local update policy exceeds its size bound"));
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }

    pub fn record_install(
        &self,
        report: &DriverUpdateReport,
        recorded_at: impl Into<String>,
    ) -> Result<UpdateHistoryEntry> {
        if report.mode != ApplyMode::Commit {
            return Err(invalid(
                "dry-run update reports cannot be recorded as installed",
            ));
        }
        let actual = report
            .actual
            .as_ref()
            .ok_or_else(|| invalid("committed update report has no readback"))?;
        let mut policy = self.load()?;
        let entry = UpdateHistoryEntry {
            id: policy.next_history_id()?,
            recorded_at: recorded_at.into(),
            operation: UpdateHistoryOperation::Install,
            device_instance_id: report.device_instance_id.clone(),
            previous: report
                .previous
                .as_ref()
                .map(|driver| driver.identity.clone()),
            requested: report.requested.clone(),
            actual: actual.identity.clone(),
            signature: report.signature.clone(),
            reboot_required: report.reboot_required,
        };
        policy.history.push(entry.clone());
        if policy.history.len() > MAX_LOCAL_UPDATE_HISTORY {
            policy.history.remove(0);
        }
        self.save(&policy)?;
        Ok(entry)
    }

    pub fn record_rollback(
        &self,
        report: &DriverRollbackReport,
        signature: DriverSignature,
        recorded_at: impl Into<String>,
    ) -> Result<UpdateHistoryEntry> {
        if report.mode != ApplyMode::Commit || !report.backup_identity_verified {
            return Err(invalid(
                "rollback history requires a committed, identity-verified restore",
            ));
        }
        let actual = report
            .actual
            .as_ref()
            .ok_or_else(|| invalid("committed rollback report has no readback"))?;
        let mut policy = self.load()?;
        let entry = UpdateHistoryEntry {
            id: policy.next_history_id()?,
            recorded_at: recorded_at.into(),
            operation: UpdateHistoryOperation::Rollback,
            device_instance_id: report.device_instance_id.clone(),
            previous: Some(report.previous.identity.clone()),
            requested: report.requested.clone(),
            actual: actual.identity.clone(),
            signature,
            reboot_required: report.reboot_required,
        };
        policy.history.push(entry.clone());
        if policy.history.len() > MAX_LOCAL_UPDATE_HISTORY {
            policy.history.remove(0);
        }
        self.save(&policy)?;
        Ok(entry)
    }

    pub fn ignore(&self, rule: IgnoredDriverRule) -> Result<()> {
        rule.validate()?;
        let mut policy = self.load()?;
        if !policy.ignored.contains(&rule) {
            policy.ignored.push(rule);
        }
        self.save(&policy)
    }

    pub fn unignore(&self, rule: &IgnoredDriverRule) -> Result<bool> {
        let mut policy = self.load()?;
        let previous = policy.ignored.len();
        policy.ignored.retain(|candidate| {
            !candidate
                .hardware_id
                .eq_ignore_ascii_case(&rule.hardware_id)
                || candidate.provider != rule.provider
                || candidate.driver_version != rule.driver_version
        });
        let removed = policy.ignored.len() != previous;
        if removed {
            self.save(&policy)?;
        }
        Ok(removed)
    }

    pub fn set_schedule(&self, schedule: UpdateSchedule) -> Result<()> {
        schedule.validate()?;
        let mut policy = self.load()?;
        policy.schedule = schedule;
        self.save(&policy)
    }

    pub fn latest_for_device(
        &self,
        device_instance_id: &str,
    ) -> Result<Option<UpdateHistoryEntry>> {
        validate_device_instance_id(device_instance_id)?;
        Ok(self
            .load()?
            .history
            .into_iter()
            .filter(|entry| {
                entry
                    .device_instance_id
                    .eq_ignore_ascii_case(device_instance_id)
            })
            .max_by_key(|entry| entry.id))
    }
}

pub fn validate_hardware_id(hardware_id: &str) -> Result<()> {
    require_text(hardware_id, MAX_WINDOWS_DEVICE_ID_UTF16, "PnP hardware ID")
}

pub fn validate_device_instance_id(device_instance_id: &str) -> Result<()> {
    require_text(
        device_instance_id,
        MAX_WINDOWS_DEVICE_ID_UTF16,
        "PnP device instance ID",
    )
}

/// Validate lexical locality before any OS call. Native backends additionally
/// canonicalize the path and require a regular file, so resolved UNC/reparse
/// targets are rejected as well.
pub fn validate_local_path(path: &Path, expected_extension: &str) -> Result<()> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(invalid("local update paths must be absolute"));
    }
    let mut windows_disk_prefix = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => windows_disk_prefix = true,
                _ => {
                    return Err(invalid(
                        "remote and device-namespace paths are not admitted",
                    ));
                }
            },
            Component::CurDir | Component::ParentDir => {
                return Err(invalid(
                    "local update paths must not contain traversal components",
                ));
            }
            Component::RootDir | Component::Normal(_) => {}
        }
    }
    if let Some(text) = path.to_str() {
        if text.contains("://")
            || (!windows_disk_prefix && (text.starts_with(r"\\") || text.starts_with("//")))
        {
            return Err(invalid("remote update endpoints are not admitted"));
        }
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("local update path has no valid file extension"))?;
    if !extension.eq_ignore_ascii_case(expected_extension) {
        return Err(invalid(format!(
            "local update path must have the .{expected_extension} extension"
        )));
    }
    if path.file_name().is_none() {
        return Err(invalid("local update path must name a file"));
    }
    Ok(())
}

pub fn is_firmware_class(class_guid: &str) -> bool {
    class_guid.eq_ignore_ascii_case(FIRMWARE_DEVICE_CLASS_GUID)
}

fn require_text(value: &str, maximum_utf16: usize, label: &str) -> Result<()> {
    if value.is_empty()
        || value.trim() != value
        || value.encode_utf16().count() > maximum_utf16
        || value.chars().any(char::is_control)
    {
        return Err(invalid(format!(
            "{label} is empty, too long, or contains control text"
        )));
    }
    Ok(())
}

fn invalid(detail: impl Into<String>) -> LctrlError {
    LctrlError::InvalidArgument {
        detail: detail.into(),
    }
}
