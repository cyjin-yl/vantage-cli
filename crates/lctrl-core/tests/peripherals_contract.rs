use lctrl_core::{
    BacklightState, BiosChange, BiosName, BiosPasswordStatus, BiosRisk, BiosValue, DeviceState,
    DolbyProfile, GamutMode, LeaveAction, LightingEffect, LockState, LowLatencyMode,
    NoiseCancellationMode, PanelDisplayMode, PanelRefreshCapability, PanelSupportBits,
    PresenceDistance, PrivacyDevice, PrivacyState, RefreshMode, SenseGlobal, SenseMode,
    classify_risk, is_success, parse_current_setting, parse_selections, save_parameter,
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
        (10, NoiseCancellationMode::FarField),
    ];
    for (raw, mode) in cases {
        assert_eq!(NoiseCancellationMode::from_raw(raw), mode);
        assert_eq!(mode.raw(), raw);
    }
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
    assert_eq!(PresenceDistance::Cm15.raw(), 0);
    assert_eq!(PresenceDistance::Cm80.raw(), 3);
    assert_eq!(LeaveAction::LockAndPause.raw(), 0);
    assert_eq!(LeaveAction::Prompt.raw(), 2);
    assert_eq!(SenseMode::from_raw(4), None);
    assert_eq!(PresenceDistance::from_raw(4), None);
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
