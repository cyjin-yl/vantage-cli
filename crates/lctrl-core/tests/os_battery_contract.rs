use lctrl_core::{
    AdapterInfo, BatteryHealth, BatteryTelemetry, ChargeStatus, LctrlError, OsBatterySnapshot,
    SystemPowerStatus,
};

fn snapshot() -> OsBatterySnapshot {
    OsBatterySnapshot {
        remaining_capacity_mwh: Some(95_110),
        full_charge_capacity_mwh: Some(95_110),
        design_capacity_mwh: None,
        voltage_mv: Some(17_715),
        charge_rate_ma: Some(0),
        discharge_rate_ma: Some(0),
        remaining_percent: Some(100),
        cycle_count: Some(224),
        temperature_deci_kelvin: None,
        estimated_runtime_min: None,
        power_online: Some(true),
        charging: Some(false),
        discharging: Some(false),
        critical: Some(false),
    }
}

#[test]
fn snapshot_maps_available_os_fields_into_telemetry() {
    let telemetry = BatteryTelemetry::from_os_snapshot(&snapshot()).unwrap();

    assert_eq!(telemetry.remaining_capacity_mwh, Some(95_110));
    assert_eq!(telemetry.full_charge_capacity_mwh, Some(95_110));
    assert_eq!(telemetry.voltage_mv, Some(17_715));
    assert_eq!(telemetry.remaining_percent, Some(100));
    assert_eq!(telemetry.cycle_count, Some(224));
    assert_eq!(telemetry.charge_status, Some(ChargeStatus::NoActivity));
    assert_eq!(telemetry.charge_completion_time_min, None);
    assert_eq!(telemetry.remaining_time_min, None);
}

#[test]
fn snapshot_reports_discharging_while_on_ac() {
    let mut source = snapshot();
    source.discharging = Some(true);
    source.power_online = Some(true);
    source.charge_rate_ma = Some(0);
    source.discharge_rate_ma = Some(850);

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(
        telemetry.charge_status,
        Some(ChargeStatus::DischargingWithAc)
    );
    assert_eq!(telemetry.current_ma, Some(-850));
}

#[test]
fn snapshot_reports_charging_rate_as_positive_current() {
    let mut source = snapshot();
    source.charging = Some(true);
    source.charge_rate_ma = Some(1200);
    source.discharge_rate_ma = Some(0);

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(telemetry.charge_status, Some(ChargeStatus::Charging));
    assert_eq!(telemetry.current_ma, Some(1200));
}

#[test]
fn snapshot_without_any_rate_reports_no_current() {
    let mut source = snapshot();
    source.charge_rate_ma = None;
    source.discharge_rate_ma = None;

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(telemetry.current_ma, None);
    assert_eq!(telemetry.charge_status, Some(ChargeStatus::NoActivity));
}

#[test]
fn snapshot_keeps_health_unavailable_without_a_design_capacity() {
    let telemetry = BatteryTelemetry::from_os_snapshot(&snapshot()).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, None);
    assert_eq!(telemetry.health, None);
    assert_eq!(telemetry.life_percent, None);
    assert!(!telemetry.rapid_charge_allowed());
}

#[test]
fn snapshot_computes_life_percent_when_design_capacity_is_known() {
    let mut source = snapshot();
    source.design_capacity_mwh = Some(100_000);

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, Some(100_000));
    assert_eq!(telemetry.life_percent, Some(95));
    assert_eq!(telemetry.health, Some(BatteryHealth::Green));
    assert!(telemetry.rapid_charge_allowed());
}

#[test]
fn snapshot_life_percent_rounds_down_and_stays_bounded() {
    let mut source = snapshot();
    source.design_capacity_mwh = Some(200_000);

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(telemetry.life_percent, Some(47));
    assert_eq!(telemetry.health, Some(BatteryHealth::Red));
}

#[test]
fn snapshot_rejects_a_zero_design_capacity() {
    let mut source = snapshot();
    source.design_capacity_mwh = Some(0);

    assert!(matches!(
        BatteryTelemetry::from_os_snapshot(&source),
        Err(LctrlError::FirmwareRejected { .. })
    ));
}

#[test]
fn snapshot_keeps_estimated_runtime_out_of_remaining_time() {
    let mut source = snapshot();
    source.estimated_runtime_min = Some(120);

    let telemetry = BatteryTelemetry::from_os_snapshot(&source).unwrap();

    assert_eq!(telemetry.remaining_time_min, Some(120));
}

#[test]
fn system_power_status_reports_ac_line_state() {
    let status = SystemPowerStatus {
        ac_line_status: Some(true),
        battery_flag: Some(1),
        battery_life_percent: Some(100),
        battery_life_time: None,
        battery_full_life_time: None,
    };

    let adapter = AdapterInfo::from_system_power_status(&status);

    assert_eq!(adapter.ac_connected, Some(true));
    assert_eq!(adapter.status, None);
    assert_eq!(adapter.wattage_w, None);
    assert!(!adapter.has_detail);
}

#[test]
fn system_power_status_reports_a_missing_ac_line() {
    let status = SystemPowerStatus {
        ac_line_status: Some(false),
        battery_flag: Some(3),
        battery_life_percent: Some(80),
        battery_life_time: None,
        battery_full_life_time: None,
    };

    let adapter = AdapterInfo::from_system_power_status(&status);

    assert_eq!(adapter.ac_connected, Some(false));
}

#[test]
fn system_power_status_keeps_unknown_ac_state_unknown() {
    let status = SystemPowerStatus {
        ac_line_status: None,
        battery_flag: None,
        battery_life_percent: None,
        battery_life_time: None,
        battery_full_life_time: None,
    };

    let adapter = AdapterInfo::from_system_power_status(&status);

    assert_eq!(adapter.ac_connected, None);
    assert_eq!(adapter.status, None);
}

#[test]
fn system_power_status_exposes_the_battery_percentage() {
    let status = SystemPowerStatus {
        ac_line_status: Some(true),
        battery_flag: Some(1),
        battery_life_percent: Some(87),
        battery_life_time: None,
        battery_full_life_time: None,
    };

    let percent = status.battery_life_percent();

    assert_eq!(percent, Some(87));
}
