use lctrl_core::{
    AdapterAuthentication, AdapterDetailValues, AdapterInfo, BatteryDate, BatteryHealth,
    BatteryTelemetry, ChargeMode, ChargeModeActual, ChargePrimitive, ChargeStatus, LctrlError,
    decode_charge_mode, plan_charge_mode,
};

#[test]
fn charge_mode_serializes_and_displays() {
    assert_eq!(
        serde_json::to_string(&ChargeMode::Normal).unwrap(),
        r#""normal""#
    );
    assert_eq!(
        serde_json::to_string(&ChargeMode::Conservation).unwrap(),
        r#""conservation""#
    );
    assert_eq!(
        serde_json::to_string(&ChargeMode::Rapid).unwrap(),
        r#""rapid""#
    );
    assert_eq!(ChargeMode::Normal.to_string(), "normal");
    assert_eq!(ChargeMode::Conservation.to_string(), "conservation");
    assert_eq!(ChargeMode::Rapid.to_string(), "rapid");
}

#[test]
fn decode_charge_mode_maps_the_three_known_states() {
    assert_eq!(decode_charge_mode(0).unwrap(), ChargeModeActual::Normal);
    assert_eq!(
        decode_charge_mode(1).unwrap(),
        ChargeModeActual::Conservation
    );
    assert_eq!(decode_charge_mode(2).unwrap(), ChargeModeActual::Rapid);
}

#[test]
fn decode_charge_mode_maps_conflict_and_unknown_combinations() {
    assert_eq!(decode_charge_mode(3).unwrap(), ChargeModeActual::Conflict);
    for raw in [4, 0x0100_0000, 0xffff_fffe] {
        assert_eq!(
            decode_charge_mode(raw).unwrap(),
            ChargeModeActual::Unknown(raw),
            "raw {raw:#x}"
        );
    }
}

#[test]
fn decode_charge_mode_maps_channel_unavailable() {
    assert!(matches!(
        decode_charge_mode(u32::MAX),
        Err(LctrlError::ChannelUnavailable { channel }) if channel == "battery charge mode"
    ));
}

#[test]
fn charge_mode_actual_displays_and_serializes() {
    assert_eq!(ChargeModeActual::Normal.to_string(), "normal");
    assert_eq!(ChargeModeActual::Conflict.to_string(), "conflict");
    assert_eq!(ChargeModeActual::Unknown(4).to_string(), "unknown (4)");
    assert_eq!(
        serde_json::to_string(&ChargeModeActual::Conflict).unwrap(),
        r#""conflict""#
    );
    assert_eq!(
        serde_json::to_string(&ChargeModeActual::Unknown(4)).unwrap(),
        r#"{"unknown":4}"#
    );
}

#[test]
fn plan_normal_disables_both_features() {
    assert_eq!(
        plan_charge_mode(ChargeMode::Normal),
        [
            ChargePrimitive::Conservation(false),
            ChargePrimitive::Rapid(false)
        ]
    );
}

#[test]
fn plan_conservation_disables_rapid_before_enabling_conservation() {
    assert_eq!(
        plan_charge_mode(ChargeMode::Conservation),
        [
            ChargePrimitive::Rapid(false),
            ChargePrimitive::Conservation(true)
        ]
    );
}

#[test]
fn plan_rapid_disables_conservation_before_enabling_rapid() {
    assert_eq!(
        plan_charge_mode(ChargeMode::Rapid),
        [
            ChargePrimitive::Conservation(false),
            ChargePrimitive::Rapid(true)
        ]
    );
}

#[test]
fn every_plan_disables_the_opposite_mode_first() {
    let cases = [
        (ChargeMode::Normal, ChargePrimitive::Conservation(false)),
        (ChargeMode::Conservation, ChargePrimitive::Rapid(false)),
        (ChargeMode::Rapid, ChargePrimitive::Conservation(false)),
    ];
    for (target, expected_first) in cases {
        let [first, _] = plan_charge_mode(target);
        assert_eq!(first, expected_first, "first step for {target}");
    }
}

