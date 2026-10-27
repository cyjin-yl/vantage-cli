use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use lctrl_core::{LctrlError, Result};
use lctrl_hal::{
    MAX_WIFI_HISTORY_ENTRIES, WifiConnectionState, WifiHistoryPersistence, WifiHistoryPolicy,
    WifiSecurityFinding, WifiSecurityReason, assess_wifi_security,
};
use lctrl_hal_win::{WindowsWifiSecurity, WlanNetshRunner};

const CONNECTED_INTERFACES: &str = r#"
There are 2 interfaces on the system:

    Name                   : Wi-Fi
    Description            : Intel(R) Wi-Fi 7
    GUID                   : 11111111-1111-1111-1111-111111111111
    Physical address       : 00:11:22:33:44:55
    Interface type         : Primary
    State                  : connected
    SSID                   : Coffee Shop 5G
    BSSID                  : 02:00:00:00:00:01
    Network type           : Infrastructure
    Radio type             : 802.11be
    Authentication         : WPA2-Personal
    Cipher                 : CCMP
    Connection mode        : Auto
    Channel                : 36
    Receive rate (Mbps)    : 1201
    Transmit rate (Mbps)   : 1201
    Signal                 : 87%
    Profile                : Coffee Shop 5G

    Name                   : Wi-Fi 2
    Description            : Second wireless adapter
    GUID                   : 22222222-2222-2222-2222-222222222222
    Physical address       : 00:11:22:33:44:66
    Interface type         : Primary
    State                  : disconnected
"#;

const SINGLE_CONNECTED_INTERFACE: &str = r#"
There is 1 interface on the system:

    Name                   : Wi-Fi
    Description            : Intel(R) Wi-Fi 7
    State                  : connected
    SSID                   : SensitiveSSID
    BSSID                  : 02:00:00:00:00:99
    Authentication         : WPA2-Personal
    Cipher                 : AES
    Channel                : 11
    Signal                 : 73 %
    Profile                : SensitiveSSID
"#;

const PROFILE_LIST: &str = r#"
Profiles on interface Wi-Fi:

Group policy profiles (read only)
---------------------------------
    <None>

User profiles
-------------
    All User Profile     : Coffee Shop 5G
    Current User Profile : Legacy Office
    All User Profile     : Mystery

Profiles on interface Wi-Fi 2:

Group policy profiles (read only)
---------------------------------
    <None>

User profiles
-------------
    All User Profile     : Disconnected Only
"#;

const EMPTY_PROFILES: &str = r#"
Profiles on interface Wi-Fi:

Group policy profiles (read only)
---------------------------------
    <None>

User profiles
-------------
    <None>
"#;

#[derive(Clone, Default)]
struct FakeNetsh {
    interfaces: Arc<String>,
    profiles: Arc<String>,
    details: Arc<BTreeMap<String, String>>,
    queries: Arc<Mutex<Vec<String>>>,
}

impl FakeNetsh {
    fn new(interfaces: &str, profiles: &str) -> Self {
        Self {
            interfaces: Arc::new(interfaces.to_owned()),
            profiles: Arc::new(profiles.to_owned()),
            ..Self::default()
        }
    }

    fn with_details<const N: usize>(self, details: [(&str, &str); N]) -> Self {
        let details = details
            .into_iter()
            .map(|(name, detail)| (name.to_owned(), detail.to_owned()))
            .collect();
        Self {
            details: Arc::new(details),
            ..self
        }
    }
}

impl WlanNetshRunner for FakeNetsh {
    fn show_interfaces(&self) -> Result<String> {
        self.queries.lock().unwrap().push("interfaces".into());
        Ok((*self.interfaces).clone())
    }

    fn show_profiles(&self) -> Result<String> {
        self.queries.lock().unwrap().push("profiles".into());
        Ok((*self.profiles).clone())
    }

