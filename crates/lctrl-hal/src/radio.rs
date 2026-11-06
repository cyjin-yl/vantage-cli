//! Hardware radio (Wi-Fi / Bluetooth / mobile broadband) observation and control.
//!
//! The contract is deliberately narrower than "airplane mode": the operating system exposes
//! per-medium radios, an aggregated per-radio state, and — for Wi-Fi only — the documented
//! per-PHY software and hardware radio switches. A radio this platform does not enumerate is
//! reported as absent, never as "off".
//!
//! See `docs/evidence/re-radio-20261105.md` for the recovered owner (WinRT
//! `Windows.Devices.Radios.Radio` over the operating system's radio-management plane), the live
//! receipts, and the read-back candidates that were tested and rejected.

use std::net::IpAddr;

use lctrl_core::{ApplyMode, LctrlError, Result};
use serde::Serialize;

/// One physical radio medium, as the operating system enumerates it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioKind {
    Wifi,
    Bluetooth,
    MobileBroadband,
    Fm,
    /// A radio medium the platform reports but this contract does not name.
    Other,
}

impl RadioKind {
    /// The kinds a radio command can address by name.
    pub const SELECTABLE: &'static [Self] = &[Self::Wifi, Self::Bluetooth, Self::MobileBroadband];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Bluetooth => "bluetooth",
            Self::MobileBroadband => "wwan",
            Self::Fm => "fm",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "wifi" => Some(Self::Wifi),
            "bluetooth" => Some(Self::Bluetooth),
            "wwan" => Some(Self::MobileBroadband),
            "fm" => Some(Self::Fm),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    /// Whether switching this radio off can plausibly cut an IP session, which is the only
    /// condition under which the session-cut guard applies.
    ///
    /// Bluetooth is deliberately excluded: on this platform its only IP surface is a personal
    /// area network adapter, and blocking the Bluetooth switch on a guarded machine is worse than
    /// the residual risk (documented in `docs/evidence/re-radio-20261105.md` §5).
    #[must_use]
    pub const fn is_session_guardable(self) -> bool {
        matches!(self, Self::Wifi | Self::MobileBroadband)
    }
}

/// The aggregated state of one radio.
///
/// This is the operating system's answer and does not separate a software switch from a
/// hardware gate; only Wi-Fi carries that distinction ([`WlanRadioDetail`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioState {
    On,
    Off,
    /// Turned off below the software switch (hardware switch or system policy).
    Disabled,
    /// The platform reported a state this contract does not name.
    Unknown,
}

impl RadioState {
    /// The state a write can request. `Disabled`/`Unknown` are observations only.
    #[must_use]
    pub const fn requested(self) -> Option<Self> {
        match self {
            Self::On | Self::Off => Some(self),
            Self::Disabled | Self::Unknown => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether this process may change radios at all (`Windows.Devices.Radios.RadioAccessStatus`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioAccess {
    Allowed,
    DeniedByUser,
    DeniedBySystem,
    /// The platform returned a status this contract does not name.
    Unspecified,
}

impl RadioAccess {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::DeniedByUser => "denied_by_user",
            Self::DeniedBySystem => "denied_by_system",
            Self::Unspecified => "unspecified",
        }
    }
}

/// One `DOT11_RADIO_STATE` switch (`on` = 1, `off` = 2, `unknown` = 0).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioSwitch {
    On,
    Off,
    Unknown,
}

impl RadioSwitch {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        }
    }
}

/// The link state of a WLAN interface (`WLAN_INTERFACE_STATE`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WlanLinkState {
    NotReady,
    Connected,
    AdHocNetworkFormed,
    Disconnecting,
    Disconnected,
    Associating,
    Discovering,
    Authenticating,
    /// A state this contract does not name; the raw value is preserved beside it.
    Unknown,
}

/// One physical layer's documented software and hardware radio switches.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PhyRadioState {
    pub phy_index: u32,
    pub software: RadioSwitch,
    pub hardware: RadioSwitch,
}