#[test]
fn plans_are_semantic_and_carry_no_command_bytes() {
    // The serialized plans contain only feature names and booleans — never a
    // GBMD subcommand value (0x03/0x05/0x07/0x08/0x0d/0x0f/0xff stay in the HAL).
    assert_eq!(
        serde_json::to_string(&plan_charge_mode(ChargeMode::Normal)).unwrap(),
        r#"[{"conservation":false},{"rapid":false}]"#
    );
    assert_eq!(
        serde_json::to_string(&plan_charge_mode(ChargeMode::Conservation)).unwrap(),
        r#"[{"rapid":false},{"conservation":true}]"#
    );
    assert_eq!(
        serde_json::to_string(&plan_charge_mode(ChargeMode::Rapid)).unwrap(),
        r#"[{"conservation":false},{"rapid":true}]"#
    );
}

#[test]
fn battery_date_decodes_bitfields() {
    // 2026-08-27: year−1980=46, month=8, day=27 → 46<<9 | 8<<5 | 27 = 23835.
    assert_eq!(
        BatteryDate::decode(23835).unwrap(),
        BatteryDate {
            year: 2026,
            month: 8,
            day: 27
        }
    );
    // 1980-01-01: 1<<5 | 1 = 33.
    assert_eq!(
        BatteryDate::decode(33).unwrap(),
        BatteryDate {
            year: 1980,
            month: 1,
            day: 1
        }
    );
    // 2107-12-31: 127<<9 | 12<<5 | 31 = 65439.
    assert_eq!(
        BatteryDate::decode(65439).unwrap(),
        BatteryDate {
            year: 2107,
            month: 12,
            day: 31
        }
    );
}

#[test]
fn battery_date_rejects_unsupported_sentinel() {
    assert!(matches!(
        BatteryDate::decode(u16::MAX),
        Err(LctrlError::InvalidArgument { .. })
    ));
}

#[test]
fn battery_date_rejects_out_of_range_fields() {
    // day 0 (month 1): 1<<5 = 32.
    assert!(matches!(
        BatteryDate::decode(32),
        Err(LctrlError::InvalidArgument { .. })
    ));
    // month 0 (day 1): 1.
    assert!(matches!(
        BatteryDate::decode(1),
        Err(LctrlError::InvalidArgument { .. })
    ));
    // month 13 (day 1): 13<<5 | 1 = 417.
    assert!(matches!(
        BatteryDate::decode(417),
        Err(LctrlError::InvalidArgument { .. })
    ));
    // month 0, day 0: 0.
    assert!(matches!(
        BatteryDate::decode(0),
        Err(LctrlError::InvalidArgument { .. })
    ));
}

#[test]
fn battery_date_displays_as_iso() {
    assert_eq!(
        BatteryDate {
            year: 2026,
            month: 8,
            day: 27
        }
        .to_string(),
        "2026-08-27"
    );
}

