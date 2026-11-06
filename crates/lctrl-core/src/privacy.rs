use serde::Serialize;

use crate::DeviceState;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyDevice {
    Camera,
    Microphone,
    Fingerprint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLayer {
    Runtime,
    Persistent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PrivacyState {
    pub device: PrivacyDevice,
    pub runtime: Option<bool>,
    pub persistent: Option<bool>,
}

impl PrivacyState {
    #[must_use]
    pub const fn layers_disagree(&self) -> bool {
        match (self.runtime, self.persistent) {
            (Some(rt), Some(pers)) => rt != pers,
            _ => false,
        }
    }
}

/// One admitted runtime devnode of a privacy device, as observed without
/// writing.
///
/// `state` is `None` when the platform reports a status that is neither
/// started nor software-disabled (for example a failed start). `error` carries
/// that observation so a split or broken group stays visible per devnode
/// instead of being collapsed into a fabricated state or failing the whole
/// read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PrivacyDeviceState {
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<DeviceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The caveat every camera consent report carries.
///
/// The Capability Access Manager registry stores last-use stamps; a stamp pair
/// with a later start than stop is a record of a session Windows noticed, not
/// an observation of the device. Neither an entry nor a `Deny` here proves the
/// current state of any camera.
pub const CAMERA_CONSENT_AS_RECORDED: &str =
    "as recorded by CapabilityAccessManager; last-use stamps are not proof that a camera is streaming";

/// The rule [`camera_in_use`] applies, printed with the report so the computed
/// flag never appears without its definition.
pub const CAMERA_IN_USE_RULE: &str = "LastUsedTimeStart > LastUsedTimeStop, both non-zero";

/// One consent container under `CapabilityAccessManager\ConsentStore\webcam`.
///
/// The containers stay apart instead of being folded into one verdict: a
/// machine-wide record and the invoking user's record are independent writes,
/// and the operating system resolves them with a policy this read does not
/// evaluate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraConsentScope {
    /// `HKCU\…\ConsentStore\webcam`.
    CurrentUser,
    /// `HKCU\…\ConsentStore\webcam\NonPackaged`, whose children are encoded
    /// executable paths.
    CurrentUserNonPackaged,
    /// `HKLM\…\ConsentStore\webcam`.
    Machine,
    /// `HKLM\…\ConsentStore\webcam\NonPackaged`.
    MachineNonPackaged,
}

impl CameraConsentScope {
    /// Whether this container stores `#`-encoded executable paths rather than
    /// package family names.
    #[must_use]
    pub const fn non_packaged(self) -> bool {
        matches!(self, Self::CurrentUserNonPackaged | Self::MachineNonPackaged)
    }
}

/// The three consent texts Windows writes to a capability record's `Value`.
///
/// These are the store's value domain; the reference machine's records
/// exercised only `Allow`, so `Deny` and `PromptOnFirstUse` are accepted from
/// that domain rather than demonstrated by the probe in
/// `docs/evidence/re-camera-inuse-20261106.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraConsent {
    Allow,
    Deny,
    PromptOnFirstUse,
}

impl CameraConsent {
    /// Parse the stored text. Only the three exact spellings are recognised
    /// (surrounding whitespace ignored); anything else returns `None` and the
    /// caller keeps the raw text, so no unknown value becomes a verdict.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "Allow" => Some(Self::Allow),
            "Deny" => Some(Self::Deny),
            "PromptOnFirstUse" => Some(Self::PromptOnFirstUse),
            _ => None,
        }
    }
}

/// One Windows FILETIME stamp (100 ns ticks since 1601-01-01 UTC) with its UTC
/// rendering.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CameraStamp {
    pub filetime: u64,
    /// `None` when the ticks precede the Unix epoch (a corrupt or sentinel
    /// value); the raw ticks stay reported either way.
    pub utc: Option<String>,
}

impl CameraStamp {
    /// `None` for the zero value Windows writes to mean "no stamp".
    #[must_use]
    pub fn new(filetime: u64) -> Option<Self> {
        if filetime == 0 {
            return None;
        }
        Some(Self {
            filetime,
            utc: filetime_to_rfc3339(filetime),
        })
    }
}

