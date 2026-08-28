use lctrl_core::LctrlError;
use std::io::{Error as IoError, ErrorKind};

fn assert_report(error: &LctrlError, expected: serde_json::Value) {
    assert_eq!(serde_json::to_value(error.report()).unwrap(), expected);
}

#[test]
fn exit_codes_follow_the_cli_contract() {
    let cases = [
        (LctrlError::from(IoError::other("io")), 1),
        (
            LctrlError::InvalidArgument {
                detail: "bad value".into(),
            },
            2,
        ),
        (
            LctrlError::Unsupported {
                feature: "feature".into(),
            },
            3,
        ),
        (
            LctrlError::ChannelUnavailable {
                channel: "channel".into(),
            },
            4,
        ),
        (
            LctrlError::PermissionDenied {
                need: "administrator".into(),
            },
            5,
        ),
        (
            LctrlError::FirmwareRejected {
                detail: "status".into(),
            },
            6,
        ),
        (
            LctrlError::VerifyMismatch {
                requested: "requested".into(),
                actual: "actual".into(),
            },
            7,
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.exit_code(), expected, "{error}");
    }
}

#[test]
fn unsupported_error_has_stable_json_shape() {
    let error = LctrlError::Unsupported {
        feature: "battery.thresholds".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "unsupported",
                "message": "feature is unsupported: battery.thresholds",
                "feature": "battery.thresholds"
            }
        }),
    );
}

#[test]
fn channel_unavailable_error_has_stable_json_shape() {
    let error = LctrlError::ChannelUnavailable {
        channel: "wmi/ACPI".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "channel_unavailable",
                "message": "channel is unavailable: wmi/ACPI",
                "channel": "wmi/ACPI"
            }
        }),
    );
}

#[test]
fn permission_denied_error_has_stable_json_shape() {
    let error = LctrlError::PermissionDenied {
        need: "root".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "permission_denied",
                "message": "permission denied; requires root",
                "need": "root"
            }
        }),
    );
}

#[test]
fn firmware_rejected_error_has_stable_json_shape() {
    let error = LctrlError::FirmwareRejected {
        detail: "status word non-zero".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "firmware_rejected",
                "message": "firmware rejected request: status word non-zero",
                "detail": "status word non-zero"
            }
        }),
    );
}

#[test]
fn invalid_argument_error_has_stable_json_shape() {
    let error = LctrlError::InvalidArgument {
        detail: "thresholds 95<60".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "invalid_argument",
                "message": "invalid argument: thresholds 95<60",
                "detail": "thresholds 95<60"
            }
        }),
    );
}

#[test]
fn verify_mismatch_error_has_stable_json_shape() {
    let error = LctrlError::VerifyMismatch {
        requested: "performance".into(),
        actual: "cool".into(),
    };

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "verify_mismatch",
                "message": "readback mismatch: requested performance, actual cool",
                "actual": "cool",
                "requested": "performance"
            }
        }),
    );
}

#[test]
fn io_error_has_stable_json_shape_without_context() {
    let error = LctrlError::from(IoError::new(ErrorKind::NotFound, "no such device"));

    assert_report(
        &error,
        serde_json::json!({
            "error": {
                "kind": "io",
                "message": "no such device"
            }
        }),
    );
}