/// The Wi-Fi-only detail behind a [`RadioState::Off`]: which switch is actually off.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WlanRadioDetail {
    /// Textual interface GUID as reported by the WLAN service (not a secret).
    pub interface_guid: String,
    /// Human description from the WLAN service, when it is readable text.
    pub interface_description: Option<String>,
    pub link_state: WlanLinkState,
    /// Raw `WLAN_INTERFACE_STATE` value, retained so an unnamed state stays honest.
    pub link_state_raw: i32,
    /// Per-PHY radio switches. Empty when the service reported no PHY.
    pub phys: Vec<PhyRadioState>,
}

/// One radio as observed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RadioStatus {
    pub kind: RadioKind,
    /// The platform's own name for the radio (localized; never used as an identifier).
    pub name: Option<String>,
    pub state: RadioState,
    /// The Wi-Fi radio's per-interface soft/hard detail. Empty for every other radio and for a
    /// radio the platform reports without a WLAN interface.
    pub wlan_interfaces: Vec<WlanRadioDetail>,
}

/// The read-only answer of `radio status`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RadioReport {
    /// Whether this process may change radios. Reported even for a read-only query so a
    /// denied write is explained before it is attempted.
    pub access: RadioAccess,
    pub radios: Vec<RadioStatus>,
    /// Kinds this platform exposes no radio for (for example `wwan` on a machine without a
    /// modem). Absence is stated, not inferred from a missing entry.
    pub absent_kinds: Vec<RadioKind>,
}

impl RadioReport {
    /// Whether an observation exists for `kind`.
    #[must_use]
    pub fn status_of(&self, kind: RadioKind) -> Option<&RadioStatus> {
        self.radios.iter().find(|radio| radio.kind == kind)
    }
}

/// The outcome of `radio off|on`.
///
/// A dry run reports the observed `previous` state and the `requested` state without performing
/// any write, so `observed` stays `None`. Only a commit ever carries an observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RadioTransition {
    pub mode: ApplyMode,
    pub kind: RadioKind,
    pub name: Option<String>,
    pub requested: RadioState,
    /// State observed before the write. A no-op request reports `previous == requested` and
    /// performs no write.
    pub previous: RadioState,
    /// State read back after the write (or the pre-write observation for a no-op). `None` on a
    /// dry run, where no write is performed.
    pub observed: Option<RadioState>,
    pub access: RadioAccess,
    /// Set when the read-back missed `requested` and the previous state was restored: the
    /// state observed after that restore.
    pub rollback_observed: Option<RadioState>,
}

impl RadioTransition {
    #[must_use]
    pub fn verified(&self) -> bool {
        self.mode == ApplyMode::Commit && self.observed == Some(self.requested)
    }
}

/// Public service contract consumed by the CLI on either platform.
pub trait RadioControl: Send + Sync {
    /// Read every enumerated radio plus the Wi-Fi per-PHY detail.
    fn radio_status(&self) -> Result<RadioReport>;

    /// Request `state` for `kind` and verify it by read-back.
    ///
    /// `allow_session_cut` bypasses the fail-closed session guard
    /// ([`radio_session_cut`]) and exists for an operator with out-of-band access. It is never
    /// needed to switch a radio on. `apply` of [`ApplyMode::DryRun`] must observe without
    /// writing; the guard is still evaluated so a dry run reports a refusal the commit would hit.
    fn radio_set_state(
        &self,
        kind: RadioKind,
        state: RadioState,
        allow_session_cut: bool,
        apply: ApplyMode,
    ) -> Result<RadioTransition>;
}

/// Network interface classes the session-cut guard distinguishes.
///
/// Only the classes that map onto a radio are named; everything else is [`InterfaceKind::Other`]
/// because the guard never needs to reason about it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    /// `IF_TYPE_IEEE80211`.
    Wifi,
    /// `IF_TYPE_WWANPP` / `IF_TYPE_WWANPP2`.
    MobileBroadband,
    /// An Ethernet-like adapter whose description names a Bluetooth personal area network.
    BluetoothPan,
    Other,
}

