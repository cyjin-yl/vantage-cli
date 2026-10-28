use super::{
    CAPACITY_RELATIVE, SHORT_TERM, SYSTEM_BATTERY, admit_interface, admit_system_battery,
    attach_temperature, battery_tag, eligible, project, unavailable,
};
use lctrl_core::{BatteryElectricalObservation, LctrlError};

#[test]
fn query_tag_classifies_only_documented_empty_ports() {
    assert_eq!(battery_tag(Ok(0u32.to_le_bytes())).unwrap(), None);
    assert_eq!(battery_tag(Ok(7u32.to_le_bytes())).unwrap(), Some(7));
    assert_eq!(
        battery_tag(Err(std::io::Error::from_raw_os_error(2).into())).unwrap(),
        None
    );
    for code in [5, 6, 87, 122, 1167] {
        match battery_tag(Err(std::io::Error::from_raw_os_error(code).into())) {
            Err(LctrlError::Io(error)) => assert_eq!(error.raw_os_error(), Some(code)),
            result => panic!("unexpected tag classification: {result:?}"),
        }
    }
    assert!(battery_tag(Err(unavailable("short tag reply"))).is_err());
}

#[test]
fn rejects_enumeration_overflow_instead_of_selecting_from_partial_sources() {
    assert!(admit_interface(0).is_ok());
    assert!(admit_interface(99).is_ok());
    assert!(admit_interface(100).is_err());
    assert!(admit_interface(u32::MAX).is_err());
}

#[test]
fn rejects_second_system_battery_instead_of_guessing_identity() {
    assert!(admit_system_battery(false).is_ok());
    assert!(admit_system_battery(true).is_err());
}

fn status(power_state: u32, voltage_mv: u32, rate: i32) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&power_state.to_le_bytes());
    bytes[4..8].copy_from_slice(&12_345u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&voltage_mv.to_le_bytes());
    bytes[12..].copy_from_slice(&rate.to_le_bytes());
    bytes
}

#[test]
fn preserves_measured_voltage_signed_milliwatts_and_raw_power_state() {
    for rate in [-42_000, 0, 42_000] {
        let result = project(SYSTEM_BATTERY, &status(0x8000_0005, 17_721, rate)).unwrap();
        assert_eq!(
            result,
            BatteryElectricalObservation {
                voltage_mv: Some(17_721),
                rate_mw: Some(rate),
                power_state: 0x8000_0005,
                temperature_deci_kelvin: None,
                source_issues: Vec::new(),
            }
        );
    }
}

#[test]
fn idle_zero_rate_is_not_replaced_with_inferred_current_or_charge_state() {
    let result = project(SYSTEM_BATTERY, &status(1, 17_721, 0)).unwrap();
    assert_eq!(result.voltage_mv, Some(17_721));
    assert_eq!(result.rate_mw, Some(0));
    assert_eq!(result.power_state, 1);
}

#[test]
fn relative_capacity_excludes_rate_but_not_voltage_or_power_state() {
    let result = project(SYSTEM_BATTERY | CAPACITY_RELATIVE, &status(2, 17_721, -100)).unwrap();
    assert_eq!(result.voltage_mv, Some(17_721));
    assert_eq!(result.rate_mw, None);
    assert_eq!(result.power_state, 2);
}

#[test]
fn unknown_sentinels_are_absent_independently() {
    let result = project(SYSTEM_BATTERY, &status(0, u32::MAX, i32::MIN)).unwrap();
    assert_eq!(result.voltage_mv, None);
    assert_eq!(result.rate_mw, None);
    assert_eq!(
        project(SYSTEM_BATTERY, &status(0, 0, -1))
            .unwrap()
            .voltage_mv,
        Some(0)
    );
    assert_eq!(
        project(SYSTEM_BATTERY, &status(0, u32::MAX, -1))
            .unwrap()
            .rate_mw,
        Some(-1)
    );
    assert_eq!(
        project(SYSTEM_BATTERY, &status(0, 17_721, i32::MIN))
            .unwrap()
            .voltage_mv,
        Some(17_721)
    );
}

#[test]
fn selects_only_non_short_term_system_batteries() {
    assert!(eligible(SYSTEM_BATTERY));
    assert!(eligible(SYSTEM_BATTERY | CAPACITY_RELATIVE));
    for capabilities in [
        0,
        CAPACITY_RELATIVE,
        SHORT_TERM,
        SYSTEM_BATTERY | SHORT_TERM,
    ] {
        assert!(!eligible(capabilities));
        assert!(project(capabilities, &status(0, 17_721, 0)).is_err());
    }
}

#[test]
fn rejects_short_and_oversized_status_replies() {
    for length in 0..16 {
        assert!(project(SYSTEM_BATTERY, &status(0, 17_721, 0)[..length]).is_err());
    }
    assert!(project(SYSTEM_BATTERY, &[0; 17]).is_err());
}

#[test]
fn temperature_retains_documented_raw_tenths_kelvin_without_invented_sentinels() {
    for temperature in [0u32, 2981, u32::MAX] {
        let mut observation = project(SYSTEM_BATTERY, &status(1, u32::MAX, i32::MIN)).unwrap();
        attach_temperature(&mut observation, Ok(temperature.to_le_bytes()));
        assert_eq!(observation.temperature_deci_kelvin, Some(temperature));
        assert_eq!(observation.voltage_mv, None);
        assert_eq!(observation.rate_mw, None);
        assert!(observation.source_issues.is_empty());
    }
}

#[test]
fn unsupported_temperature_keeps_successful_electrical_values() {
    let mut observation = project(SYSTEM_BATTERY, &status(1, 17_721, 0)).unwrap();
    attach_temperature(
        &mut observation,
        Err(std::io::Error::from_raw_os_error(1).into()),
    );
    assert_eq!(observation.temperature_deci_kelvin, None);
    assert_eq!(observation.voltage_mv, Some(17_721));
    assert_eq!(observation.rate_mw, Some(0));
    assert!(observation.source_issues.is_empty());
}

#[test]
fn temperature_failures_retain_error_detail_without_discarding_electrical_values() {
    for code in [2, 5, 87, 122, 1167] {
        let mut observation = project(SYSTEM_BATTERY, &status(1, 17_721, -500)).unwrap();
        let error = std::io::Error::from_raw_os_error(code);
        let detail = error.to_string();
        attach_temperature(&mut observation, Err(error.into()));
        assert_eq!(observation.voltage_mv, Some(17_721));
        assert_eq!(observation.rate_mw, Some(-500));
        assert_eq!(observation.power_state, 1);
        assert_eq!(observation.temperature_deci_kelvin, None);
        assert_eq!(observation.source_issues.len(), 1);
        assert_eq!(observation.source_issues[0].detail, detail);
    }
    let mut observation = project(SYSTEM_BATTERY, &status(1, 17_721, 0)).unwrap();
    attach_temperature(
        &mut observation,
        Err(unavailable("malformed or short battery IOCTL reply")),
    );
    assert_eq!(observation.source_issues.len(), 1);
    assert!(
        observation.source_issues[0]
            .detail
            .contains("malformed or short")
    );
}