/// Windows FILETIME → RFC 3339 UTC at the stamp's own 100 ns precision.
///
/// `None` for zero and for stamps before 1970-01-01. Howard Hinnant's
/// `civil_from_days`, the same calendar arithmetic `lctrl-hal-win`'s update
/// ledger uses, shifted from the .NET 0001 epoch to the FILETIME 1601 epoch.
#[must_use]
pub fn filetime_to_rfc3339(filetime: u64) -> Option<String> {
    const TICKS_PER_SECOND: u64 = 10_000_000;
    // Seconds from 1601-01-01 to 1970-01-01.
    const UNIX_EPOCH_FILETIME_SECONDS: u64 = 11_644_473_600;
    if filetime == 0 {
        return None;
    }
    let seconds = filetime / TICKS_PER_SECOND;
    let fraction = filetime % TICKS_PER_SECOND;
    let unix = seconds.checked_sub(UNIX_EPOCH_FILETIME_SECONDS)?;
    let days = i64::try_from(unix / 86_400).ok()?;
    let (year, month, day) = civil_from_days(days);
    let rem = unix % 86_400;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{fraction:07}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    ))
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → proleptic
/// Gregorian date, without a calendar dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Decode a `NonPackaged` subkey name into the executable path it stores.
///
/// Windows replaces every `\` of the path with `#`, so `C:#Program Files#a.exe`
/// is `C:\Program Files\a.exe` and `##srv#share#a.exe` is `\\srv\share\a.exe`.
/// Only names shaped like an encoded absolute path are decoded; anything else
/// returns `None` so the caller can report the stored name verbatim instead of
/// inventing a path. A `#` that belonged to a directory or file name is
/// indistinguishable from an encoded separator, which is another reason the
/// stored key is always reported alongside the decoded path.
#[must_use]
pub fn decode_non_packaged_path(key: &str) -> Option<String> {
    let (prefix, rest) = if let Some(rest) = key.strip_prefix("##") {
        ("\\\\".to_string(), rest)
    } else {
        let mut chars = key.chars();
        let drive = chars.next()?;
        if !drive.is_ascii_alphabetic() || chars.next()? != ':' || chars.next()? != '#' {
            return None;
        }
        (format!("{drive}:\\"), chars.as_str())
    };
    if rest.is_empty() {
        return None;
    }
    Some(format!("{prefix}{}", rest.replace('#', "\\")))
}

/// The in-use rule: both stamps present and non-zero, and the start later than
/// the stop.
#[must_use]
pub fn camera_in_use(start: Option<u64>, stop: Option<u64>) -> bool {
    match (start, stop) {
        (Some(start), Some(stop)) => start != 0 && stop != 0 && start > stop,
        _ => false,
    }
}

/// One per-application camera record as stored, without interpretation.
///
/// `key` is the subkey name exactly as written. `path` is the decoded
/// executable path of a `NonPackaged` record and stays `None` when the name is
/// not a decodable encoded path. An absent `consent` means the record carried
/// no `Value`; it is never inherited from the container.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CameraConsentEntry {
    pub scope: CameraConsentScope,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent: Option<CameraConsent>,
    /// The stored consent text when it is not one of the three documented
    /// values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_set: Option<CameraStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_start: Option<CameraStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_stop: Option<CameraStamp>,
    /// [`camera_in_use`] applied to the two stamps above.
    pub in_use: bool,
}

/// Split a stored `Value` text into the recognised consent and, when the text
/// is not one of the three documented spellings, the raw text to report.
///
/// An absent or empty `Value` is neither consent nor an unrecognised value: it
/// stays `(None, None)`, because Windows records "no consent text here" and an
/// inheritance this reader does not evaluate.
#[must_use]
pub fn parse_camera_consent(raw: Option<&str>) -> (Option<CameraConsent>, Option<String>) {
    match raw.map(str::trim) {
        None | Some("") => (None, None),
        Some(text) => match CameraConsent::parse(text) {
            Some(value) => (Some(value), None),
            None => (None, Some(text.to_string())),
        },
    }
}

impl CameraConsentEntry {
    /// Map the raw registry values of one record.
    #[must_use]
    pub fn new(
        scope: CameraConsentScope,
        key: &str,
        consent_raw: Option<&str>,
        last_set: Option<u64>,
        last_used_start: Option<u64>,
        last_used_stop: Option<u64>,
    ) -> Self {
        let (consent, consent_raw) = parse_camera_consent(consent_raw);
        Self {
            scope,
            key: key.to_string(),
            path: if scope.non_packaged() {
                decode_non_packaged_path(key)
            } else {
                None
            },
            consent,
            consent_raw,
            last_set: last_set.and_then(CameraStamp::new),
            last_used_start: last_used_start.and_then(CameraStamp::new),
            last_used_stop: last_used_stop.and_then(CameraStamp::new),
            in_use: camera_in_use(last_used_start, last_used_stop),
        }
    }
}