/// The interface facts the session-cut guard needs, gathered from the platform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkInterfaceFacts {
    /// Human name (the adapter's friendly name), used only for the refusal message.
    pub name: String,
    pub description: String,
    pub kind: InterfaceKind,
    pub addresses: Vec<IpAddr>,
    /// The adapter was reported up (operationally) *and* holds a gateway address, which is the
    /// platform's own statement that it can own a default route.
    pub has_default_gateway: bool,
}

impl NetworkInterfaceFacts {
    /// Whether switching `kind` off would take this interface down with it.
    #[must_use]
    pub const fn carries_radio(&self, kind: RadioKind) -> bool {
        matches!(
            (kind, self.kind),
            (RadioKind::Wifi, InterfaceKind::Wifi)
                | (RadioKind::MobileBroadband, InterfaceKind::MobileBroadband)
                | (RadioKind::Bluetooth, InterfaceKind::BluetoothPan)
        )
    }
}

/// The remote session whose loss a radio write could cause.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoteSession {
    /// The connecting peer.
    pub peer: IpAddr,
    /// This host's address for that connection, when the environment reports it.
    pub local: Option<IpAddr>,
}

/// Why a radio write would endanger the current session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCutReason {
    /// The radio's interface owns the address this session is bound to locally.
    SessionLocalAddress,
    /// The radio's interface holds a gateway, so it can own the default route.
    DefaultRoute,
}

/// A refusal to switch a radio off, naming the interface that carries the session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionCutRefusal {
    pub interface: String,
    pub radio: RadioKind,
    pub reason: SessionCutReason,
}

impl SessionCutRefusal {
    /// The complete sentence for a CLI error, naming the exact interface and radio.
    #[must_use]
    pub fn detail(&self) -> String {
        let reason = match self.reason {
            SessionCutReason::SessionLocalAddress => {
                "owns this session's local address, so switching it off would sever this session"
            }
            SessionCutReason::DefaultRoute => {
                "owns the default route on this machine, so switching it off would sever this session"
            }
        };
        format!(
            "refusing to switch the {} radio off: interface \"{}\" {reason}; re-run with --force only if the machine is reachable out-of-band",
            self.radio.as_str(),
            self.interface,
        )
    }

    /// The error a caller returns for a refusal.
    #[must_use]
    pub fn to_error(&self) -> LctrlError {
        LctrlError::PermissionDenied {
            need: self.detail(),
        }
    }
}

/// Decide whether switching `kind` off would cut `session`.
///
/// Fails closed: any interface that carries the radio and either owns the session's local
/// address or holds a gateway is a refusal. `None` when the radio is not guardable
/// ([`RadioKind::is_session_guardable`]) or no remote session is known.
#[must_use]
pub fn radio_session_cut(
    kind: RadioKind,
    interfaces: &[NetworkInterfaceFacts],
    session: Option<&RemoteSession>,
) -> Option<SessionCutRefusal> {
    if !kind.is_session_guardable() {
        return None;
    }
    let session = session?;
    let local = session.local;
    // The address comparison is checked across all interfaces first so the refusal names the
    // strongest evidence available.
    if let Some(local) = local {
        if let Some(interface) = interfaces
            .iter()
            .find(|interface| interface.carries_radio(kind) && interface.addresses.contains(&local))
        {
            return Some(SessionCutRefusal {
                interface: interface.name.clone(),
                radio: kind,
                reason: SessionCutReason::SessionLocalAddress,
            });
        }
    }
    if let Some(interface) = interfaces
        .iter()
        .find(|interface| interface.carries_radio(kind) && interface.has_default_gateway)
    {
        return Some(SessionCutRefusal {
            interface: interface.name.clone(),
            radio: kind,
            reason: SessionCutReason::DefaultRoute,
        });
    }
    None
}

