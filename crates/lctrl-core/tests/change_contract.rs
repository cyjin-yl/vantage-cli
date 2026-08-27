use lctrl_core::{ApplyMode, ChangeReport};

#[test]
fn dry_run_report_omits_actual_value() {
    let report = ChangeReport::dry_run("normal", "conservation");

    assert_eq!(report.mode(), ApplyMode::DryRun);
    assert_eq!(report.previous(), &"normal");
    assert_eq!(report.requested(), &"conservation");
    assert_eq!(report.actual(), None);
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::json!({
            "mode": "dry_run",
            "previous": "normal",
            "requested": "conservation"
        })
    );
}

#[test]
fn committed_report_contains_readback_value() {
    let report = ChangeReport::committed("normal", "conservation", "conservation");

    assert_eq!(report.mode(), ApplyMode::Commit);
    assert_eq!(report.previous(), &"normal");
    assert_eq!(report.requested(), &"conservation");
    assert_eq!(report.actual(), Some(&"conservation"));
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::json!({
            "mode": "commit",
            "previous": "normal",
            "requested": "conservation",
            "actual": "conservation"
        })
    );
}

#[test]
fn apply_mode_values_are_stable_snake_case() {
    assert_eq!(
        serde_json::to_string(&ApplyMode::DryRun).unwrap(),
        r#""dry_run""#
    );
    assert_eq!(
        serde_json::to_string(&ApplyMode::Commit).unwrap(),
        r#""commit""#
    );
}