    fn show_profile(&self, name: &str, interface: Option<&str>) -> Result<String> {
        self.queries
            .lock()
            .unwrap()
            .push(format!("profile:{name}:{}", interface.unwrap_or("")));
        self.details
            .get(name)
            .cloned()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("missing fake profile {name}"),
            })
    }
}

#[test]
fn reads_live_interfaces_separately_from_disconnected_saved_profiles() {
    let runner = FakeNetsh::new(CONNECTED_INTERFACES, PROFILE_LIST).with_details([
        (
            "Coffee Shop 5G",
            r#"
Profile Coffee Shop 5G on interface Wi-Fi:

Profile information
-------------------
    Version                : 1
    Type                   : Wireless LAN
    Name                   : Coffee Shop 5G

Security settings
-----------------
    Authentication         : WPA2-Personal
    Cipher                 : CCMP
    Security key           : Present
"#,
        ),
        (
            "Legacy Office",
            r#"
Profile information
-------------------
    Name                   : Legacy Office
Security settings
-----------------
    Authentication         : WPA-Personal
    Cipher                 : TKIP
    Security key           : Present
"#,
        ),
        (
            "Mystery",
            r#"
Profile information
-------------------
    Name                   : Mystery
Security settings
-----------------
    Authentication         : WPA2-Personal
    Cipher                 : ROT13
    Security key           : Present
"#,
        ),
        (
            "Disconnected Only",
            r#"
Profile information
-------------------
    Name                   : Disconnected Only
    SSID name              : "Disconnected Only"
Security settings
-----------------
    Authentication         : Open
    Cipher                 : None
    Security key           : Absent
"#,
        ),
    ]);
    let service = WindowsWifiSecurity::new(runner);

    let report = service.assess().unwrap();
    assert_eq!(report.interfaces.len(), 2);
    let connected = &report.interfaces[0];
    assert_eq!(connected.interface_name, "Wi-Fi");
    assert_eq!(connected.state.as_deref(), Some("connected"));
    assert_eq!(connected.connection_state, WifiConnectionState::Connected);
    let live = connected.live_network.as_ref().unwrap();
    assert_eq!(live.ssid.as_deref(), Some("Coffee Shop 5G"));
    assert_eq!(live.bssid.as_deref(), Some("02:00:00:00:00:01"));
    assert_eq!(live.authentication.as_deref(), Some("WPA2-Personal"));
    assert_eq!(live.cipher.as_deref(), Some("CCMP"));
    assert_eq!(live.signal_percent, Some(87));
    assert_eq!(live.channel, Some(36));
    assert_eq!(live.assessment.finding, WifiSecurityFinding::Safe);

    let disconnected = &report.interfaces[1];
    assert_eq!(
        disconnected.connection_state,
        WifiConnectionState::Disconnected
    );
    assert!(disconnected.live_network.is_none());

    assert_eq!(report.saved_profiles.len(), 4);
    let saved_disconnected = report
        .saved_profiles
        .iter()
        .find(|profile| profile.name == "Disconnected Only")
        .unwrap();
    assert_eq!(
        saved_disconnected.interface_name.as_deref(),
        Some("Wi-Fi 2")
    );
    assert_eq!(
        saved_disconnected.assessment.finding,
        WifiSecurityFinding::Open
    );
    assert!(!report.interfaces.iter().any(|interface| {
        interface
            .live_network
            .as_ref()
            .and_then(|live| live.ssid.as_deref())
            == Some("Disconnected Only")
    }));
    let legacy = report
        .saved_profiles
        .iter()
        .find(|profile| profile.name == "Legacy Office")
        .unwrap();
    assert_eq!(legacy.assessment.finding, WifiSecurityFinding::Weak);
    let mystery = report
        .saved_profiles
        .iter()
        .find(|profile| profile.name == "Mystery")
        .unwrap();
    assert_eq!(mystery.assessment.finding, WifiSecurityFinding::Unknown);
    assert_eq!(mystery.cipher.as_deref(), Some("ROT13"));

    // One observation per interface, never one per saved profile.
    assert_eq!(service.history().entries.len(), 2);
}

