use lctrl_core::{ApplyMode, Availability, CapabilitySet, Platform};
use lctrl_tune::{
    EcMode, FanMode, PanelRefresh, Planner, ProfileCatalog, ProfileOrigin, TriggerClass,
    TuneSetting, parse_profile_toml,
};

fn doc(text: &str, origin: ProfileOrigin) -> lctrl_tune::ProfileDocument {
    parse_profile_toml(text, origin).expect("valid profile")
}

#[test]
fn parser_accepts_schema_v1_typed_goals_and_performance_fan() {
    let parsed = doc(
        r#"
        schema = 1
        [profile]
        name = "game"
        [goal]
        ec_mode = 2
        pl1_w = 25
        pl2_w = 45
        tau_s = 28
        epp = "performance"
        turbo = true
        fan_mode = "performance"
        panel_hz = "adaptive"
        charge_mode = "conservation"
        backlight = 2
        "#,
        ProfileOrigin::User,
    );

    assert_eq!(parsed.profile.name.as_str(), "game");
    assert_eq!(parsed.origin, ProfileOrigin::User);
    assert_eq!(parsed.goal.ec_mode, Some(EcMode::Beast));
    assert_eq!(parsed.goal.fan_mode, Some(FanMode::Performance));
    assert_eq!(parsed.goal.panel_hz, Some(PanelRefresh::Adaptive));
}

#[test]
fn parser_rejects_unknown_schema_and_nested_fields() {
    let unknown_schema = r#"
        schema = 2
        [profile]
        name = "bad"
    "#;
    assert!(parse_profile_toml(unknown_schema, ProfileOrigin::User).is_err());

    let unknown_root = r#"
        [profile]
        name = "bad"
        mystery = true
    "#;
    assert!(parse_profile_toml(unknown_root, ProfileOrigin::User).is_err());

    let unknown_goal = r#"
        [profile]
        name = "bad"
        [goal]
        epp = "power"
        unknown_goal = 1
    "#;
    assert!(parse_profile_toml(unknown_goal, ProfileOrigin::User).is_err());

    let unknown_trigger = r#"
        [profile]
        name = "bad"
        [[trigger]]
        type = "on_ac"
        mystery = true
    "#;
    assert!(parse_profile_toml(unknown_trigger, ProfileOrigin::User).is_err());
}

#[test]
fn source_precedence_and_recursive_field_inheritance_are_deterministic() {
    let builtin_base = doc(
        r#"
        [profile]
        name = "base"
        [goal]
        ec_mode = 0
        pl1_w = 15
        pl2_w = 30
        fan_mode = "smart"
        "#,
        ProfileOrigin::Builtin,
    );
    let builtin = doc(
        r#"
        [profile]
        name = "balanced"
        priority = 1
        inherits = "base"
        "#,
        ProfileOrigin::Builtin,
    );
    let system = doc(
        r#"
        [profile]
        name = "balanced"
        priority = 2
        inherits = "base"
        [goal]
        epp = "balance-performance"
        "#,
        ProfileOrigin::System,
    );
    let user = doc(
        r#"
        [profile]
        name = "quiet"
        inherits = "balanced"
        [goal]
        turbo = false
        "#,
        ProfileOrigin::User,
    );

    let catalog = ProfileCatalog::from_layers([builtin_base, builtin], [system], [user]).unwrap();
    let resolved = catalog.get("quiet").unwrap();
    assert_eq!(resolved.goal.ec_mode, Some(EcMode::Smart));
    assert_eq!(resolved.goal.pl1_w, Some(15));
    assert_eq!(
        resolved.goal.epp.unwrap().to_string(),
        "balance-performance"
    );
    assert_eq!(resolved.goal.turbo, Some(false));
    assert_eq!(catalog.get("balanced").unwrap().priority, 2);
    assert_eq!(
        catalog.get("balanced").unwrap().origin,
        ProfileOrigin::System
    );
}

#[test]
fn inheritance_rejects_missing_reference_and_cycles() {
    let missing = doc(
        r#"
        [profile]
        name = "child"
        inherits = "no-such-profile"
        "#,
        ProfileOrigin::User,
    );
    assert!(ProfileCatalog::from_layers([], [], [missing]).is_err());

    let a = doc(
        r#"
        [profile]
        name = "a"
        inherits = "b"
        "#,
        ProfileOrigin::User,
    );
    let b = doc(
        r#"
        [profile]
        name = "b"
        inherits = "a"
        "#,
        ProfileOrigin::User,
    );
    assert!(ProfileCatalog::from_layers([], [], [a, b]).is_err());
}

