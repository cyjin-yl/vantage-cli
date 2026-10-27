use lctrl_core::{Availability, Capability, CapabilitySet, LctrlError, Platform};

#[test]
fn new_sets_platform_and_empty_features() {
    let set = CapabilitySet::new(Platform::Windows);

    assert_eq!(set.platform, Platform::Windows);
    assert!(set.features.is_empty());
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"{"platform":"windows","features":{}}"#
    );
}

#[test]
fn record_inserts_and_serializes_with_snake_case() {
    let mut set = CapabilitySet::new(Platform::Linux);
    set.record("battery.status", Availability::Available, None)
        .expect("record available");
    set.record(
        "tune.pl1",
        Availability::Limited,
        Some("Linux powercap only".into()),
    )
    .expect("record limited");

    let json = serde_json::to_value(&set).unwrap();

    assert_eq!(json["platform"], "linux");
    assert_eq!(
        json["features"]["battery.status"]["availability"],
        "available"
    );
    assert!(json["features"]["battery.status"].get("detail").is_none());
    assert_eq!(json["features"]["tune.pl1"]["availability"], "limited");
    assert_eq!(
        json["features"]["tune.pl1"]["detail"],
        "Linux powercap only"
    );
}

#[test]
fn record_replaces_existing_feature_without_duplicating() {
    let mut set = CapabilitySet::new(Platform::Windows);
    set.record("kbd.backlight", Availability::Available, None)
        .expect("first record");
    let replaced = set
        .record(
            "kbd.backlight",
            Availability::Limited,
            Some("driver missing".into()),
        )
        .expect("second record");

    assert_eq!(set.features.len(), 1);
    assert_eq!(
        replaced.expect("previous should be returned"),
        Capability {
            availability: Availability::Available,
            detail: None,
        }
    );
}

#[test]
fn record_rejects_empty_and_whitespace_only_feature_ids() {
    let mut set = CapabilitySet::new(Platform::Linux);
    let original_len = set.features.len();

    let empty = set.record("", Availability::Available, None);
    let whitespace = set.record("   ", Availability::Available, None);
    let tabs = set.record("\t\n", Availability::Available, None);

    assert!(matches!(empty, Err(LctrlError::InvalidArgument { .. })));
    assert!(matches!(
        whitespace,
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(matches!(tabs, Err(LctrlError::InvalidArgument { .. })));
    assert_eq!(set.features.len(), original_len);
}

#[test]
fn serialization_order_is_deterministic() {
    let mut set = CapabilitySet::new(Platform::Windows);
    set.record("z.feature", Availability::Available, None)
        .unwrap();
    set.record("a.feature", Availability::Unavailable, None)
        .unwrap();
    let mut set_again = CapabilitySet::new(Platform::Windows);
    set_again
        .record("a.feature", Availability::Unavailable, None)
        .unwrap();
    set_again
        .record("z.feature", Availability::Available, None)
        .unwrap();

    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        serde_json::to_string(&set_again).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"{"platform":"windows","features":{"a.feature":{"availability":"unavailable"},"z.feature":{"availability":"available"}}}"#
    );
}

#[test]
fn unavailability_can_carry_detail() {
    let mut set = CapabilitySet::new(Platform::Linux);
    let _ = set.record(
        "tune.pl1",
        Availability::Unavailable,
        Some("MSR path blocked".into()),
    );

    let json = serde_json::to_value(&set).unwrap();
    assert_eq!(json["features"]["tune.pl1"]["availability"], "unavailable");
    assert_eq!(json["features"]["tune.pl1"]["detail"], "MSR path blocked");
}

#[test]
fn availability_values_are_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&Availability::Available).unwrap(),
        r#""available""#
    );
    assert_eq!(
        serde_json::to_string(&Availability::Limited).unwrap(),
        r#""limited""#
    );
    assert_eq!(
        serde_json::to_string(&Availability::Unavailable).unwrap(),
        r#""unavailable""#
    );
}

#[test]
fn platform_values_are_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&Platform::Windows).unwrap(),
        r#""windows""#
    );
    assert_eq!(
        serde_json::to_string(&Platform::Linux).unwrap(),
        r#""linux""#
    );
}

#[test]
fn finding_by_id_returns_none_when_absent() {
    let set = CapabilitySet::new(Platform::Windows);
    assert!(set.get("missing").is_none());
}

#[test]
fn finding_by_id_returns_present_capability() {
    let mut set = CapabilitySet::new(Platform::Windows);
    set.record(
        "kbd.backlight",
        Availability::Limited,
        Some("driver missing".into()),
    )
    .unwrap();

    assert_eq!(
        set.get("kbd.backlight"),
        Some(&Capability {
            availability: Availability::Limited,
            detail: Some("driver missing".into()),
        })
    );
}