fn telemetry_fixture() -> [u8; 83] {
    let mut raw = [0u8; 83];
    raw[0..2].copy_from_slice(&9990u16.to_le_bytes());
    raw[2..4].copy_from_slice(&9645u16.to_le_bytes());
    raw[4..6].copy_from_slice(&9000u16.to_le_bytes());
    raw[10..12].copy_from_slice(&1u16.to_le_bytes());
    raw[14..16].copy_from_slice(&3061u16.to_le_bytes());
    raw[16..18].copy_from_slice(&1200u16.to_le_bytes());
    raw[18..20].copy_from_slice(&0u16.to_le_bytes());
    raw[20..22].copy_from_slice(&15600u16.to_le_bytes());
    raw[22..40].copy_from_slice(b"Lion\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
    raw[40..52].copy_from_slice(b"Sunwoda\0\0\0\0\0");
    raw[52..76].copy_from_slice(b"W1LX5CN0294\0\0\0\0\0\0\0\0\0\0\0\0\0");
    raw[76..83].copy_from_slice(&[7, 0, 0x48, 0x0c, 0, 1, 9]);
    raw
}

#[test]
fn telemetry_parses_verified_target_layout() {
    let telemetry = BatteryTelemetry::parse(&telemetry_fixture()).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, Some(99_900));
    assert_eq!(telemetry.full_charge_capacity_mwh, Some(96_450));
    assert_eq!(telemetry.remaining_capacity_mwh, Some(90_000));
    assert_eq!(telemetry.voltage_mv, Some(15_600));
    assert_eq!(telemetry.current_ma, Some(1200));
    assert_eq!(telemetry.temperature_deci_kelvin, Some(3061));
    assert_eq!(telemetry.manufacture_date, None);
    assert_eq!(telemetry.first_used_date, None);
    assert_eq!(telemetry.design_voltage_mv, None);
    assert_eq!(telemetry.remaining_percent, None);
    assert_eq!(telemetry.life_percent, None);
    assert_eq!(telemetry.charge_status, Some(ChargeStatus::Charging));
    assert_eq!(telemetry.remaining_time_min, None);
    assert_eq!(telemetry.charge_completion_time_min, None);
    assert_eq!(telemetry.wattage_w, None);
    assert_eq!(telemetry.cycle_count, None);
    assert_eq!(telemetry.chemistry.as_deref(), Some("Lion"));
    assert_eq!(telemetry.manufacturer.as_deref(), Some("Sunwoda"));
    assert_eq!(telemetry.serial_number.as_deref(), Some("W1LX5CN0294"));
    assert_eq!(telemetry.firmware_version, None);
    assert!(telemetry.rapid_charge_allowed());
}
#[test]
fn telemetry_maps_unsupported_sentinels_to_absent_fields() {
    let telemetry = BatteryTelemetry::parse(&[0xff; 83]).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, None);
    assert_eq!(telemetry.full_charge_capacity_mwh, None);
    assert_eq!(telemetry.remaining_capacity_mwh, None);
    assert_eq!(telemetry.voltage_mv, None);
    assert_eq!(telemetry.current_ma, None); // 0xffff_ffff
    assert_eq!(telemetry.temperature_deci_kelvin, None);
    assert_eq!(telemetry.manufacture_date, None);
    assert_eq!(telemetry.first_used_date, None);
    assert_eq!(telemetry.design_voltage_mv, None);
    assert_eq!(telemetry.remaining_percent, None);
    assert_eq!(telemetry.life_percent, None);
    assert_eq!(telemetry.charge_status, None);
    assert_eq!(telemetry.remaining_time_min, None);
    assert_eq!(telemetry.charge_completion_time_min, None);
    assert_eq!(telemetry.wattage_w, None);
    assert_eq!(telemetry.cycle_count, None);
    assert!(!telemetry.rapid_charge_allowed());
}

#[test]
fn telemetry_decodes_signed_discharge_current() {
    let mut raw = telemetry_fixture();
    raw[16..18].copy_from_slice(&(-850i16).to_le_bytes());
    let telemetry = BatteryTelemetry::parse(&raw).unwrap();
    assert_eq!(telemetry.current_ma, Some(-850));
}

#[test]
fn telemetry_converts_deci_kelvin_to_celsius() {
    let telemetry = BatteryTelemetry::parse(&telemetry_fixture()).unwrap();
    let celsius = telemetry.temperature_celsius().unwrap();
    assert!((celsius - 32.94).abs() < 0.01, "got {celsius}");
    assert_eq!(telemetry.temperature_deci_kelvin, Some(3061));
}

#[test]
fn rapid_charge_is_rejected_for_a_39000_mwh_design_capacity() {
    let mut raw = telemetry_fixture();
    raw[0..2].copy_from_slice(&3900u16.to_le_bytes()); // 3900 × 10 = 39000 mWh
    let telemetry = BatteryTelemetry::parse(&raw).unwrap();
    assert_eq!(telemetry.design_capacity_mwh, Some(39_000));
    assert!(!telemetry.rapid_charge_allowed());
}

