use lctrl_core::{
    BacklightState, BiosChange, BiosName, BiosPasswordStatus, BiosRisk, BiosValue, DeviceState,
    DolbyProfile, GamutMode, LightingEffect, LockState, LowLatencyMode, NoiseCancellationMode,
    PanelAdapterId, PanelDisplayMode, PanelMode, PanelRefreshCapability, PanelRefreshRequest,
    PanelRotation, PanelSupportBits, PanelTarget, PrivacyDevice, PrivacyState, RefreshMode,
    SenseDistance, SenseGlobal, SenseMode, SenseWait, classify_risk, is_success,
    parse_current_setting, parse_selections, save_parameter,
};

#[test]
fn lighting_effects_preserve_unknown_values() {
    for raw in 0..=4 {
        assert_eq!(LightingEffect::from_raw(raw).raw(), raw);
    }
    assert_eq!(LightingEffect::from_raw(99), LightingEffect::Unknown(99));
}

#[test]
fn backlight_rejects_levels_above_discovered_max() {
    assert!(BacklightState::new(2, 3, LightingEffect::Static).is_ok());
    assert!(BacklightState::new(4, 3, LightingEffect::Static).is_err());
    assert!(BacklightState::new(0, 0, LightingEffect::Static).is_err());
}

#[test]
fn semantic_state_encodings_handle_inverted_wires() {
    assert_eq!(LockState::from_raw(1, false), LockState::Locked);
    assert_eq!(LockState::from_raw(1, true), LockState::Unlocked);
    assert_eq!(DeviceState::Enabled.raw(false), 1);
    assert_eq!(DeviceState::Enabled.raw(true), 0);
    assert_eq!(DeviceState::from_raw(1, true), DeviceState::Disabled);
}

#[test]
fn panel_modes_and_unknown_values_round_trip() {
    for raw in 0..=4 {
        assert_eq!(PanelDisplayMode::from_raw(raw).raw(), raw);
    }
    assert_eq!(
        PanelDisplayMode::from_raw(99),
        PanelDisplayMode::Unknown(99)
    );
    assert_eq!(GamutMode::from_raw(3), GamutMode::Unknown(3));
    assert_eq!(LowLatencyMode::from_raw(9), LowLatencyMode::Unknown(9));
    assert_eq!(RefreshMode::from_raw(7), RefreshMode::Unknown(7));
}

#[test]
fn panel_support_bits_are_exact() {
    let bits = PanelSupportBits::new(
        PanelSupportBits::PIP | PanelSupportBits::GAMUT | PanelSupportBits::GAME_AID_FPS,
    );
    assert!(bits.supports(PanelSupportBits::PIP));
    assert!(!bits.supports(PanelSupportBits::MPRT));
    assert!(bits.supports(PanelSupportBits::GAMUT));
    assert!(bits.supports(PanelSupportBits::GAME_AID_FPS));
}

#[test]
fn refresh_capability_uses_discovered_range() {
    let capability = PanelRefreshCapability::new(60, 120, 60);
    assert!(capability.supports_hz(60));
    assert!(capability.supports_hz(120));
    assert!(!capability.supports_hz(144));
}

#[test]
fn native_panel_mode_preserves_every_non_refresh_field() {
    let target = PanelTarget::new(r"\\.\DISPLAY1", PanelAdapterId::new(7, 9), 3, 5, true);
    let mut current = PanelMode::new(target, 3200, 2000, 32, 60);
    current.position_x = -40;
    current.position_y = 20;
    let requested = current.with_refresh_hz(120);
    assert!(current.same_mode_except_refresh(&requested));
    assert_eq!(requested.refresh_hz, 120);

    let mut wrong_width = requested.clone();
    wrong_width.width = 2560;
    let mut wrong_height = requested.clone();
    wrong_height.height = 1600;
    let mut wrong_depth = requested.clone();
    wrong_depth.bits_per_pixel = 30;
    let mut wrong_x = requested.clone();
    wrong_x.position_x = 0;
    let mut wrong_y = requested.clone();
    wrong_y.position_y = 0;
    let mut wrong_rotation = requested.clone();
    wrong_rotation.rotation = PanelRotation::Rotate180;
    let mut wrong_adapter = requested.clone();
    wrong_adapter.target.adapter = PanelAdapterId::new(7, 10);
    let mut wrong_source = requested.clone();
    wrong_source.target.source_id = 4;
    let mut wrong_target = requested.clone();
    wrong_target.target.target_id = 6;

    for altered in [
        wrong_width,
        wrong_height,
        wrong_depth,
        wrong_x,
        wrong_y,
        wrong_rotation,
        wrong_adapter,
        wrong_source,
        wrong_target,
    ] {
        assert!(!current.same_mode_except_refresh(&altered));
    }
}

