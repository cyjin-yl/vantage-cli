use std::sync::Mutex;

use lctrl_core::{LctrlError, MagicBayKind, Result};
use lctrl_hal::MobileBroadbandControl;

use super::{
    ADAPTER_QUERY, IF_TYPE_WWANPP, IF_TYPE_WWANPP2, MODULE_QUERY, ROOT_CIMV2, ROOT_STANDARD_CIMV2,
    WindowsMobileBroadband, probe_mobile_broadband,
};
use crate::{WmiObject, WmiTransport, WmiValue};

#[derive(Default)]
struct FakeWmi {
    modules: Vec<WmiObject>,
    adapters: Vec<WmiObject>,
    addresses: Vec<WmiObject>,
    namespaces: Mutex<Vec<String>>,
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        self.namespaces
            .lock()
            .expect("namespace log lock")
            .push(format!("{namespace}|{wql}"));
        match (namespace, wql) {
            (ROOT_CIMV2, MODULE_QUERY) => Ok(self.modules.clone()),
            (ROOT_STANDARD_CIMV2, ADAPTER_QUERY) => Ok(self.adapters.clone()),
            (ROOT_STANDARD_CIMV2, query) if query == super::ADDRESS_QUERY => {
                Ok(self.addresses.clone())
            }
            _ => Err(LctrlError::Unsupported {
                feature: format!("unexpected query {namespace} {wql}"),
            }),
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        Err(LctrlError::Unsupported {
            feature: "no method invocation in this channel".into(),
        })
    }
}