#[test]
fn rapid_charge_remains_allowed_outside_the_39wh_guard() {
    let mut raw = telemetry_fixture();
    raw[0..2].copy_from_slice(&3901u16.to_le_bytes()); // 39010 mWh
    assert!(
        BatteryTelemetry::parse(&raw)
            .unwrap()
            .rapid_charge_allowed()
    );

    let mut raw = telemetry_fixture();
    raw[0..2].copy_from_slice(&3899u16.to_le_bytes()); // 38990 mWh
    assert!(
        BatteryTelemetry::parse(&raw)
            .unwrap()
            .rapid_charge_allowed()
    );
}

#[test]
fn rapid_charge_fails_closed_when_design_capacity_is_unavailable() {
    let mut raw = telemetry_fixture();
    raw[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
    let telemetry = BatteryTelemetry::parse(&raw).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, None);
    assert!(!telemetry.rapid_charge_allowed());
}

#[test]
fn telemetry_decodes_documented_fixed_width_identity_fields() {
    let telemetry = BatteryTelemetry::parse(&telemetry_fixture()).unwrap();

    assert_eq!(telemetry.chemistry.as_deref(), Some("Lion"));
    assert_eq!(telemetry.manufacturer.as_deref(), Some("Sunwoda"));
    assert_eq!(telemetry.serial_number.as_deref(), Some("W1LX5CN0294"));
    assert_eq!(telemetry.firmware_version, None);
}

#[test]
fn telemetry_serializes_with_stable_field_names() {
    let telemetry = BatteryTelemetry::parse(&telemetry_fixture()).unwrap();
    let json = serde_json::to_value(telemetry).unwrap();
    assert_eq!(json["design_capacity_mwh"], 99_900);
    assert_eq!(json["current_ma"], 1200);
    assert_eq!(json["charge_status"], "charging");
    assert_eq!(json["temperature_deci_kelvin"], 3061);
    assert_eq!(json["voltage_mv"], 15_600);
    assert_eq!(json["chemistry"], "Lion");
    assert_eq!(json["manufacturer"], "Sunwoda");
    assert_eq!(json["serial_number"], "W1LX5CN0294");
}

#[test]
fn charge_status_maps_documented_values_and_preserves_unknowns() {
    let cases = [
        (0, ChargeStatus::NoActivity),
        (1, ChargeStatus::Charging),
        (2, ChargeStatus::Discharging),
        (3, ChargeStatus::DischargingWithAc),
        (4, ChargeStatus::Error),
        (5, ChargeStatus::Detached),
    ];
    for (raw, expected) in cases {
        assert_eq!(ChargeStatus::decode(raw), expected, "raw {raw}");
    }
    assert_eq!(ChargeStatus::decode(6), ChargeStatus::Unknown(6));
    assert_eq!(ChargeStatus::decode(0xffff), ChargeStatus::Unknown(0xffff));
    assert_eq!(ChargeStatus::Unknown(6).to_string(), "unknown (6)");
    assert_eq!(
        serde_json::to_string(&ChargeStatus::Charging).unwrap(),
        r#""charging""#
    );
    assert_eq!(
        serde_json::to_string(&ChargeStatus::Unknown(6)).unwrap(),
        r#"{"unknown":6}"#
    );
}

#[test]
fn battery_health_maps_documented_values_and_preserves_unknowns() {
    let cases = [
        (1, BatteryHealth::Green),
        (2, BatteryHealth::Yellow),
        (3, BatteryHealth::Red),
        (4, BatteryHealth::Invalid),
        (5, BatteryHealth::NotInstalled),
    ];
    for (raw, expected) in cases {
        assert_eq!(BatteryHealth::decode(raw), expected, "raw {raw}");
    }
    // Everything outside 1..=5 is the documented error state, raw code kept.
    assert_eq!(BatteryHealth::decode(0), BatteryHealth::Error(0));
    assert_eq!(BatteryHealth::decode(6), BatteryHealth::Error(6));
    assert_eq!(BatteryHealth::decode(0xffff), BatteryHealth::Error(0xffff));
    assert_eq!(BatteryHealth::Error(6).to_string(), "error (6)");
}

