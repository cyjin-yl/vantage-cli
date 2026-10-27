use lctrl_core::{LctrlError, Result};
use serde::Serialize;

/// A hard upper bound for process-local Wi-Fi observations.
///
/// History is deliberately small: it is a diagnostic aid, not a location or
/// network-identity tracking database.
pub const MAX_WIFI_HISTORY_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurityFinding {
    Safe,
    Legacy,
    Weak,
    Open,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurityReason {
    OpenNetwork,
    WepEncryption,
    SharedKeyAuthentication,
    TkipEncryption,
    WpaEncryption,
    Wpa2Ccmp,
    Wpa3Ccmp,
    Wpa3Gcmp,
    Wpa3Enterprise192Gcmp256,
    MissingAuthentication,
    MissingCipher,
    UnrecognizedAuthentication,
    UnrecognizedCipher,
    UnsupportedCombination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WifiSecurityAssessment {
    pub finding: WifiSecurityFinding,
    pub reason: WifiSecurityReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiConnectionState {
    Connected,
    Disconnected,
    /// A transitional or unrecognized state. The original netsh state text is
    /// retained separately so this classification never invents a state.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiLiveNetwork {
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub profile_name: Option<String>,
    pub authentication: Option<String>,
    pub cipher: Option<String>,
    pub signal_percent: Option<u8>,
    pub channel: Option<u32>,
    pub assessment: WifiSecurityAssessment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiInterfaceSnapshot {
    pub observed_at_unix_ms: u64,
    pub interface_name: String,
    pub description: Option<String>,
    /// Raw state text from the operating system, when present.
    pub state: Option<String>,
    pub connection_state: WifiConnectionState,
    /// Present only for an interface whose live state is connected. Saved
    /// profiles are never promoted into this field.
    pub live_network: Option<WifiLiveNetwork>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiSavedProfile {
    pub name: String,
    pub interface_name: Option<String>,
    /// Raw profile scope text (for example, `All User Profile`).
    pub profile_type: String,
    pub authentication: Option<String>,
    pub cipher: Option<String>,
    /// Assessment of the saved configuration, not evidence of a live
    /// connection or of its current signal/channel.
    pub assessment: WifiSecurityAssessment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiSecurityReport {
    pub interfaces: Vec<WifiInterfaceSnapshot>,
    pub saved_profiles: Vec<WifiSavedProfile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiHistoryPersistence {
    /// No file, registry, daemon, or operating-system persistence is used.
    MemoryOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WifiHistoryPolicy {
    pub capacity: usize,
    /// SSIDs are omitted by default. BSSID and profile-name identifiers are
    /// always omitted from history, even when this opt-in is enabled.
    pub retain_ssid: bool,
    pub persistence: WifiHistoryPersistence,
}

impl WifiHistoryPolicy {
    pub fn new(capacity: usize, retain_ssid: bool) -> Result<Self> {
        let policy = Self {
            capacity,
            retain_ssid,
            persistence: WifiHistoryPersistence::MemoryOnly,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<()> {
        if self.capacity == 0 || self.capacity > MAX_WIFI_HISTORY_ENTRIES {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "Wi-Fi history capacity must be between 1 and {MAX_WIFI_HISTORY_ENTRIES}"
                ),
            });
        }
        Ok(())
    }
}

impl Default for WifiHistoryPolicy {
    fn default() -> Self {
        Self {
            capacity: 32,
            retain_ssid: false,
            persistence: WifiHistoryPersistence::MemoryOnly,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiHistorySnapshot {
    pub policy: WifiHistoryPolicy,
    /// Oldest to newest. Entries are observations of interfaces only; saved
    /// profile enumeration never contributes history entries.
    pub entries: Vec<WifiInterfaceSnapshot>,
}

/// Public service contract consumed by the CLI on either platform.
pub trait WifiSecurityControl: Send + Sync {
    fn wifi_security(&self) -> Result<WifiSecurityReport>;
    fn wifi_security_history(&self) -> Result<WifiHistorySnapshot>;
    fn clear_wifi_security_history(&self) -> Result<()>;
}

/// Classifies only explicitly recognized authentication/cipher combinations.
/// Unknown values remain unknown and are preserved by the caller in raw form.
#[must_use]
pub fn assess_wifi_security(
    authentication: Option<&str>,
    cipher: Option<&str>,
) -> WifiSecurityAssessment {
    let Some(authentication) = non_empty(authentication) else {
        return assessment(
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::MissingAuthentication,
        );
    };
    let Some(cipher) = non_empty(cipher) else {
        return assessment(
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::MissingCipher,
        );
    };

    let cipher_kind = classify_cipher(cipher);
    // These ciphers are weak regardless of an unusual or unrecognized outer
    // authentication label. In particular, WPA/WPA2 with TKIP is never safe.
    match cipher_kind {
        CipherKind::Wep => {
            return assessment(WifiSecurityFinding::Weak, WifiSecurityReason::WepEncryption);
        }
        CipherKind::Tkip => {
            return assessment(
                WifiSecurityFinding::Weak,
                WifiSecurityReason::TkipEncryption,
            );
        }
        _ => {}
    }

    let authentication_kind = classify_authentication(authentication);
    if authentication_kind == AuthenticationKind::Unknown {
        return assessment(
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnrecognizedAuthentication,
        );
    }
    if cipher_kind == CipherKind::Unknown {
        return assessment(
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnrecognizedCipher,
        );
    }

    match (authentication_kind, cipher_kind) {
        (AuthenticationKind::Open, CipherKind::None) => {
            assessment(WifiSecurityFinding::Open, WifiSecurityReason::OpenNetwork)
        }
        (AuthenticationKind::Shared, _) => assessment(
            WifiSecurityFinding::Weak,
            WifiSecurityReason::SharedKeyAuthentication,
        ),
        (AuthenticationKind::Wpa, CipherKind::Ccmp | CipherKind::Gcmp | CipherKind::Gcmp256) => {
            assessment(
                WifiSecurityFinding::Legacy,
                WifiSecurityReason::WpaEncryption,
            )
        }
        (AuthenticationKind::Wpa2, CipherKind::Ccmp) => {
            assessment(WifiSecurityFinding::Safe, WifiSecurityReason::Wpa2Ccmp)
        }
        (AuthenticationKind::Wpa3, CipherKind::Ccmp) => {
            assessment(WifiSecurityFinding::Safe, WifiSecurityReason::Wpa3Ccmp)
        }
        (AuthenticationKind::Wpa3, CipherKind::Gcmp | CipherKind::Gcmp256) => {
            assessment(WifiSecurityFinding::Safe, WifiSecurityReason::Wpa3Gcmp)
        }
        (AuthenticationKind::Wpa3Enterprise192, CipherKind::Gcmp256) => assessment(
            WifiSecurityFinding::Safe,
            WifiSecurityReason::Wpa3Enterprise192Gcmp256,
        ),
        _ => assessment(
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnsupportedCombination,
        ),
    }
}

const fn assessment(
    finding: WifiSecurityFinding,
    reason: WifiSecurityReason,
) -> WifiSecurityAssessment {
    WifiSecurityAssessment { finding, reason }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AuthenticationKind {
    Open,
    Shared,
    Wpa,
    Wpa2,
    Wpa3,
    Wpa3Enterprise192,
    Unknown,
}

fn classify_authentication(value: &str) -> AuthenticationKind {
    if normalized_eq_any(value, &["open", "开放"]) {
        AuthenticationKind::Open
    } else if normalized_eq_any(value, &["shared", "共享"]) {
        AuthenticationKind::Shared
    } else if normalized_eq_any(
        value,
        &[
            "wpa3enterprise192bit",
            "wpa3enterprise192",
            "wpa3企业192bit",
        ],
    ) {
        AuthenticationKind::Wpa3Enterprise192
    } else if normalized_eq_any(
        value,
        &[
            "wpa3personal",
            "wpa3enterprise",
            "wpa3sae",
            "wpa3个人",
            "wpa3企业",
        ],
    ) {
        AuthenticationKind::Wpa3
    } else if normalized_eq_any(
        value,
        &["wpa2personal", "wpa2enterprise", "wpa2个人", "wpa2企业"],
    ) {
        AuthenticationKind::Wpa2
    } else if normalized_eq_any(
        value,
        &["wpapersonal", "wpaenterprise", "wpa个人", "wpa企业"],
    ) {
        AuthenticationKind::Wpa
    } else {
        AuthenticationKind::Unknown
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CipherKind {
    None,
    Wep,
    Tkip,
    Ccmp,
    Gcmp,
    Gcmp256,
    Unknown,
}

fn classify_cipher(value: &str) -> CipherKind {
    if normalized_eq_any(value, &["none", "无"]) {
        CipherKind::None
    } else if normalized_eq_any(value, &["wep", "wep40", "wep104", "wep128"]) {
        CipherKind::Wep
    } else if normalized_eq(value, "tkip") {
        CipherKind::Tkip
    } else if normalized_eq_any(value, &["ccmp", "ccmp128", "aes", "aesccmp"]) {
        CipherKind::Ccmp
    } else if normalized_eq_any(value, &["gcmp", "gcmp128"]) {
        CipherKind::Gcmp
    } else if normalized_eq(value, "gcmp256") {
        CipherKind::Gcmp256
    } else {
        CipherKind::Unknown
    }
}

fn normalized_eq_any(value: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| normalized_eq(value, candidate))
}

/// Compares netsh tokens without allocating a normalized copy. Separators and
/// ASCII case are presentation details; all other characters remain significant.
fn normalized_eq(value: &str, candidate: &str) -> bool {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-' && *character != '_')
        .map(|character| character.to_ascii_lowercase())
        .eq(candidate.chars())
}
