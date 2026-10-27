use std::time::Duration;

use lctrl_core::{
    DiagnosticCancellationReason, DiagnosticCancellationToken, DiagnosticKind,
    DiagnosticMeasurement, DiagnosticOutcome, DiagnosticReason, DiagnosticResult,
    DiagnosticRunOptions, DiagnosticThreshold,
};

#[test]
fn cancellation_clones_share_the_first_structured_reason() {
    let token = DiagnosticCancellationToken::new();
    let clone = token.clone();
    clone.cancel();
    token.cancel_with_reason(DiagnosticCancellationReason::DeadlineExceeded);
    assert!(token.is_cancelled());
    assert_eq!(
        token.reason(),
        Some(DiagnosticCancellationReason::Requested)
    );
}

#[test]
fn options_have_a_finite_default_and_explicit_per_item_deadline() {
    assert!(!DiagnosticRunOptions::default().per_item_deadline.is_zero());
    let token = DiagnosticCancellationToken::new();
    let options = DiagnosticRunOptions::new(token.clone(), Duration::from_millis(25));
    token.cancel();
    assert!(options.cancellation.is_cancelled());
    assert_eq!(options.per_item_deadline, Duration::from_millis(25));
}

#[test]
fn result_keeps_measurement_threshold_source_and_reason_structured() {
    let result = DiagnosticResult::new(
        DiagnosticKind::Thermal,
        DiagnosticOutcome::Failed,
        "critical trip point reached",
    )
    .with_measurement(DiagnosticMeasurement::new("CurrentTemperature", 80.0).with_unit("degC"))
    .with_threshold(DiagnosticThreshold::at_or_above("CriticalTripPoint", 80.0).with_unit("degC"))
    .with_source("WMI ROOT\\WMI MSAcpi_ThermalZoneTemperature")
    .with_reason(DiagnosticReason::ThresholdExceeded);
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["measurements"][0]["value"], 80.0);
    assert_eq!(json["threshold"]["comparison"], "at_or_above");
    assert_eq!(json["reason"], "threshold_exceeded");
    for (outcome, expected) in [
        (DiagnosticOutcome::Passed, "passed"),
        (DiagnosticOutcome::Failed, "failed"),
        (DiagnosticOutcome::Skipped, "skipped"),
        (DiagnosticOutcome::Warning, "warning"),
        (DiagnosticOutcome::Unavailable, "unavailable"),
        (DiagnosticOutcome::Cancelled, "cancelled"),
        (DiagnosticOutcome::ResourceError, "resource_error"),
    ] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), expected);
    }
}
