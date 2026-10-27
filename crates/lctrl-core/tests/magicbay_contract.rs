use lctrl_core::{MAGICBAY_VENDOR_ID, MagicBayKind, identify_magicbay};

#[test]
fn known_magicbay_table_matches_verified_ids() {
    let cases = [
        (0x62b5, MagicBayKind::TikoLte),
        (0x7005, MagicBayKind::Lte2),
        (0x1117, MagicBayKind::Hud),
    ];
    for (pid, kind) in cases {
        let device = identify_magicbay(MAGICBAY_VENDOR_ID, pid).unwrap();
        assert_eq!(device.kind, kind);
    }
    assert!(identify_magicbay(0x1234, 0x7005).is_none());
    assert!(identify_magicbay(MAGICBAY_VENDOR_ID, 0xffff).is_none());
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