/// One consent container: its own record plus the per-application records below
/// it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CameraConsentScopeReport {
    pub scope: CameraConsentScope,
    /// The container key existed.
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent: Option<CameraConsent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_set: Option<CameraStamp>,
    pub entries: Vec<CameraConsentEntry>,
    /// Records this read could not open or decode. Reported rather than
    /// silently dropped.
    pub notes: Vec<String>,
}

/// The camera-relevant half of the documented Privacy Policy CSP
/// (`LetAppsAccessCamera`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAppPrivacyPolicy {
    /// `0`: the user decides.
    UserInControl,
    /// `1`: force allow.
    ForceAllow,
    /// `2`: force deny.
    ForceDeny,
}

impl CameraAppPrivacyPolicy {
    /// Documented `LetAppsAccessCamera` values; unknown values stay raw in
    /// [`CameraPrivacyPolicy::let_apps_access_camera_value`].
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::UserInControl),
            1 => Some(Self::ForceAllow),
            2 => Some(Self::ForceDeny),
            _ => None,
        }
    }
}

/// Policy state, kept apart from the consent store it does not describe.
///
/// `present` is whether `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy`
/// exists; an absent key is reported as absent instead of being read as "user
/// in control".
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CameraPrivacyPolicy {
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub let_apps_access_camera: Option<CameraAppPrivacyPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub let_apps_access_camera_value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_allow: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_deny: Option<Vec<String>>,
    /// Policy values that exist but could not be read as declared. Reported
    /// rather than turning the whole store read into a failure.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Everything `privacy camera inuse` reports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CameraConsentReport {
    pub as_recorded: &'static str,
    pub in_use_rule: &'static str,
    pub scopes: Vec<CameraConsentScopeReport>,
    pub policy: CameraPrivacyPolicy,
}

impl CameraConsentReport {
    #[must_use]
    pub fn new(scopes: Vec<CameraConsentScopeReport>, policy: CameraPrivacyPolicy) -> Self {
        Self {
            as_recorded: CAMERA_CONSENT_AS_RECORDED,
            in_use_rule: CAMERA_IN_USE_RULE,
            scopes,
            policy,
        }
    }