/// The remote session this process runs under, from the SSH server's environment.
///
/// `SSH_CONNECTION` is `"<peer address> <peer port> <local address> <local port>"`;
/// `SSH_CLIENT` is `"<peer address> <peer port> <local port>"` and carries no local address.
/// Process environment is the only source: no socket is inspected and no session is invented.
#[must_use]
pub fn remote_session_from_env() -> Option<RemoteSession> {
    let connection = std::env::var("SSH_CONNECTION").ok();
    let client = std::env::var("SSH_CLIENT").ok();
    parse_remote_session(connection.as_deref(), client.as_deref())
}

/// Parse the SSH server's session variables (kept separate from [`remote_session_from_env`] so
/// the format is testable without a process environment).
#[must_use]
pub fn parse_remote_session(
    connection: Option<&str>,
    client: Option<&str>,
) -> Option<RemoteSession> {
    if let Some(connection) = connection {
        let mut fields = connection.split_whitespace();
        if let Some(peer) = fields.next().and_then(|value| value.parse::<IpAddr>().ok()) {
            let _peer_port = fields.next();
            let local = fields.next().and_then(|value| value.parse::<IpAddr>().ok());
            return Some(RemoteSession { peer, local });
        }
    }
    let client = client?;
    let peer = client
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<IpAddr>().ok())?;
    Some(RemoteSession { peer, local: None })
}

