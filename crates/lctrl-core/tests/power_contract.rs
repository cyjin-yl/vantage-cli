use lctrl_core::{
    LctrlError, PowerGuid, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey,
    PowerSettingValue, PowerSource, PowerValueRange, validate_power_write,
};

#[test]
fn scheme_id_rejects_empty() {
    let error = PowerSchemeId::new("").unwrap_err();
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
}
#[test]
fn identifiers_reject_whitespace_only() {
    assert!(matches!(
        PowerSchemeId::new(" \t").unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        PowerGuid::new("\n").unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}

#[test]
fn scheme_id_rejects_nul() {
    let error = PowerSchemeId::new("balanced\0").unwrap_err();
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
}

#[test]
fn guid_rejects_empty_and_nul() {
    assert!(matches!(
        PowerGuid::new("").unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        PowerGuid::new("381b4222-f694-41f0-9685-ff5bb260df2e\0").unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}

#[test]
fn valid_guid_text_is_preserved_verbatim() {
    let guid = PowerGuid::new("381b4222-f694-41f0-9685-ff5bb260df2e").expect("valid guid");
    assert_eq!(guid.as_str(), "381b4222-f694-41f0-9685-ff5bb260df2e");
    assert_eq!(guid.to_string(), "381b4222-f694-41f0-9685-ff5bb260df2e");
    assert_eq!(
        serde_json::to_value(&guid).unwrap(),
        serde_json::json!("381b4222-f694-41f0-9685-ff5bb260df2e")
    );
    let spaced = " {381B4222-F694-41F0-9685-FF5BB260DF2E} ";
    let preserved = PowerGuid::new(spaced).expect("valid textual guid");
    assert_eq!(preserved.as_str(), spaced);
    assert_eq!(preserved.to_string(), spaced);
}

#[test]
fn scheme_id_round_trips_and_serializes() {
    let id = PowerSchemeId::new("Balanced").expect("valid id");
    assert_eq!(id.as_str(), "Balanced");
    assert_eq!(id.to_string(), "Balanced");
    assert_eq!(
        serde_json::to_value(&id).unwrap(),
        serde_json::json!("Balanced")
    );
}

#[test]
fn ac_and_dc_are_distinct_sources() {
    assert_ne!(PowerSource::Ac, PowerSource::Dc);
    assert_eq!(PowerSource::Ac.to_string(), "ac");
    assert_eq!(PowerSource::Dc.to_string(), "dc");
    assert_eq!(
        serde_json::to_value(PowerSource::Ac).unwrap(),
        serde_json::json!("ac")
    );
    assert_eq!(
        serde_json::to_value(PowerSource::Dc).unwrap(),
        serde_json::json!("dc")
    );
}

#[test]
fn range_boundaries_are_inclusive() {
    let range = PowerValueRange::new(0, 100, 5).expect("valid range");
    assert!(range.contains(0));
    assert!(range.contains(100));
    assert!(range.contains(50));
    assert!(!range.contains(101));
    assert!(range.contains(5));
}

#[test]
fn range_rejects_inverted_bounds_and_zero_increment() {
    assert!(matches!(
        PowerValueRange::new(50, 10, 1).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        PowerValueRange::new(0, 100, 0).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}

#[test]
fn write_validation_respects_min_max_boundaries() {
    let range = PowerValueRange::new(10, 20, 5).expect("valid range");
    assert!(validate_power_write(10, &range).is_ok());
    assert!(validate_power_write(20, &range).is_ok());
    assert!(matches!(
        validate_power_write(9, &range).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        validate_power_write(21, &range).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}

#[test]
fn write_validation_aligns_to_increment_from_min() {
    let range = PowerValueRange::new(10, 20, 5).expect("valid range");
    assert!(validate_power_write(15, &range).is_ok());
    assert!(matches!(
        validate_power_write(12, &range).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        validate_power_write(17, &range).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}

#[test]
fn write_validation_is_overflow_safe() {
    let range = PowerValueRange::new(u32::MAX - 3, u32::MAX, 3).expect("valid range");
    assert!(validate_power_write(u32::MAX, &range).is_ok());
    assert!(matches!(
        validate_power_write(u32::MAX - 1, &range).unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
    assert!(matches!(
        validate_power_write(u32::MAX, &PowerValueRange::new(0, u32::MAX - 1, 1).unwrap())
            .unwrap_err(),
        LctrlError::InvalidArgument { .. }
    ));
}
#[test]
fn write_validation_rejects_an_invalid_range() {
    let range = PowerValueRange {
        min: 10,
        max: 20,
        increment: 0,
    };
    assert!(validate_power_write(11, &range).is_err());
}

#[test]
fn mutations_can_only_activate_or_set() {
    let id = PowerSchemeId::new("balanced").expect("valid id");
    let key = PowerSettingKey {
        subgroup: PowerGuid::new("sub").expect("valid subgroup"),
        setting: PowerGuid::new("set").expect("valid setting"),
    };
    let range = PowerValueRange::new(0, 100, 5).expect("valid range");
    let set = PowerSettingValue::new(60, &range).expect("valid setting value");
    assert_eq!(set.get(), 60);
    assert_eq!(set.to_string(), "60");

    let activate = PowerMutation::Activate(id.clone());
    let set_value = PowerMutation::SetValue {
        key: key.clone(),
        source: PowerSource::Ac,
        value: set,
    };

    let ops = serde_json::to_value(&activate).unwrap();
    assert_eq!(ops["activate"], "balanced");
    assert!(ops.get("set_value").is_none());

    let set_json = serde_json::to_value(&set_value).unwrap();
    assert_eq!(set_json["set_value"]["key"]["subgroup"], "sub");
    assert_eq!(set_json["set_value"]["key"]["setting"], "set");
    assert_eq!(set_json["set_value"]["source"], "ac");
    assert_eq!(set_json["set_value"]["value"], 60);
}

#[test]
fn setting_value_requires_a_valid_range_value() {
    let range = PowerValueRange::new(10, 20, 5).expect("valid range");
    assert_eq!(PowerSettingValue::new(10, &range).unwrap().get(), 10);
    assert!(PowerSettingValue::new(12, &range).is_err());
}

#[test]
fn scheme_metadata_is_serializable() {
    let scheme = PowerScheme::new(
        PowerSchemeId::new("Balanced").expect("valid id"),
        "Balanced power plan",
        true,
    );
    let json = serde_json::to_value(&scheme).unwrap();
    assert_eq!(json["id"], "Balanced");
    assert_eq!(json["name"], "Balanced power plan");
    assert_eq!(json["active"], true);
}