#[test]
fn inherited_power_limits_must_obey_pl1_at_most_pl2() {
    let base = doc(
        r#"
        [profile]
        name = "base"
        [goal]
        pl2_w = 20
        "#,
        ProfileOrigin::Builtin,
    );
    let child = doc(
        r#"
        [profile]
        name = "child"
        inherits = "base"
        [goal]
        pl1_w = 21
        "#,
        ProfileOrigin::User,
    );
    let error = ProfileCatalog::from_layers([base], [], [child]).unwrap_err();
    assert!(error.to_string().contains("PL1"));
}

#[test]
fn ranking_uses_trigger_class_then_priority_then_lexical_name() {
    let resident = doc(
        r#"
        [profile]
        name = "z-resident"
        priority = 99
        "#,
        ProfileOrigin::Builtin,
    );
    let battery = doc(
        r#"
        [profile]
        name = "battery"
        priority = 0
        [[trigger]]
        type = "on_battery"
        "#,
        ProfileOrigin::Builtin,
    );
    let process_z = doc(
        r#"
        [profile]
        name = "z-process"
        priority = 1
        [[trigger]]
        type = "process_match"
        names = ["game.exe"]
        "#,
        ProfileOrigin::User,
    );
    let process_a = doc(
        r#"
        [profile]
        name = "a-process"
        priority = 1
        [[trigger]]
        type = "process_match"
        names = ["editor.exe"]
        "#,
        ProfileOrigin::User,
    );

    let catalog =
        ProfileCatalog::from_layers([resident, battery], [], [process_z, process_a]).unwrap();
    let ranked = catalog.ranked();
    assert_eq!(ranked[0].name.as_str(), "a-process");
    assert_eq!(ranked[1].name.as_str(), "z-process");
    assert_eq!(ranked[0].trigger_class(), TriggerClass::Process);
    assert_eq!(ranked[2].name.as_str(), "battery");
    assert_eq!(ranked[3].name.as_str(), "z-resident");
}

#[test]
fn windows_plan_never_fabricates_balanced_power_writes() {
    let balanced = ProfileCatalog::builtins().unwrap();
    let profile = balanced.get("balanced").unwrap();
    let caps = CapabilitySet::new(Platform::Windows);
    let plan = Planner::compile(
        profile,
        Platform::Windows,
        &Default::default(),
        &caps,
        ApplyMode::DryRun,
    )
    .unwrap();
    assert!(plan.writes.iter().all(|setting| {
        !matches!(
            setting,
            TuneSetting::Pl1(_) | TuneSetting::Pl2(_) | TuneSetting::Tau(_)
        )
    }));
}

#[test]
fn unavailable_targets_are_skipped_in_dry_run_and_fail_in_commit() {
    let profile = doc(
        r#"
        [profile]
        name = "portable"
        [goal]
        ec_mode = 1
        pl1_w = 10
        pl2_w = 20
        fan_mode = "performance"
        "#,
        ProfileOrigin::User,
    );
    let catalog = ProfileCatalog::from_layers([], [], [profile]).unwrap();
    let profile = catalog.get("portable").unwrap();
    let mut caps = CapabilitySet::new(Platform::Linux);
    caps.record("ec_mode", Availability::Available, None)
        .unwrap();

    let dry = Planner::compile(
        profile,
        Platform::Linux,
        &Default::default(),
        &caps,
        ApplyMode::DryRun,
    )
    .unwrap();
    assert_eq!(dry.writes.len(), 1);
    assert!(
        dry.skipped
            .iter()
            .any(|target| target.target.as_str() == "pl1_w")
    );
    assert!(
        dry.skipped
            .iter()
            .any(|target| target.target.as_str() == "fan_mode")
    );

    let error = Planner::compile(
        profile,
        Platform::Linux,
        &Default::default(),
        &caps,
        ApplyMode::Commit,
    )
    .unwrap_err();
    assert!(error.to_string().contains("unavailable"));
}