    /// Every record whose stamps satisfy the in-use rule, in report order.
    pub fn in_use(&self) -> impl Iterator<Item = &CameraConsentEntry> {
        self.scopes
            .iter()
            .flat_map(|scope| scope.entries.iter())
            .filter(|entry| entry.in_use)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_decodes_to_the_utc_stamp_it_encodes() {
        // Live values read from the reference machine's ConsentStore.
        assert_eq!(
            filetime_to_rfc3339(0x1dd2e18bfdecedf).as_deref(),
            Some("2026-08-17T07:19:32.4125919Z")
        );
        assert_eq!(
            filetime_to_rfc3339(0x1dd2e18c89f9036).as_deref(),
            Some("2026-08-17T07:19:47.0976054Z")
        );
        assert_eq!(
            filetime_to_rfc3339(0x1dd6084b41a8ceb).as_deref(),
            Some("2026-10-20T11:18:16.4571371Z")
        );
        // The Unix epoch itself.
        assert_eq!(
            filetime_to_rfc3339(11_644_473_600 * 10_000_000).as_deref(),
            Some("1970-01-01T00:00:00.0000000Z")
        );
        // Zero means "no stamp"; a pre-1970 stamp cannot be rendered.
        assert_eq!(filetime_to_rfc3339(0), None);
        assert_eq!(filetime_to_rfc3339(1), None);
        assert_eq!(CameraStamp::new(0), None);
    }

    #[test]
    fn non_packaged_names_decode_only_when_they_are_encoded_absolute_paths() {
        assert_eq!(
            decode_non_packaged_path("C:#Program Files#Google#Chrome#Application#chrome.exe")
                .as_deref(),
            Some("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe")
        );
        assert_eq!(
            decode_non_packaged_path("C:#Windows#System32#svchost.exe").as_deref(),
            Some("C:\\Windows\\System32\\svchost.exe")
        );
        assert_eq!(
            decode_non_packaged_path("##srv#share#app.exe").as_deref(),
            Some("\\\\srv\\share\\app.exe")
        );
        // Not an encoded absolute path: the stored name is reported raw.
        assert_eq!(decode_non_packaged_path("app.exe"), None);
        assert_eq!(decode_non_packaged_path("#Program Files#app.exe"), None);
        assert_eq!(decode_non_packaged_path("C:#"), None);
        assert_eq!(decode_non_packaged_path("##"), None);
    }

    #[test]
    fn consent_parses_only_the_three_documented_spellings() {
        assert_eq!(CameraConsent::parse("Allow"), Some(CameraConsent::Allow));
        assert_eq!(CameraConsent::parse("Deny"), Some(CameraConsent::Deny));
        assert_eq!(
            CameraConsent::parse(" PromptOnFirstUse\n"),
            Some(CameraConsent::PromptOnFirstUse)
        );
        // A different case is not coerced into a verdict.
        assert_eq!(CameraConsent::parse("allow"), None);
        assert_eq!(CameraConsent::parse(""), None);
    }

    #[test]
    fn in_use_requires_two_non_zero_stamps_with_start_after_stop() {
        assert!(camera_in_use(Some(20), Some(10)));
        assert!(!camera_in_use(Some(10), Some(20)));
        assert!(!camera_in_use(Some(10), Some(10)));
        // No recorded stop: the rule says not in use, and the raw stamp still
        // appears in the entry.
        assert!(!camera_in_use(Some(10), Some(0)));
        assert!(!camera_in_use(Some(0), Some(0)));
        assert!(!camera_in_use(None, Some(10)));
        assert!(!camera_in_use(Some(10), None));
        assert!(!camera_in_use(None, None));
    }

    #[test]
    fn entries_keep_unknown_consent_and_undecodable_names() {
        let entry = CameraConsentEntry::new(
            CameraConsentScope::CurrentUserNonPackaged,
            "#odd#name.exe",
            Some("Blocked"),
            Some(0),
            Some(20),
            Some(30),
        );
        assert_eq!(entry.key, "#odd#name.exe");
        assert_eq!(entry.path, None);
        assert_eq!(entry.consent, None);
        assert_eq!(entry.consent_raw.as_deref(), Some("Blocked"));
        assert_eq!(entry.last_set, None);
        assert_eq!(
            entry.last_used_start.as_ref().map(|stamp| stamp.filetime),
            Some(20)
        );
        assert!(!entry.in_use);

        let recorded_without_consent = CameraConsentEntry::new(
            CameraConsentScope::CurrentUser,
            "Microsoft.BioEnrollment_cw5n1h2txyewy",
            None,
            None,
            Some(0x1dd1_4228_dc83_4fe),
            Some(0x1dd1_4229_3f72_a14),
        );
        assert_eq!(recorded_without_consent.consent, None);
        assert_eq!(recorded_without_consent.consent_raw, None);
        assert_eq!(recorded_without_consent.path, None);
        assert!(!recorded_without_consent.in_use);
    }

    #[test]
    fn policy_values_map_only_documented_numbers() {
        assert_eq!(
            CameraAppPrivacyPolicy::from_raw(0),
            Some(CameraAppPrivacyPolicy::UserInControl)
        );
        assert_eq!(
            CameraAppPrivacyPolicy::from_raw(1),
            Some(CameraAppPrivacyPolicy::ForceAllow)
        );
        assert_eq!(
            CameraAppPrivacyPolicy::from_raw(2),
            Some(CameraAppPrivacyPolicy::ForceDeny)
        );
        assert_eq!(CameraAppPrivacyPolicy::from_raw(3), None);
        assert_eq!(CameraAppPrivacyPolicy::from_raw(u32::MAX), None);
        assert!(!CameraPrivacyPolicy::default().present);
    }

    #[test]
    fn report_keeps_scopes_separate_and_counts_in_use_records() {
        let report = CameraConsentReport::new(
            vec![
                CameraConsentScopeReport {
                    scope: CameraConsentScope::CurrentUser,
                    present: true,
                    consent: Some(CameraConsent::Allow),
                    consent_raw: None,
                    last_set: None,
                    entries: vec![CameraConsentEntry::new(
                        CameraConsentScope::CurrentUser,
                        "app_a",
                        Some("Allow"),
                        None,
                        Some(20),
                        Some(10),
                    )],
                    notes: Vec::new(),
                },
                CameraConsentScopeReport {
                    scope: CameraConsentScope::Machine,
                    present: true,
                    consent: Some(CameraConsent::Deny),
                    consent_raw: None,
                    last_set: None,
                    entries: Vec::new(),
                    notes: Vec::new(),
                },
            ],
            CameraPrivacyPolicy::default(),
        );
        assert_eq!(report.scopes[0].consent, Some(CameraConsent::Allow));
        assert_eq!(report.scopes[1].consent, Some(CameraConsent::Deny));
        assert_eq!(report.in_use().count(), 1);
        assert!(!report.policy.present);
        assert!(
            report
                .as_recorded
                .starts_with("as recorded by CapabilityAccessManager")
        );
        assert_eq!(report.in_use_rule, CAMERA_IN_USE_RULE);
    }
}
