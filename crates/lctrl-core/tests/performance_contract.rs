use lctrl_core::{
    Availability, DispatcherVersion, FanDescriptor, FanId, FanMode, FanStep, FanTable,
    PerformanceCapabilities, PerformanceMode, PerformanceState, SensorId, TemperatureLocation,
    TemperatureSensor, TemperatureSensorMetadata, TemperatureSource,
};

#[test]
fn performance_modes_round_trip_its_values() {
    let cases = [
        (PerformanceMode::Balanced, 1),
        (PerformanceMode::Quiet, 2),
        (PerformanceMode::Performance, 3),
        (PerformanceMode::Geek, 4),
    ];
    for (mode, raw) in cases {
        assert_eq!(mode.its_value(), raw);
        assert_eq!(PerformanceMode::from_its(raw).unwrap(), mode);
    }
    assert!(PerformanceMode::from_its(0).is_err());
    assert!(PerformanceMode::from_its(5).is_err());
}

#[test]
fn dispatcher_versions_use_exact_boundaries() {
    assert_eq!(
        DispatcherVersion::from_raw(0x0fff),
        DispatcherVersion::Legacy(0x0fff)
    );
    assert_eq!(DispatcherVersion::from_raw(0x1000), DispatcherVersion::V2);
    assert_eq!(DispatcherVersion::from_raw(0x1fff), DispatcherVersion::V2);
    assert_eq!(DispatcherVersion::from_raw(0x2000), DispatcherVersion::V3);
    assert_eq!(DispatcherVersion::from_raw(0x2fff), DispatcherVersion::V3);
    assert_eq!(DispatcherVersion::from_raw(0x3000), DispatcherVersion::V4);
    assert_eq!(DispatcherVersion::from_raw(u32::MAX), DispatcherVersion::V4);
}

#[test]
fn capability_mask_and_version_gate_modes() {
    let capabilities = PerformanceCapabilities::new(0x01 | 0x02 | 0x08 | 0x10);
    assert!(capabilities.supports(PerformanceMode::Balanced, DispatcherVersion::V2));
    assert!(capabilities.supports(PerformanceMode::Quiet, DispatcherVersion::V2));
    assert!(capabilities.supports(PerformanceMode::Performance, DispatcherVersion::V2));
    assert!(!capabilities.supports(PerformanceMode::Geek, DispatcherVersion::V3));
    assert!(capabilities.supports(PerformanceMode::Geek, DispatcherVersion::V4));

    let sparse = PerformanceCapabilities::new(0x01);
    assert!(sparse.supports(PerformanceMode::Balanced, DispatcherVersion::V4));
    assert!(!sparse.supports(PerformanceMode::Performance, DispatcherVersion::V4));
}

#[test]
fn requested_and_active_modes_remain_distinct() {
    let state = PerformanceState {
        requested: Some(PerformanceMode::Balanced),
        active: Some(PerformanceMode::Quiet),
        automatic: true,
        version: DispatcherVersion::V4,
        capabilities: PerformanceCapabilities::new(0x1b),
    };
    assert_ne!(state.requested, state.active);
    assert!(state.automatic);
}

#[test]
fn fan_fixture_validates_target_bounds() {
    let descriptor = FanDescriptor::new(FanId::new(0), 2100, 5100).unwrap();
    assert_eq!(descriptor.min_rpm, 2100);
    assert_eq!(descriptor.max_rpm, 5100);
    assert_eq!(descriptor.rpm_percent(2100).unwrap(), 0.0);
    assert_eq!(descriptor.rpm_percent(5100).unwrap(), 100.0);
    assert_eq!(descriptor.rpm_percent(3600).unwrap(), 50.0);
    assert!(descriptor.rpm_percent(2099).is_err());
    assert!(descriptor.rpm_percent(5101).is_err());
}

#[test]
fn zero_width_fan_range_is_rejected() {
    assert!(FanDescriptor::new(FanId::new(0), 3000, 3000).is_err());
    assert!(FanDescriptor::new(FanId::new(0), 4000, 3000).is_err());
}

#[test]
fn fan_and_sensor_ids_must_fit_wmi_u8_boundary() {
    assert_eq!(FanId::new(255).method_arg().unwrap(), 255);
    assert!(FanId::new(256).method_arg().is_err());
    assert_eq!(SensorId::new(255).method_arg().unwrap(), 255);
    assert!(SensorId::new(256).method_arg().is_err());
}

#[test]
fn fan_mode_preserves_unknown_values() {
    assert_eq!(FanMode::from_raw(0), FanMode::Standard);
    assert_eq!(FanMode::from_raw(1), FanMode::Silent);
    assert_eq!(FanMode::from_raw(2), FanMode::Performance);
    assert_eq!(FanMode::from_raw(3), FanMode::Custom);
    assert_eq!(FanMode::from_raw(99), FanMode::Unknown(99));
}

#[test]
fn fan_step_decodes_and_encodes_exact_wire_layout() {
    let step = FanStep::from_packed(0x09c4_0190);
    assert_eq!(step.temperature_deci_c, 400);
    assert_eq!(step.rpm, 2500);
    assert_eq!(step.packed(), 0x09c4_0190);
    assert_eq!(step.set_bytes(), [0x90, 0x01, 0xc4, 0x09]);
}

#[test]
fn fan_table_validation_checks_order_temperature_and_rpm() {
    let descriptor = FanDescriptor::new(FanId::new(0), 2100, 5100).unwrap();
    let valid = FanTable {
        fan_id: FanId::new(0),
        sensor_id: SensorId::new(0),
        mode: FanMode::Standard,
        min_temperature_deci_c: 0,
        max_temperature_deci_c: 850,
        steps: vec![FanStep::new(400, 2500), FanStep::new(700, 4500)],
        raw_fan_table: vec![0x09c4_0190, 0x1194_02bc],
        raw_sensor_table: vec![400, 700],
    };
    valid.validate(&descriptor).unwrap();

    let mut unordered = valid.clone();
    unordered.steps.swap(0, 1);
    assert!(unordered.validate(&descriptor).is_err());

    let mut below_min = valid.clone();
    below_min.steps[0].rpm = 2000;
    assert!(below_min.validate(&descriptor).is_err());

    let mut too_hot = valid;
    too_hot.steps[1].temperature_deci_c = 900;
    assert!(too_hot.validate(&descriptor).is_err());
}

#[test]
fn unavailable_temperature_has_no_fabricated_value() {
    let sensor = TemperatureSensor {
        metadata: TemperatureSensorMetadata {
            id: "cpu0".into(),
            name: "CPU Package".into(),
            source: TemperatureSource::WmiGamezone,
            location: TemperatureLocation::Cpu,
            availability: Availability::Unavailable,
        },
        value_c: None,
    };
    assert_eq!(sensor.metadata.availability, Availability::Unavailable);
    assert_eq!(sensor.value_c, None);
}
