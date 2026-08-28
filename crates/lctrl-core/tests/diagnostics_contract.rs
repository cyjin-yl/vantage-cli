use lctrl_core::{DiagnosticKind, DiagnosticOutcome, DiagnosticResult, UpdateCapability};

#[test]
fn diagnostics_and_update_states_serialize_truthfully() {
    let result = DiagnosticResult {
        kind: DiagnosticKind::Battery,
        outcome: DiagnosticOutcome::Unavailable,
        detail: "no battery".into(),
    };
    let json = serde_json::to_value(result).unwrap();
    assert_eq!(json["kind"], "battery");
    assert_eq!(json["outcome"], "unavailable");

    let update = serde_json::to_value(UpdateCapability::Unavailable {
        reason: "catalog trust metadata absent".into(),
    })
    .unwrap();
    assert!(
        update["unavailable"]["reason"]
            .as_str()
            .unwrap()
            .contains("trust")
    );
}
