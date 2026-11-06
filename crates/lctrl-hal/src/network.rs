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

/// The radio band a live WLAN association occupies.
///
/// The band is derived from the reported **center frequency**, never from the
/// channel number: 6 GHz reuses the 2.4 GHz channel numbers, so on a
/// 6 GHz-capable adapter a channel number alone cannot name a band.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum WifiBand {
    #[serde(rename = "2.4ghz")]
    Ghz2,
    #[serde(rename = "5ghz")]
    Ghz5,
    #[serde(rename = "6ghz")]
    Ghz6,
    #[serde(rename = "60ghz")]
    Ghz60,
    /// A center frequency outside every band this contract names.
    Unknown,
}

impl WifiBand {
    /// Classifies a center frequency in kHz, the unit
    /// `WLAN_BSS_ENTRY::ulChCenterFrequency` reports.
    ///
    /// The ranges are the ITU band edges of the WLAN bands; a frequency that
    /// falls between two of them stays [`WifiBand::Unknown`] rather than being
    /// rounded into a neighbour.
    #[must_use]
    pub const fn from_center_frequency_khz(khz: u32) -> Self {
        match khz {
            2_400_000..=2_500_000 => Self::Ghz2,
            5_150_000..=5_895_000 => Self::Ghz5,
            5_925_000..=7_125_000 => Self::Ghz6,
            57_000_000..=71_000_000 => Self::Ghz60,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ghz2 => "2.4ghz",
            Self::Ghz5 => "5ghz",
            Self::Ghz6 => "6ghz",
            Self::Ghz60 => "60ghz",
            Self::Unknown => "unknown",
        }
    }
}

/// Everything one WLAN interface reports about its live link.
///
/// Every field the operating system did not answer stays `None`. A connected
/// interface with a hidden SSID reports `ssid == None` next to
/// `connection_state == Connected`; absence is never promoted into a value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiLinkStatus {
    /// Textual interface GUID from the WLAN service (not a secret).
    pub interface_guid: String,
    pub interface_description: Option<String>,
    /// Raw `WLAN_INTERFACE_STATE` value, retained so an unnamed state stays
    /// distinguishable from a state this contract names.
    pub interface_state_raw: i32,
    /// Canonical name of [`Self::interface_state_raw`] when it is a documented
    /// value (`not_ready`, `connected`, `associating`, …).
    pub interface_state: Option<String>,
    pub connection_state: WifiConnectionState,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub profile_name: Option<String>,
    /// Canonical name of the `DOT11_PHY_TYPE` the association uses.
    pub phy_type: Option<String>,
    pub phy_type_raw: Option<i32>,
    pub channel: Option<u32>,
    pub band: Option<WifiBand>,
    /// `WLAN_BSS_ENTRY::ulChCenterFrequency`, in kHz.
    pub center_frequency_khz: Option<u32>,
    /// `WLAN_ASSOCIATION_ATTRIBUTES::wlanSignalQuality`, a percentage.
    pub signal_percent: Option<u8>,
    /// `WLAN_BSS_ENTRY::lRssi`, in dBm.
    pub rssi_dbm: Option<i32>,
    /// Receive rate in kbit/s, the unit the WLAN service reports.
    pub rx_rate_kbps: Option<u32>,
    /// Transmit rate in kbit/s.
    pub tx_rate_kbps: Option<u32>,
    pub authentication: Option<String>,
    pub authentication_raw: Option<i32>,
    pub cipher: Option<String>,
    pub cipher_raw: Option<i32>,
    pub security_enabled: Option<bool>,
    pub one_x_enabled: Option<bool>,
    /// Assessment of the live association, classified by
    /// [`assess_wifi_security`] from the same names reported above. `None` when
    /// the interface has no live association to assess.
    pub assessment: Option<WifiSecurityAssessment>,
}

/// The read-only answer of `wifi status`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WifiStatusReport {
    pub observed_at_unix_ms: u64,
    pub interfaces: Vec<WifiLinkStatus>,
}