fn object(pairs: &[(&str, WmiValue)]) -> WmiObject {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

fn module_row(device_id: &str) -> WmiObject {
    object(&[("DeviceID", WmiValue::String(device_id.into()))])
}

fn adapter_row(index: u32, if_type: u32, media: u32, operational: u32) -> WmiObject {
    object(&[
        ("InterfaceIndex", WmiValue::U32(index)),
        ("InterfaceType", WmiValue::U16(u16::try_from(if_type).unwrap())),
        (
            "InterfaceDescription",
            WmiValue::String("Generic Mobile Broadband Adapter".into()),
        ),
        ("MediaConnectState", WmiValue::U32(media)),
        ("InterfaceOperationalStatus", WmiValue::U32(operational)),
    ])
}

fn address_row(index: u32, address: &str, family: u32) -> WmiObject {
    object(&[
        ("InterfaceIndex", WmiValue::U32(index)),
        ("IPAddress", WmiValue::String(address.into())),
        ("AddressFamily", WmiValue::U16(u16::try_from(family).unwrap())),
    ])
}

#[test]
fn attached_composite_module_reports_link_state_and_data_session() {
    let wmi = FakeWmi {
        modules: vec![
            module_row(r"USB\VID_17EF&PID_7005\0123456789AB"),
            module_row(r"USB\VID_17EF&PID_7005&MI_00\7&2F1A3B4C&0&0000"),
            module_row(r"USB\VID_17EF&PID_1117\HUD0001"),
        ],
        adapters: vec![
            adapter_row(41, 6, 2, 2),
            adapter_row(57, IF_TYPE_WWANPP, 1, 1),
        ],
        addresses: vec![
            address_row(57, "10.42.7.9", 2),
            address_row(57, "fe80::1c2b:3f4d:5e6f:7081%57", 23),
            address_row(41, "192.168.5.240", 2),
        ],
        ..FakeWmi::default()
    };

    let status = WindowsMobileBroadband::new(&wmi)
        .lte_status()
        .expect("attached module reports status");

    assert_eq!(status.modules.len(), 1, "parent and MI_00 coalesce");
    assert_eq!(status.modules[0].kind, MagicBayKind::Lte2);
    assert_eq!(status.modules[0].interfaces, vec!["mbim"]);
    assert_eq!(status.interfaces.len(), 1, "only WWAN interfaces are listed");
    assert_eq!(status.interfaces[0].interface_index, 57);
    assert_eq!(status.interfaces[0].ipv4, vec!["10.42.7.9"]);
    assert_eq!(status.interfaces[0].ipv6.len(), 1);
    assert!(status.data_session, "a routable address means a data session");
}

#[test]
fn unrelated_wwan_modem_is_not_reported_under_the_accessory_channel() {
    let wmi = FakeWmi {
        modules: Vec::new(),
        adapters: vec![adapter_row(57, IF_TYPE_WWANPP2, 1, 1)],
        addresses: vec![address_row(57, "10.42.7.9", 2)],
        ..FakeWmi::default()
    };

    match WindowsMobileBroadband::new(&wmi).lte_status() {
        Err(LctrlError::ChannelUnavailable { channel }) => {
            assert!(channel.contains("no MagicBay LTE accessory"), "{channel}");
        }
        other => panic!("unexpected result {other:?}"),
    }
}

#[test]
fn tiko_pid_is_recognised_and_other_lenovo_pids_are_not_modules() {
    let wmi = FakeWmi {
        modules: vec![
            module_row(r"USB\VID_17EF&PID_62A5\TIKO0001"),
            module_row(r"USB\VID_17EF&PID_62B5\UNKNOWN0001"),
            module_row(r"USB\VID_1234&PID_7005\FOREIGN0001"),
        ],
        ..FakeWmi::default()
    };

    let status = WindowsMobileBroadband::new(&wmi)
        .lte_status()
        .expect("tiko module reports status");

    assert_eq!(status.modules.len(), 1);
    assert_eq!(status.modules[0].pid, Some(0x62a5));
    assert_eq!(status.modules[0].kind, MagicBayKind::TikoLte);
    assert!(
        status.interfaces.is_empty(),
        "a module whose MBIM function is not bound has no interface"
    );
    assert!(!status.data_session);
}

#[test]
fn link_local_only_addresses_do_not_claim_a_data_session() {
    let wmi = FakeWmi {
        modules: vec![module_row(r"USB\VID_17EF&PID_7005\0123456789AB")],
        adapters: vec![adapter_row(57, IF_TYPE_WWANPP, 1, 1)],
        addresses: vec![
            address_row(57, "169.254.13.7", 2),
            address_row(57, "fe80::1c2b:3f4d:5e6f:7081%57", 23),
        ],
        ..FakeWmi::default()
    };

    let status = WindowsMobileBroadband::new(&wmi)
        .lte_status()
        .expect("status with link-local addresses");
    assert_eq!(status.interfaces.len(), 1);
    assert_eq!(status.interfaces[0].ipv4, vec!["169.254.13.7"]);
    assert!(!status.data_session);
}

#[test]
fn documented_media_and_operational_values_are_projected() {
    let wmi = FakeWmi {
        modules: vec![module_row(r"USB\VID_17EF&PID_7005\0123456789AB")],
        adapters: vec![
            adapter_row(57, IF_TYPE_WWANPP, 1, 1),
            adapter_row(58, IF_TYPE_WWANPP2, 2, 6),
            adapter_row(59, IF_TYPE_WWANPP, 0, 5),
            adapter_row(60, IF_TYPE_WWANPP, 9, 9),
        ],
        ..FakeWmi::default()
    };

    let status = WindowsMobileBroadband::new(&wmi)
        .lte_status()
        .expect("status");
    use lctrl_core::{MagicBayLteMediaState as Media, MagicBayLteOperational as Operational};
    let expected = [
        (Media::Connected, Operational::Up),
        (Media::Disconnected, Operational::NotPresent),
        (Media::Unknown, Operational::Dormant),
        (Media::Unknown, Operational::Unknown),
    ];
    assert_eq!(status.interfaces.len(), expected.len());
    for (interface, (media, operational)) in status.interfaces.iter().zip(expected) {
        assert_eq!(interface.media_state, media, "{}", interface.interface_index);
        assert_eq!(
            interface.operational, operational,
            "{}",
            interface.interface_index
        );
    }
}

#[test]
fn rows_without_the_selected_properties_are_skipped() {
    let wmi = FakeWmi {
        modules: vec![object(&[("Name", WmiValue::String("no DeviceID".into()))])],
        adapters: vec![
            object(&[("InterfaceIndex", WmiValue::U32(70))]),
            object(&[(
                "InterfaceType",
                WmiValue::U16(u16::try_from(IF_TYPE_WWANPP).unwrap()),
            )]),
        ],
        ..FakeWmi::default()
    };

    assert!(matches!(
        WindowsMobileBroadband::new(&wmi).lte_status(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn probe_uses_the_recovered_detector_query_and_counts_wwan_interfaces() {
    let wmi = FakeWmi {
        modules: vec![module_row(r"USB\VID_17EF&PID_7005\0123456789AB")],
        adapters: vec![
            adapter_row(41, 6, 2, 2),
            adapter_row(57, IF_TYPE_WWANPP, 1, 1),
        ],
        ..FakeWmi::default()
    };

    let probe = probe_mobile_broadband(&wmi).expect("probe");
    assert_eq!(probe.inventory.devices.len(), 1);
    assert_eq!(probe.wwan_interfaces, 1);

    let namespaces = wmi.namespaces.lock().expect("namespace log").clone();
    assert_eq!(
        namespaces,
        vec![
            format!("{ROOT_CIMV2}|{MODULE_QUERY}"),
            format!("{ROOT_STANDARD_CIMV2}|{ADAPTER_QUERY}"),
        ]
    );
}
