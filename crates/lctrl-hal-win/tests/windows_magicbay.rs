use lctrl_core::MagicBayKind;
use lctrl_hal_win::{coalesce_magicbay_devices, parse_magicbay_instance_id};

#[test]
fn parses_verified_usb_composite_and_mbim_interface() {
    let device = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005&MI_00\6&ABC&0000").unwrap();
    assert_eq!(device.vid, Some(0x17ef));
    assert_eq!(device.pid, Some(0x7005));
    assert_eq!(device.kind, MagicBayKind::Lte2);
    assert_eq!(device.interfaces, vec!["mbim"]);
}

#[test]
fn composite_parent_and_mbim_interface_coalesce_into_one_accessory() {
    let parent = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005\SERIAL").unwrap();
    let interface = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL&0000").unwrap();

    let devices = coalesce_magicbay_devices(vec![interface, parent]);

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].path, r"USB\VID_17EF&PID_7005\SERIAL");
    assert_eq!(devices[0].interfaces, vec!["mbim"]);
}

#[test]
fn distinct_identical_pid_accessories_remain_separate() {
    let first = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005\SERIAL-A").unwrap();
    let first_interface =
        parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL-A&0000").unwrap();
    let second = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005\SERIAL-B").unwrap();
    let second_interface =
        parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005&MI_00\SERIAL-B&0000").unwrap();

    let devices = coalesce_magicbay_devices(vec![first_interface, second, second_interface, first]);

    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].interfaces, vec!["mbim"]);
    assert_eq!(devices[1].interfaces, vec!["mbim"]);
}

#[test]
fn parses_early_lte_hud_and_acpi_display_ids() {
    assert_eq!(
        parse_magicbay_instance_id(r"USB\VID_17EF&PID_62B5\X")
            .unwrap()
            .kind,
        MagicBayKind::TikoLte
    );
    assert_eq!(
        parse_magicbay_instance_id(r"USB\VID_17EF&PID_1117\X")
            .unwrap()
            .kind,
        MagicBayKind::Hud
    );
    let display = parse_magicbay_instance_id(r"ACPI\QCOM2488\0").unwrap();
    assert_eq!(display.bus, "acpi");
    assert_eq!(display.kind, MagicBayKind::DisplayBridge);
    assert_eq!(display.interfaces, vec!["display"]);
    let role_switch = parse_magicbay_instance_id(r"ACPI\QCOM24B7\0").unwrap();
    assert_eq!(role_switch.kind, MagicBayKind::UsbRoleSwitch);
    assert_eq!(role_switch.interfaces, vec!["usb_role_switch"]);
}

#[test]
fn rejects_unrelated_devices_and_preserves_unknown_vendor_pid() {
    assert!(parse_magicbay_instance_id(r"USB\VID_1234&PID_7005\X").is_none());
    assert_eq!(
        parse_magicbay_instance_id(r"USB\VID_17EF&PID_FFFF\X")
            .unwrap()
            .kind,
        MagicBayKind::Unknown
    );
}