/// Public service contract consumed by the CLI on either platform.
///
/// Implementations answer from the operating system's current state only: they
/// never trigger a scan, never set interface parameters, and never connect or
/// disconnect. A scan can drop the association that carries a remote session,
/// so it is not part of this verb.
pub trait WifiStatusControl: Send + Sync {
    fn wifi_status(&self) -> Result<WifiStatusReport>;
}

/// Names a `WLAN_INTERFACE_STATE` value.
#[must_use]
pub const fn wlan_interface_state_name(value: i32) -> Option<&'static str> {
    match value {
        0 => Some("not_ready"),
        1 => Some("connected"),
        2 => Some("ad_hoc_network_formed"),
        3 => Some("disconnecting"),
        4 => Some("disconnected"),
        5 => Some("associating"),
        6 => Some("discovering"),
        7 => Some("authenticating"),
        _ => None,
    }
}

/// Classifies a `WLAN_INTERFACE_STATE` value into the coarse link state.
///
/// Transitional states (`associating`, `discovering`, `authenticating`,
/// `not_ready`, `disconnecting`) are [`WifiConnectionState::Unknown`]: they are
/// neither a live link nor an idle one, and the raw value is preserved beside
/// this classification.
#[must_use]
pub const fn classify_wlan_interface_state(value: i32) -> WifiConnectionState {
    match value {
        1 => WifiConnectionState::Connected,
        4 => WifiConnectionState::Disconnected,
        _ => WifiConnectionState::Unknown,
    }
}

/// Names a `DOT11_AUTH_ALGORITHM` value (`wlantypes.h`).
///
/// The names below are the ones the operating system's own presentation uses
/// (`Open`, `Shared`, `WPA2-Enterprise`, …), so [`assess_wifi_security`]
/// classifies a value read from the WLAN service exactly as it classifies the
/// same network read from netsh. IHV-defined values stay unnamed.
#[must_use]
pub const fn dot11_authentication_name(value: i32) -> Option<&'static str> {
    match value {
        1 => Some("Open"),
        2 => Some("Shared"),
        3 => Some("WPA-Enterprise"),
        4 => Some("WPA-Personal"),
        5 => Some("WPA-None"),
        6 => Some("WPA2-Enterprise"),
        7 => Some("WPA2-Personal"),
        // wlantypes.h: "DOT11_AUTH_ALGO_WPA3 = 8 — means WPA3 Enterprise 192 bits".
        8 => Some("WPA3-Enterprise 192-bit"),
        9 => Some("WPA3-SAE"),
        10 => Some("OWE"),
        11 => Some("WPA3-Enterprise"),
        _ => None,
    }
}

/// Names a `DOT11_CIPHER_ALGORITHM` value (`wlantypes.h`).
#[must_use]
pub const fn dot11_cipher_name(value: i32) -> Option<&'static str> {
    match value {
        0x00 => Some("None"),
        0x01 => Some("WEP40"),
        0x02 => Some("TKIP"),
        0x04 => Some("CCMP"),
        0x05 => Some("WEP104"),
        0x06 => Some("BIP"),
        0x08 => Some("GCMP"),
        0x09 => Some("GCMP256"),
        0x0a => Some("CCMP256"),
        0x0b => Some("BIP-GMAC-128"),
        0x0c => Some("BIP-GMAC-256"),
        0x0d => Some("BIP-CMAC-256"),
        0x100 => Some("Use group"),
        0x101 => Some("WEP"),
        _ => None,
    }
}

/// Names a `DOT11_PHY_TYPE` value (`windot11.h`).
#[must_use]
pub const fn dot11_phy_type_name(value: i32) -> Option<&'static str> {
    match value {
        0 => Some("unknown"),
        1 => Some("fhss"),
        2 => Some("dsss"),
        3 => Some("irbaseband"),
        4 => Some("802.11a"),
        5 => Some("802.11b"),
        6 => Some("802.11g"),
        7 => Some("802.11n"),
        8 => Some("802.11ac"),
        9 => Some("802.11ad"),
        10 => Some("802.11ax"),
        11 => Some("802.11be"),
        _ => None,
    }
}