#[test]
fn panel_auto_has_explicit_default_mode_semantics() {
    assert_eq!(
        PanelRefreshRequest::Auto.to_string(),
        "auto (firmware default)"
    );
    assert_eq!(PanelRefreshRequest::Fixed(60).to_string(), "60 Hz");
    for (raw, rotation) in [
        (0, PanelRotation::Default),
        (1, PanelRotation::Rotate90),
        (2, PanelRotation::Rotate180),
        (3, PanelRotation::Rotate270),
    ] {
        assert_eq!(PanelRotation::from_raw(raw), Some(rotation));
        assert_eq!(rotation.raw(), raw);
    }
    assert_eq!(PanelRotation::from_raw(4), None);
}

#[test]
fn privacy_layers_remain_independent() {
    let state = PrivacyState {
        device: PrivacyDevice::Camera,
        runtime: Some(false),
        persistent: Some(true),
    };
    assert!(state.layers_disagree());
}

#[test]
fn dolby_profiles_map_exactly_zero_through_six() {
    let profiles = [
        DolbyProfile::Movie,
        DolbyProfile::Music,
        DolbyProfile::Game,
        DolbyProfile::Voice,
        DolbyProfile::Personalize,
        DolbyProfile::Dynamic,
        DolbyProfile::Off,
    ];
    for (raw, profile) in profiles.into_iter().enumerate() {
        assert_eq!(profile.raw(), raw as u8);
        assert_eq!(DolbyProfile::from_raw(raw as u8), Some(profile));
    }
    assert_eq!(DolbyProfile::from_raw(7), None);
}

#[test]
fn noise_cancellation_maps_all_documented_values() {
    let cases = [
        (0, NoiseCancellationMode::Off),
        (1, NoiseCancellationMode::Shared),
        (2, NoiseCancellationMode::Single),
        (3, NoiseCancellationMode::Spatial),
        (4, NoiseCancellationMode::VoiceId),
    ];
    for (raw, mode) in cases {
        assert_eq!(NoiseCancellationMode::from_raw(raw), mode);
        assert_eq!(mode.raw(), raw);
    }
    // The Elevoc SDK domain is 0–7 with 6/7 remapped to 1/2 before send;
    // values outside the documented set stay explicit unknowns.
    assert_eq!(NoiseCancellationMode::from_raw(5).raw(), 5);
    assert_eq!(
        NoiseCancellationMode::from_raw(99),
        NoiseCancellationMode::Unknown(99)
    );
}

#[test]
fn sensing_values_map_only_documented_presets() {
    assert_eq!(SenseGlobal::Disabled.raw(), 2);
    assert_eq!(SenseGlobal::Enabled.raw(), 3);
    assert_eq!(SenseMode::Browsing.raw(), 2);
    assert_eq!(SenseMode::FaceDown.raw(), 3);
    assert_eq!(SenseMode::Walking.raw(), 5);
    assert_eq!(SenseDistance::Near.raw(), 0);
    assert_eq!(SenseDistance::Far.raw(), 2);
    assert_eq!(SenseWait::Short.raw(), 0);
    assert_eq!(SenseWait::Long.raw(), 2);
    assert_eq!(SenseMode::from_raw(4), None);
    assert_eq!(SenseDistance::from_raw(3), None);
    assert_eq!(SenseWait::from_raw(3), None);
}

#[test]
fn bios_names_and_values_reject_protocol_injection() {
    for name in ["", "Name,Other", "Name;Save", "Name\0Value"] {
        assert!(BiosName::new(name).is_err(), "{name:?}");
    }
    for value in ["Value,Other", "Value;Save", "Value\0Other"] {
        assert!(BiosValue::new(value).is_err(), "{value:?}");
    }
}

#[test]
fn bios_change_serialization_is_exact() {
    let change = BiosChange::new("IntegratedCamera", "Disable").unwrap();
    assert_eq!(change.serialized(), "IntegratedCamera,Disable;");
    assert_eq!(save_parameter(), ";");
}

#[test]
fn current_setting_splits_once_and_preserves_remaining_commas() {
    let item = parse_current_setting("Name,Value,WithComma").unwrap();
    assert_eq!(item.name, "Name");
    assert_eq!(item.value, "Value,WithComma");
    assert!(parse_current_setting("").is_none());
    assert!(parse_current_setting("NoComma").is_none());
}

#[test]
fn selections_trim_and_remove_empty_entries() {
    assert_eq!(
        parse_selections(" , Enable,Disable,"),
        vec!["Enable", "Disable"]
    );
}

#[test]
fn bios_success_is_exact_and_case_sensitive() {
    assert!(is_success("Success"));
    assert!(!is_success("success"));
    assert!(!is_success("Success "));
}

#[test]
fn bios_password_status_decodes_only_supervisor_bit() {
    assert!(!BiosPasswordStatus::from_raw(1, 128, 0).supervisor_set);
    assert!(BiosPasswordStatus::from_raw(1, 128, 2).supervisor_set);
    assert!(BiosPasswordStatus::from_raw(1, 128, 3).supervisor_set);
}

#[test]
fn bios_risk_classification_gates_known_dangerous_items() {
    assert_eq!(
        classify_risk("SecureRollbackPrevention"),
        BiosRisk::Destructive
    );
    assert_eq!(classify_risk("GraphicsDevice"), BiosRisk::Disruptive);
    assert_eq!(classify_risk("IntegratedCamera"), BiosRisk::Normal);
}
