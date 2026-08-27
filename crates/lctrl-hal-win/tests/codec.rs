use lctrl_core::LctrlError;
use lctrl_hal_win::{
    AdapterDetail, BatteryDetail83, GbmdCommand, GenericGet, GenericSet, IOCTL_BATTERY_CONFIG,
    IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_GENERIC_GET_VARIANT,
    IOCTL_GENERIC_SET,
};

#[test]
fn ioctl_constants_match_verified_literal_codes() {
    assert_eq!(IOCTL_GENERIC_SET, 0x8310_20c0);
    assert_eq!(IOCTL_GENERIC_GET, 0x8310_20c4);
    assert_eq!(IOCTL_GENERIC_GET_VARIANT, 0x8310_20e8);
    assert_eq!(IOCTL_GBMD, 0x8310_20f8);
    assert_eq!(IOCTL_BATTERY_CONFIG, 0x8310_2120);
    assert_eq!(IOCTL_BATTERY_DETAIL, 0x8310_2138);
    assert_eq!(IOCTL_GAPD, 0x8310_215c);
}

#[test]
fn gbmd_commands_encode_as_exactly_one_byte() {
    let cases = [
        (GbmdCommand::STATUS, 0xff),
        (GbmdCommand::CONSERVATION_ON_GEN1, 0x03),
        (GbmdCommand::CONSERVATION_OFF_GEN1, 0x05),
        (GbmdCommand::CONSERVATION_ON_GEN2, 0x0d),
        (GbmdCommand::CONSERVATION_OFF_GEN2, 0x0f),
        (GbmdCommand::RAPID_ON, 0x07),
        (GbmdCommand::RAPID_OFF, 0x08),
    ];

    for (command, expected) in cases {
        assert_eq!(command.encode(), [expected]);
    }
}

#[test]
fn gbmd_status_decodes_little_endian_dword() {
    assert_eq!(
        GbmdCommand::decode_status(&[0x04, 0x00, 0x86, 0x00]).unwrap(),
        0x0086_0004
    );
}

#[test]
fn gbmd_status_rejects_short_or_long_responses() {
    for bytes in [&[][..], &[0; 3][..], &[0; 5][..]] {
        assert!(matches!(
            GbmdCommand::decode_status(bytes),
            Err(LctrlError::FirmwareRejected { .. })
        ));
    }
}

#[test]
fn generic_get_encodes_command_and_decodes_status() {
    let get = GenericGet::new(14);

    assert_eq!(get.encode(), [14, 0, 0, 0]);
    assert_eq!(GenericGet::decode(&[0x10, 0, 0, 0]).unwrap(), 0x10);
}

#[test]
fn generic_set_encodes_three_little_endian_dwords() {
    let set = GenericSet::new(6, 1, 0x1234_5678);

    assert_eq!(
        set.encode(),
        [
            0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x78, 0x56, 0x34, 0x12,
        ]
    );
}

#[test]
fn gapd_decodes_exact_ten_byte_layout() {
    let detail =
        AdapterDetail::decode(&[0x34, 0x12, 0x78, 0x56, 100, 0, 65, 0, 0xaa, 0xbb]).unwrap();

    assert_eq!(detail.pid, 0x1234);
    assert_eq!(detail.vid, 0x5678);
    assert_eq!(detail.system_power_w, 100);
    assert_eq!(detail.current_power_w, 65);
    assert_eq!(detail.reserved, [0xaa, 0xbb]);
    assert!(detail.is_underpowered());
}

#[test]
fn gapd_rejects_any_non_ten_byte_response() {
    assert!(matches!(
        AdapterDetail::decode(&[0; 9]),
        Err(LctrlError::FirmwareRejected { .. })
    ));
    assert!(matches!(
        AdapterDetail::decode(&[0; 11]),
        Err(LctrlError::FirmwareRejected { .. })
    ));
}

#[test]
fn battery_detail_preserves_all_83_bytes_and_reads_documented_scalars() {
    let mut raw = [0u8; 83];
    raw[0..2].copy_from_slice(&9990u16.to_le_bytes());
    raw[2..4].copy_from_slice(&9645u16.to_le_bytes());
    raw[4..6].copy_from_slice(&9000u16.to_le_bytes());
    raw[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
    raw[14..16].copy_from_slice(&3061u16.to_le_bytes());
    raw[20..22].copy_from_slice(&15600u16.to_le_bytes());
    raw[22..26].copy_from_slice(b"Lion");

    let detail = BatteryDetail83::decode(&raw).unwrap();

    assert_eq!(detail.as_bytes(), &raw);
    assert_eq!(detail.read_u16(0).unwrap(), 9990);
    assert_eq!(detail.read_u16(2).unwrap(), 9645);
    assert_eq!(detail.read_u16(4).unwrap(), 9000);
    assert_eq!(detail.read_u32(6).unwrap(), u32::MAX);
    assert_eq!(detail.read_u16(14).unwrap(), 3061);
    assert_eq!(detail.read_u16(20).unwrap(), 15600);
    assert_eq!(&detail.as_bytes()[22..26], b"Lion");
}

#[test]
fn battery_detail_rejects_wrong_size_and_out_of_range_access() {
    assert!(matches!(
        BatteryDetail83::decode(&[0; 82]),
        Err(LctrlError::FirmwareRejected { .. })
    ));
    let detail = BatteryDetail83::decode(&[0; 83]).unwrap();
    assert!(matches!(
        detail.read_u16(82),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(matches!(
        detail.read_u32(80),
        Err(LctrlError::InvalidArgument { .. })
    ));
}