/// Decodes a service-set identifier.
///
/// An SSID is a byte string, not guaranteed UTF-8; invalid sequences are
/// replaced rather than dropped. An empty SSID (a hidden network) is `None`.
#[must_use]
pub fn decode_ssid(bytes: &[u8]) -> Option<String> {
    let trimmed = bytes
        .iter()
        .position(|byte| *byte == 0)
        .map_or(bytes, |end| &bytes[..end]);
    if trimmed.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(trimmed).into_owned())
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn band_comes_from_center_frequency_not_channel_numbers() {
        assert_eq!(WifiBand::from_center_frequency_khz(2_437_000), WifiBand::Ghz2);
        assert_eq!(WifiBand::from_center_frequency_khz(5_180_000), WifiBand::Ghz5);
        assert_eq!(WifiBand::from_center_frequency_khz(5_955_000), WifiBand::Ghz6);
        assert_eq!(
            WifiBand::from_center_frequency_khz(60_480_000),
            WifiBand::Ghz60
        );
        // Between the 5 GHz and 6 GHz ranges: no band is invented.
        assert_eq!(
            WifiBand::from_center_frequency_khz(5_900_000),
            WifiBand::Unknown
        );
        assert_eq!(WifiBand::from_center_frequency_khz(0), WifiBand::Unknown);
    }

    #[test]
    fn only_connected_and_disconnected_states_are_classified() {
        assert_eq!(
            classify_wlan_interface_state(1),
            WifiConnectionState::Connected
        );
        assert_eq!(
            classify_wlan_interface_state(4),
            WifiConnectionState::Disconnected
        );
        for transitional in [0, 2, 3, 5, 6, 7, 99] {
            assert_eq!(
                classify_wlan_interface_state(transitional),
                WifiConnectionState::Unknown
            );
        }
        assert_eq!(wlan_interface_state_name(5), Some("associating"));
        assert_eq!(wlan_interface_state_name(99), None);
    }

    #[test]
    fn live_enum_values_match_the_header_table() {
        // Values from docs/evidence/re-network-20261105.md §2 (wlantypes.h) and
        // the association observed in §3.5 (auth 6 + cipher 4 = WPA2-Enterprise/CCMP).
        assert_eq!(dot11_authentication_name(6), Some("WPA2-Enterprise"));
        assert_eq!(dot11_authentication_name(7), Some("WPA2-Personal"));
        assert_eq!(dot11_cipher_name(4), Some("CCMP"));
        assert_eq!(dot11_cipher_name(0x101), Some("WEP"));
        assert_eq!(dot11_authentication_name(0x8000_0000_u32 as i32), None);
        assert_eq!(dot11_phy_type_name(8), Some("802.11ac"));
        assert_eq!(
            assess_wifi_security(Some("WPA2-Enterprise"), Some("CCMP")),
            WifiSecurityAssessment {
                finding: WifiSecurityFinding::Safe,
                reason: WifiSecurityReason::Wpa2Ccmp,
            }
        );
        assert_eq!(
            assess_wifi_security(Some("Open"), Some("None")),
            WifiSecurityAssessment {
                finding: WifiSecurityFinding::Open,
                reason: WifiSecurityReason::OpenNetwork,
            }
        );
    }

    #[test]
    fn ssid_decoding_keeps_bytes_and_reports_hidden_networks() {
        assert_eq!(decode_ssid(b"BUAA-Mobile"), Some("BUAA-Mobile".into()));
        assert_eq!(decode_ssid(b"BUAA-Mobile\0\0\0"), Some("BUAA-Mobile".into()));
        assert_eq!(decode_ssid(b""), None);
        assert_eq!(decode_ssid(&[0, 0, 0]), None);
        // A non-UTF-8 SSID is replaced, not dropped.
        assert_eq!(decode_ssid(&[0xff, 0xfe]), Some("\u{fffd}\u{fffd}".into()));
    }
}
