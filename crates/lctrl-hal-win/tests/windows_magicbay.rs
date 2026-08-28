use lctrl_core::MagicBayKind;
use lctrl_hal_win::parse_magicbay_instance_id;

#[test]
fn parses_verified_usb_composite_and_mbim_interface() {
    let device = parse_magicbay_instance_id(r"USB\VID_17EF&PID_7005&MI_00\6&ABC&0000").unwrap();
    assert_eq!(device.vid, Some(0x17ef));
    assert_eq!(device.pid, Some(0x7005));
    assert_eq!(device.kind, MagicBayKind::Lte2);
    assert_eq!(device.interfaces, vec!["mbim"]);
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
    assert_eq!(display.interfaces, vec!["display"]);
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