#[test]
fn parses_localized_netsh_fields_and_wpa3_values() {
    let interfaces = r#"
系统上有 1 个接口:

    名称                   : WLAN
    描述                   : Intel(R) Wi-Fi 7
    状态                   : 已连接
    SSID                   : 家庭网络
    BSSID                  : 02:00:00:00:00:08
    身份验证               : WPA3 - 个人
    密码                   : GCMP-256
    通道                   : 149
    信号                   : 95 %
    配置文件               : 家庭网络
"#;
    let profiles = r#"
接口 WLAN 上的配置文件:

组策略配置文件(只读)
--------------------
    <无>

用户配置文件
------------
    <无>
"#;
    let service = WindowsWifiSecurity::new(FakeNetsh::new(interfaces, profiles));

    let report = service.assess().unwrap();
    assert!(report.saved_profiles.is_empty());
    let interface = &report.interfaces[0];
    assert_eq!(interface.interface_name, "WLAN");
    assert_eq!(interface.connection_state, WifiConnectionState::Connected);
    let live = interface.live_network.as_ref().unwrap();
    assert_eq!(live.ssid.as_deref(), Some("家庭网络"));
    assert_eq!(live.authentication.as_deref(), Some("WPA3 - 个人"));
    assert_eq!(live.cipher.as_deref(), Some("GCMP-256"));
    assert_eq!(live.channel, Some(149));
    assert_eq!(live.signal_percent, Some(95));
    assert_eq!(live.assessment.finding, WifiSecurityFinding::Safe);
}

#[test]
fn security_mapping_is_conservative_for_legacy_and_unknown_values() {
    let cases = [
        (
            Some("Open"),
            Some("None"),
            WifiSecurityFinding::Open,
            WifiSecurityReason::OpenNetwork,
        ),
        (
            Some("Open"),
            Some("WEP"),
            WifiSecurityFinding::Weak,
            WifiSecurityReason::WepEncryption,
        ),
        (
            Some("Shared"),
            Some("WEP-104"),
            WifiSecurityFinding::Weak,
            WifiSecurityReason::WepEncryption,
        ),
        (
            Some("WPA-Personal"),
            Some("TKIP"),
            WifiSecurityFinding::Weak,
            WifiSecurityReason::TkipEncryption,
        ),
        (
            Some("WPA2-Personal"),
            Some("TKIP"),
            WifiSecurityFinding::Weak,
            WifiSecurityReason::TkipEncryption,
        ),
        (
            Some("WPA-Enterprise"),
            Some("AES"),
            WifiSecurityFinding::Legacy,
            WifiSecurityReason::WpaEncryption,
        ),
        (
            Some("WPA2 - Personal"),
            Some("AES"),
            WifiSecurityFinding::Safe,
            WifiSecurityReason::Wpa2Ccmp,
        ),
        (
            Some("WPA3-SAE"),
            Some("CCMP"),
            WifiSecurityFinding::Safe,
            WifiSecurityReason::Wpa3Ccmp,
        ),
        (
            Some("WPA3-Enterprise 192-bit"),
            Some("GCMP-256"),
            WifiSecurityFinding::Safe,
            WifiSecurityReason::Wpa3Enterprise192Gcmp256,
        ),
        (
            Some("WPA3-Enterprise 192-bit"),
            Some("CCMP"),
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnsupportedCombination,
        ),
        (
            Some("WPA2-Personal"),
            Some("ROT13"),
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnrecognizedCipher,
        ),
        (
            Some("Future-Security"),
            Some("CCMP"),
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::UnrecognizedAuthentication,
        ),
        (
            None,
            Some("CCMP"),
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::MissingAuthentication,
        ),
        (
            Some("WPA2-Personal"),
            None,
            WifiSecurityFinding::Unknown,
            WifiSecurityReason::MissingCipher,
        ),
    ];

    for (authentication, cipher, finding, reason) in cases {
        let assessment = assess_wifi_security(authentication, cipher);
        assert_eq!(
            assessment.finding, finding,
            "authentication={authentication:?}, cipher={cipher:?}"
        );
        assert_eq!(
            assessment.reason, reason,
            "authentication={authentication:?}, cipher={cipher:?}"
        );
    }
}

