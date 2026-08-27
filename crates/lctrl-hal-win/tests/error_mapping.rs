use lctrl_core::LctrlError;
use lctrl_hal_win::{map_win_error, map_wmi_hresult};

fn assert_error_kind(error: &LctrlError, expected_code: u8, expected_kind: &str) {
    assert_eq!(error.exit_code(), expected_code);
    let json = serde_json::to_value(error.report()).unwrap();
    assert_eq!(json["error"]["kind"], expected_kind);
}

#[test]
fn access_denied_requires_administrator() {
    let error = map_win_error(5, "EnergyDrv");

    assert_error_kind(&error, 5, "permission_denied");
}

#[test]
fn invalid_parameter_marks_feature_unsupported() {
    let error = map_win_error(87, "battery.adapter");

    assert_error_kind(&error, 3, "unsupported");
    let json = serde_json::to_value(error.report()).unwrap();
    assert_eq!(json["error"]["feature"], "battery.adapter");
}

#[test]
fn not_ready_marks_channel_unavailable() {
    let error = map_win_error(21, "EnergyDrv");

    assert_error_kind(&error, 4, "channel_unavailable");
}

#[test]
fn io_device_error_is_firmware_rejection() {
    let error = map_win_error(1117, "EnergyDrv");

    assert_error_kind(&error, 6, "firmware_rejected");
    let json = serde_json::to_value(error.report()).unwrap();
    assert_eq!(json["error"]["detail"], "EnergyDrv: I/O device error");
}

#[test]
fn missing_device_marks_channel_unavailable() {
    let error = map_win_error(2, "EnergyDrv");

    assert_error_kind(&error, 4, "channel_unavailable");
}

#[test]
fn zero_error_on_failure_path_is_an_io_error() {
    let error = map_win_error(0, "EnergyDrv");

    assert_error_kind(&error, 1, "io");
}

#[test]
fn missing_wmi_class_marks_feature_unsupported() {
    let error = map_wmi_hresult(0x8004_1002_u32 as i32, "fan.control");

    assert_error_kind(&error, 3, "unsupported");
}

#[test]
fn wmi_access_denied_requires_administrator() {
    let error = map_wmi_hresult(0x8004_1003_u32 as i32, "bios.set");

    assert_error_kind(&error, 5, "permission_denied");
}

#[test]
fn target_invalid_object_is_unsupported_not_retried() {
    let error = map_wmi_hresult(0x8004_1008_u32 as i32, "perf.fan");

    assert_error_kind(&error, 3, "unsupported");
}

#[test]
fn wmi_invalid_class_marks_feature_unsupported() {
    let error = map_wmi_hresult(0x8004_100f_u32 as i32, "wmi.class");

    assert_error_kind(&error, 3, "unsupported");
}

#[test]
fn unknown_wmi_hresult_keeps_native_code_in_io_message() {
    let error = map_wmi_hresult(0x8000_4005_u32 as i32, "wmi.query");

    assert_error_kind(&error, 1, "io");
    let json = serde_json::to_value(error.report()).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("0x80004005")
    );
}