#[test]
fn adapter_authentication_decodes_gbmd_type_bits() {
    // Real-machine sample 0x00860004: bits 15..16 = 0 → Inbox.
    assert_eq!(
        AdapterAuthentication::from_gbmd(0x0086_0004),
        AdapterAuthentication::Inbox
    );
    // bits 15..16 = 1 → Lenovo.
    assert_eq!(
        AdapterAuthentication::from_gbmd(0x0000_8004),
        AdapterAuthentication::Lenovo
    );
    // bits 15..16 = 2 → Unknown.
    assert_eq!(
        AdapterAuthentication::from_gbmd(0x0001_0004),
        AdapterAuthentication::Unknown
    );
    // bits 15..16 = 3 → SlowCharger.
    assert_eq!(
        AdapterAuthentication::from_gbmd(0x0001_8004),
        AdapterAuthentication::SlowCharger
    );
}

#[test]
fn adapter_info_gates_detail_on_gbmd_bit24() {
    // Real-machine GBMD reply: no authenticated-charger capability (bit24=0).
    let info = AdapterInfo::from_gbmd(0x0086_0004, None);
    assert_eq!(info.authentication, AdapterAuthentication::Inbox);
    assert!(!info.has_detail);
    assert_eq!(info.detail, None);
    assert!(!info.is_underpowered());

    // Bit 24 set → detailed capability advertised even before a GAPD read.
    let info = AdapterInfo::from_gbmd(0x0100_0004, None);
    assert!(info.has_detail);
    assert_eq!(info.detail, None);
    assert!(!info.is_underpowered());
}

#[test]
fn adapter_info_computes_underpowered_only_with_detail() {
    let detail = AdapterDetailValues {
        pid: Some(0x1234),
        vid: Some(0x5678),
        system_power_w: 100,
        current_power_w: 65,
    };
    let info = AdapterInfo::from_gbmd(0x0100_0004, Some(detail));
    assert!(info.has_detail);
    assert_eq!(info.detail, Some(detail));
    assert!(info.is_underpowered());

    let sufficient = AdapterDetailValues {
        system_power_w: 65,
        current_power_w: 100,
        ..detail
    };
    assert!(!AdapterInfo::from_gbmd(0x0100_0004, Some(sufficient)).is_underpowered());

    let equal = AdapterDetailValues {
        system_power_w: 100,
        current_power_w: 100,
        ..detail
    };
    assert!(!AdapterInfo::from_gbmd(0x0100_0004, Some(equal)).is_underpowered());
}

#[test]
fn adapter_detail_values_detect_underpowered_charger() {
    let under = AdapterDetailValues {
        pid: Some(0),
        vid: Some(0),
        system_power_w: 100,
        current_power_w: 65,
    };
    assert!(under.is_underpowered());

    let over = AdapterDetailValues {
        pid: Some(0),
        vid: Some(0),
        system_power_w: 65,
        current_power_w: 100,
    };
    assert!(!over.is_underpowered());

    let equal = AdapterDetailValues {
        pid: Some(0),
        vid: Some(0),
        system_power_w: 100,
        current_power_w: 100,
    };
    assert!(!equal.is_underpowered());
}

#[test]
fn adapter_types_serialize_and_display() {
    assert_eq!(AdapterAuthentication::Lenovo.to_string(), "lenovo");
    assert_eq!(
        AdapterAuthentication::SlowCharger.to_string(),
        "slow charger"
    );
    assert_eq!(
        serde_json::to_string(&AdapterAuthentication::SlowCharger).unwrap(),
        r#""slow_charger""#
    );

    let info = AdapterInfo::from_gbmd(0x0101_0004, None);
    let json = serde_json::to_value(info).unwrap();
    assert_eq!(json["authentication"], "unknown");
    assert_eq!(json["has_detail"], true);
    assert!(json.get("detail").is_none());
}