#[test]
fn profile_keys_and_key_content_never_enter_output_or_queries() {
    const SECRET: &str = "do-not-ever-serialize-this-key";
    let disconnected = r#"
There is 1 interface on the system:
    Name                   : Wi-Fi
    State                  : disconnected
"#;
    let profiles = r#"
Profiles on interface Wi-Fi:
User profiles
-------------
    All User Profile     : Keyless View
"#;
    let detail = format!(
        r#"
Profile information
-------------------
    Name                   : Keyless View
    SSID name              : "Keyless View"
Security settings
-----------------
    Authentication         : WPA2-Personal
    Cipher                 : CCMP
    Security key           : Present
    Key Content            : {SECRET}
"#
    );
    let runner =
        FakeNetsh::new(disconnected, profiles).with_details([("Keyless View", detail.as_str())]);
    let queries = Arc::clone(&runner.queries);
    let service = WindowsWifiSecurity::new(runner);

    let report = service.assess().unwrap();
    assert_eq!(
        report.saved_profiles[0].assessment.finding,
        WifiSecurityFinding::Safe
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains(SECRET));
    assert!(!json.contains("Key Content"));
    assert!(!json.contains("key=clear"));
    let queries = queries.lock().unwrap();
    assert!(queries.iter().any(|query| query.starts_with("profile:")));
    assert!(queries.iter().all(|query| !query.contains("key=clear")));
}

#[test]
fn history_is_bounded_memory_only_and_redacts_identifiers_by_default() {
    let policy = WifiHistoryPolicy::new(2, false).unwrap();
    let service = WindowsWifiSecurity::with_history_policy(
        FakeNetsh::new(SINGLE_CONNECTED_INTERFACE, EMPTY_PROFILES),
        policy,
    )
    .unwrap();

    for _ in 0..3 {
        service.assess().unwrap();
    }
    let history = service.history();
    assert_eq!(history.policy.capacity, 2);
    assert_eq!(
        history.policy.persistence,
        WifiHistoryPersistence::MemoryOnly
    );
    assert_eq!(history.entries.len(), 2);
    for entry in &history.entries {
        assert!(entry.observed_at_unix_ms > 0);
        let live = entry.live_network.as_ref().unwrap();
        assert!(live.ssid.is_none());
        assert!(live.bssid.is_none());
        assert!(live.profile_name.is_none());
        assert_eq!(live.signal_percent, Some(73));
        assert_eq!(live.channel, Some(11));
    }
    let json = serde_json::to_string(&history).unwrap();
    assert!(!json.contains("SensitiveSSID"));
    assert!(!json.contains("02:00:00:00:00:99"));

    service.clear_history();
    assert!(service.history().entries.is_empty());

    let opted_in = WindowsWifiSecurity::with_history_policy(
        FakeNetsh::new(SINGLE_CONNECTED_INTERFACE, EMPTY_PROFILES),
        WifiHistoryPolicy::new(1, true).unwrap(),
    )
    .unwrap();
    opted_in.assess().unwrap();
    let opted_in_history = opted_in.history();
    let live = opted_in_history.entries[0].live_network.as_ref().unwrap();
    assert_eq!(live.ssid.as_deref(), Some("SensitiveSSID"));
    assert!(live.bssid.is_none());
    assert!(live.profile_name.is_none());

    assert!(WifiHistoryPolicy::new(0, false).is_err());
    assert!(WifiHistoryPolicy::new(MAX_WIFI_HISTORY_ENTRIES + 1, false).is_err());
}
