use lctrl_core::{
    MAGICBAY_LTE_PIDS, MAGICBAY_VENDOR_ID, MagicBayKind, MagicBayLteInterface,
    MagicBayLteMediaState, MagicBayLteOperational, coalesce_magicbay_devices, identify_magicbay,
    parse_magicbay_device_id,
};

#[test]
fn known_magicbay_table_matches_verified_ids() {
    // PID literals from `LenovoMagiCenterDetector.exe` 2.0.2.1:
    // HUD {PID_1117}, LTE {PID_7005}, TIKO {PID_62A5}
    // (docs/evidence/re-magicbay-20261105.md §2.1).
    let cases = [
        (0x62a5, MagicBayKind::TikoLte),
        (0x7005, MagicBayKind::Lte2),
        (0x1117, MagicBayKind::Hud),
    ];
    for (pid, kind) in cases {
        let device = identify_magicbay(MAGICBAY_VENDOR_ID, pid).unwrap();
        assert_eq!(device.kind, kind);
    }
    assert!(identify_magicbay(0x1234, 0x7005).is_none());
    assert!(identify_magicbay(MAGICBAY_VENDOR_ID, 0xffff).is_none());
    // 0x62B5 is one off the shipped Tiko id and matches nothing Lenovo ships.
    assert!(identify_magicbay(MAGICBAY_VENDOR_ID, 0x62b5).is_none());
    assert_eq!(MAGICBAY_LTE_PIDS, [0x7005, 0x62a5]);
}

#[test]
fn known_kinds_have_stable_json_names() {
    assert_eq!(
        serde_json::to_string(&MagicBayKind::TikoLte).unwrap(),
        r#""tiko_lte""#
    );
    assert_eq!(
        serde_json::to_string(&MagicBayKind::Lte2).unwrap(),
        r#""lte2""#
    );
    assert_eq!(
        serde_json::to_string(&MagicBayKind::Hud).unwrap(),
        r#""hud""#
    );
}

#[test]
fn lte_kind_predicate_covers_both_mobile_broadband_accessories() {
    assert!(MagicBayKind::TikoLte.is_lte());
    assert!(MagicBayKind::Lte2.is_lte());
    for kind in [
        MagicBayKind::Hud,
        MagicBayKind::DisplayBridge,
        MagicBayKind::UsbRoleSwitch,
        MagicBayKind::Unknown,
    ] {
        assert!(!kind.is_lte(), "{kind:?} must not be an LTE module");
    }
}

#[test]
fn composite_parent_and_mbim_interface_coalesce_into_one_accessory() {
    let parent = parse_magicbay_device_id(r"USB\VID_17EF&PID_7005\SERIAL").unwrap();
    let interface =
        parse_magicbay_device_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL&0000").unwrap();
    assert_eq!(interface.interfaces, vec!["mbim"]);

    let devices = coalesce_magicbay_devices(vec![interface, parent]);

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].path, r"USB\VID_17EF&PID_7005\SERIAL");
    assert_eq!(devices[0].interfaces, vec!["mbim"]);
}

#[test]
fn distinct_identical_pid_accessories_remain_separate() {
    let first = parse_magicbay_device_id(r"USB\VID_17EF&PID_7005\SERIAL-A").unwrap();
    let first_interface =
        parse_magicbay_device_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL-A&0000").unwrap();
    let second = parse_magicbay_device_id(r"USB\VID_17EF&PID_7005\SERIAL-B").unwrap();
    let second_interface =
        parse_magicbay_device_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL-B&0000").unwrap();

    let devices = coalesce_magicbay_devices(vec![first_interface, second, second_interface, first]);

    assert_eq!(devices.len(), 2);
    assert!(devices.iter().all(|device| device.interfaces == ["mbim"]));
}

#[test]
fn acpi_display_bridge_and_role_switch_are_recognised() {
    let display = parse_magicbay_device_id(r"ACPI\QCOM2488\0").unwrap();
    assert_eq!(display.bus, "acpi");
    assert_eq!(display.kind, MagicBayKind::DisplayBridge);
    assert_eq!(display.interfaces, vec!["display"]);
    let role_switch = parse_magicbay_device_id(r"ACPI\QCOM24B7\0").unwrap();
    assert_eq!(role_switch.kind, MagicBayKind::UsbRoleSwitch);
    assert_eq!(role_switch.interfaces, vec!["usb_role_switch"]);
}

#[test]
fn unrelated_devices_are_rejected_and_unknown_vendor_pids_are_preserved() {
    assert!(parse_magicbay_device_id(r"USB\VID_1234&PID_7005\X").is_none());
    assert!(parse_magicbay_device_id(r"USB\VID_17EF&PID_70\X").is_none());
    assert_eq!(
        parse_magicbay_device_id(r"USB\VID_17EF&PID_FFFF\X")
            .unwrap()
            .kind,
        MagicBayKind::Unknown
    );
}

#[test]
fn documented_media_and_operational_values_are_projected() {
    assert_eq!(
        MagicBayLteMediaState::from_raw(0),
        MagicBayLteMediaState::Unknown
    );
    assert_eq!(
        MagicBayLteMediaState::from_raw(1),
        MagicBayLteMediaState::Connected
    );
    assert_eq!(
        MagicBayLteMediaState::from_raw(2),
        MagicBayLteMediaState::Disconnected
    );
    assert_eq!(
        MagicBayLteMediaState::from_raw(9),
        MagicBayLteMediaState::Unknown
    );
    let operational = [
        (1, MagicBayLteOperational::Up),
        (2, MagicBayLteOperational::Down),
        (3, MagicBayLteOperational::Testing),
        (4, MagicBayLteOperational::Unknown),
        (5, MagicBayLteOperational::Dormant),
        (6, MagicBayLteOperational::NotPresent),
        (7, MagicBayLteOperational::LowerLayerDown),
        (0, MagicBayLteOperational::Unknown),
        (99, MagicBayLteOperational::Unknown),
    ];
    for (raw, expected) in operational {
        assert_eq!(MagicBayLteOperational::from_raw(raw), expected, "{raw}");
    }
}

#[test]
fn only_routable_addresses_count_as_a_data_session() {
    let interface = |ipv4: &[&str], ipv6: &[&str]| MagicBayLteInterface {
        interface_index: 57,
        if_type: 243,
        description: "Generic Mobile Broadband Adapter".into(),
        media_state: MagicBayLteMediaState::Connected,
        operational: MagicBayLteOperational::Up,
        ipv4: ipv4.iter().map(|value| (*value).to_string()).collect(),
        ipv6: ipv6.iter().map(|value| (*value).to_string()).collect(),
    };

    assert!(interface(&["10.42.7.9"], &[]).has_global_address());
    assert!(interface(&[], &["2a00:1450::1"]).has_global_address());
    assert!(!interface(&[], &[]).has_global_address());
    assert!(!interface(&["169.254.13.7"], &["fe80::1%57"]).has_global_address());
    assert!(!interface(&["127.0.0.1"], &["::1"]).has_global_address());
}