/// Validate a requested state for a radio write.
pub fn validate_requested_state(state: RadioState) -> Result<RadioState> {
    state.requested().ok_or_else(|| LctrlError::InvalidArgument {
        detail: format!(
            "radio state \"{}\" is an observation, not a request: use \"on\" or \"off\"",
            state.as_str()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface(
        name: &str,
        kind: InterfaceKind,
        addresses: &[&str],
        has_default_gateway: bool,
    ) -> NetworkInterfaceFacts {
        NetworkInterfaceFacts {
            name: name.to_owned(),
            description: name.to_owned(),
            kind,
            addresses: addresses
                .iter()
                .map(|address| address.parse().expect("test address parses"))
                .collect(),
            has_default_gateway,
        }
    }

    fn session(local: &str) -> RemoteSession {
        RemoteSession {
            peer: "100.116.130.58".parse().expect("peer address"),
            local: Some(local.parse().expect("local address")),
        }
    }

    #[test]
    fn session_local_address_on_the_wifi_interface_refuses_and_names_it() {
        let interfaces = [
            interface("Tailscale", InterfaceKind::Other, &["100.74.217.107"], false),
            interface("WLAN", InterfaceKind::Wifi, &["10.135.66.206"], true),
        ];
        let refusal = radio_session_cut(
            RadioKind::Wifi,
            &interfaces,
            Some(&session("10.135.66.206")),
        )
        .expect("a Wi-Fi session must be refused");
        assert_eq!(refusal.interface, "WLAN");
        assert_eq!(refusal.reason, SessionCutReason::SessionLocalAddress);
        assert!(refusal.detail().contains("WLAN"));
        assert!(refusal.detail().contains("--force"));
    }

    #[test]
    fn a_tunnel_session_still_refuses_when_the_wifi_interface_holds_the_default_route() {
        let interfaces = [
            interface("Tailscale", InterfaceKind::Other, &["100.74.217.107"], false),
            interface("WLAN", InterfaceKind::Wifi, &["10.135.66.206"], true),
        ];
        let refusal = radio_session_cut(
            RadioKind::Wifi,
            &interfaces,
            Some(&session("100.74.217.107")),
        )
        .expect("the default-route holder must be refused");
        assert_eq!(refusal.reason, SessionCutReason::DefaultRoute);
        assert_eq!(refusal.interface, "WLAN");
    }

    #[test]
    fn a_down_wifi_interface_without_a_gateway_does_not_refuse() {
        let interfaces = [
            interface("Ethernet", InterfaceKind::Other, &["192.168.5.240"], true),
            interface("WLAN 2", InterfaceKind::Wifi, &["169.254.91.136"], false),
        ];
        assert!(radio_session_cut(
            RadioKind::Wifi,
            &interfaces,
            Some(&session("192.168.5.240")),
        )
        .is_none());
    }

    #[test]
    fn bluetooth_and_other_radios_are_never_guarded() {
        let interfaces = [
            interface(
                "Bluetooth PAN",
                InterfaceKind::BluetoothPan,
                &["169.254.17.145"],
                false,
            ),
            interface("WLAN", InterfaceKind::Wifi, &["10.135.66.206"], true),
        ];
        let session = session("10.135.66.206");
        assert!(radio_session_cut(RadioKind::Bluetooth, &interfaces, Some(&session)).is_none());
        assert!(radio_session_cut(RadioKind::Fm, &interfaces, Some(&session)).is_none());
        assert!(radio_session_cut(RadioKind::Other, &interfaces, Some(&session)).is_none());
    }

    #[test]
    fn a_local_session_is_not_guarded() {
        let interfaces = [interface("WLAN", InterfaceKind::Wifi, &["10.135.66.206"], true)];
        assert!(radio_session_cut(RadioKind::Wifi, &interfaces, None).is_none());
    }

    #[test]
    fn wwan_is_guarded_by_its_own_interface_class() {
        let interfaces = [
            interface("Cellular", InterfaceKind::MobileBroadband, &["10.1.2.3"], true),
            interface("WLAN", InterfaceKind::Wifi, &["10.135.66.206"], false),
        ];
        let refusal = radio_session_cut(
            RadioKind::MobileBroadband,
            &interfaces,
            Some(&session("10.1.2.3")),
        )
        .expect("a WWAN session must be refused");
        assert_eq!(refusal.interface, "Cellular");
        assert_eq!(refusal.reason, SessionCutReason::SessionLocalAddress);
    }

    #[test]
    fn ssh_connection_parsing_keeps_peer_and_local() {
        let session = parse_remote_session(Some("100.116.130.58 51050 100.74.217.107 22"), None)
            .expect("SSH_CONNECTION parses");
        assert_eq!(session.peer, "100.116.130.58".parse::<IpAddr>().unwrap());
        assert_eq!(session.local, Some("100.74.217.107".parse().unwrap()));
    }

    #[test]
    fn ssh_client_parsing_has_no_local_address_but_still_reports_a_session() {
        let session = parse_remote_session(None, Some("fe80::1 51050 22"))
            .expect("SSH_CLIENT parses without a local address");
        assert_eq!(session.local, None);
        assert_eq!(session.peer, "fe80::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn missing_or_malformed_session_environment_is_no_session() {
        assert!(parse_remote_session(None, None).is_none());
        assert!(parse_remote_session(Some("not-an-address 1 2 3"), None).is_none());
        assert!(parse_remote_session(Some(""), Some("  ")).is_none());
    }

    #[test]
    fn requested_states_exclude_observations() {
        assert_eq!(RadioState::On.requested(), Some(RadioState::On));
        assert_eq!(RadioState::Off.requested(), Some(RadioState::Off));
        assert!(validate_requested_state(RadioState::Disabled).is_err());
        assert!(validate_requested_state(RadioState::Unknown).is_err());
    }

    #[test]
    fn selectable_kinds_round_trip_through_their_names() {
        for kind in RadioKind::SELECTABLE {
            assert_eq!(RadioKind::parse(kind.as_str()), Some(*kind));
        }
        assert_eq!(RadioKind::parse("cellular"), None);
    }

    #[test]
    fn radio_kinds_map_to_the_interface_classes_that_carry_them() {
        let wifi = interface("WLAN", InterfaceKind::Wifi, &[], false);
        assert!(wifi.carries_radio(RadioKind::Wifi));
        assert!(!wifi.carries_radio(RadioKind::MobileBroadband));
        assert!(!wifi.carries_radio(RadioKind::Bluetooth));
    }
}
