#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use std::sync::{Arc, Mutex};

use lctrl_core::{ApplyMode, LctrlError, Result};
use lctrl_hal::PanelBrightnessControl;
use lctrl_hal_win::{WindowsPanelBrightness, WmiObject, WmiTransport, WmiValue};

#[derive(Clone, Copy)]
enum Action {
    Success,
    NoOutputApplied,
    NoOutputUnchanged,
    FailAfterEffect,
    ThirdParty,
    Lost,
    Changed,
    FailWithoutEffect,
    BadReturn,
    Rejected,
    ObservationFailed,
}

struct State {
    brightness: Vec<WmiObject>,
    connections: Vec<WmiObject>,
    methods: Vec<WmiObject>,
    actions: Vec<Action>,
    calls: Vec<(String, WmiObject)>,
    observation_failed: bool,
}

#[derive(Clone)]
struct Fake(Arc<Mutex<State>>);

fn row(name: &str) -> WmiObject {
    WmiObject::from([
        ("Active".into(), WmiValue::Bool(true)),
        ("InstanceName".into(), WmiValue::String(name.into())),
    ])
}
fn brightness(name: &str, current: u8) -> WmiObject {
    let mut row = row(name);
    row.insert("CurrentBrightness".into(), WmiValue::U8(current));
    row.insert("Levels".into(), WmiValue::U32(3));
    row.insert(
        "Level".into(),
        WmiValue::Array(vec![WmiValue::U8(20), WmiValue::U8(50), WmiValue::U8(100)]),
    );
    row
}
fn connection(name: &str, technology: u32) -> WmiObject {
    let mut row = row(name);
    row.insert("VideoOutputTechnology".into(), WmiValue::U32(technology));
    row
}
fn fixture(actions: Vec<Action>) -> Fake {
    Fake(Arc::new(Mutex::new(State {
        brightness: vec![brightness("panel\\\"key", 100)],
        connections: vec![connection("panel\\\"key", 11)],
        methods: vec![row("panel\\\"key")],
        actions,
        calls: Vec::new(),
        observation_failed: false,
    })))
}
impl WmiTransport for Fake {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        assert_eq!(namespace, "ROOT\\WMI");
        let state = self.0.lock().unwrap();
        if state.observation_failed {
            return Err(LctrlError::ChannelUnavailable {
                channel: "injected observation failure".into(),
            });
        }
        if wql.ends_with("FROM WmiMonitorBrightness") {
            Ok(state.brightness.clone())
        } else if wql.ends_with("FROM WmiMonitorConnectionParams") {
            Ok(state.connections.clone())
        } else if wql.ends_with("FROM WmiMonitorBrightnessMethods") {
            Ok(state.methods.clone())
        } else {
            panic!("unexpected query")
        }
    }
    fn invoke_instance(
        &self,
        namespace: &str,
        class: &str,
        path: &str,
        method: &str,
        input: &WmiObject,
    ) -> Result<WmiObject> {
        assert_eq!(
            (namespace, class, method),
            (
                "ROOT\\WMI",
                "WmiMonitorBrightnessMethods",
                "WmiSetBrightness"
            )
        );
        let mut state = self.0.lock().unwrap();
        state.calls.push((path.into(), input.clone()));
        let value = match input.get("Brightness") {
            Some(WmiValue::U8(value)) => *value,
            _ => panic!("wrong value type"),
        };
        assert_eq!(input.get("Timeout"), Some(&WmiValue::U32(0)));
        let action = state.actions.remove(0);
        match action {
            Action::Success | Action::NoOutputApplied | Action::FailAfterEffect => {
                state.brightness[0].insert("CurrentBrightness".into(), WmiValue::U8(value));
            }
            Action::ThirdParty => {
                state.brightness[0].insert("CurrentBrightness".into(), WmiValue::U8(20));
            }
            Action::Lost => state.brightness.clear(),
            Action::Changed => {
                state.brightness = vec![brightness("replacement", value)];
                state.connections = vec![connection("replacement", 11)];
                state.methods = vec![row("replacement")];
            }
            Action::FailWithoutEffect
            | Action::BadReturn
            | Action::Rejected
            | Action::NoOutputUnchanged => {}
            Action::ObservationFailed => state.observation_failed = true,
        }
        match action {
            Action::FailAfterEffect
            | Action::ThirdParty
            | Action::Lost
            | Action::Changed
            | Action::ObservationFailed => Err(LctrlError::ChannelUnavailable {
                channel: "injected failure".into(),
            }),
            Action::FailWithoutEffect => Err(LctrlError::PermissionDenied {
                need: "injected permission".into(),
            }),
            Action::BadReturn => Ok(WmiObject::from([(
                "ReturnValue".into(),
                WmiValue::Bool(true),
            )])),
            Action::Rejected => Ok(WmiObject::from([("ReturnValue".into(), WmiValue::U32(5))])),
            Action::Success => Ok(WmiObject::from([("ReturnValue".into(), WmiValue::U32(0))])),
            Action::NoOutputApplied | Action::NoOutputUnchanged => Ok(WmiObject::new()),
        }
    }
}

#[test]
fn selects_only_exact_matching_internal_and_rejects_ambiguity() {
    for technology in [0x8000_0000, 11, 13] {
        let fake = fixture(vec![]);
        fake.0.lock().unwrap().connections[0] = connection("panel\\\"key", technology);
        assert_eq!(
            WindowsPanelBrightness::new(fake)
                .brightness()
                .unwrap()
                .current_percent,
            100
        );
    }
    let fake = fixture(vec![]);
    {
        let mut s = fake.0.lock().unwrap();
        s.brightness.push(brightness("external", 50));
        s.connections.push(connection("external", 10));
        s.methods.push(row("external"));
    }
    assert_eq!(
        WindowsPanelBrightness::new(fake.clone())
            .brightness()
            .unwrap()
            .current_percent,
        100
    );
    fake.0.lock().unwrap().connections[1] = connection("external", 13);
    assert!(
        WindowsPanelBrightness::new(fake.clone())
            .brightness()
            .is_err()
    );
    fake.0.lock().unwrap().connections[1] = connection("external", 0xffff_ffff);
    fake.0.lock().unwrap().connections[0] = connection("different", 11);
    assert!(WindowsPanelBrightness::new(fake).brightness().is_err());
}

#[test]
fn rejects_bad_levels_duplicates_and_untrusted_identities() {
    for levels in [vec![], vec![20, 20, 100], vec![20, 50, 101]] {
        let fake = fixture(vec![]);
        fake.0.lock().unwrap().brightness[0].insert(
            "Level".into(),
            WmiValue::Array(levels.into_iter().map(WmiValue::U8).collect()),
        );
        assert!(WindowsPanelBrightness::new(fake).brightness().is_err());
    }
    let fake = fixture(vec![]);
    fake.0
        .lock()
        .unwrap()
        .connections
        .push(connection("panel\\\"key", 11));
    assert!(WindowsPanelBrightness::new(fake).brightness().is_err());
    let fake = fixture(vec![]);
    fake.0.lock().unwrap().brightness[0]
        .insert("InstanceName".into(), WmiValue::String("\0".into()));
    assert!(WindowsPanelBrightness::new(fake).brightness().is_err());
    let fake = fixture(vec![]);
    fake.0.lock().unwrap().connections[0] = connection("panel\\\"key", 0);
    assert!(WindowsPanelBrightness::new(fake).brightness().is_err());
}

#[test]
fn dry_run_and_invalid_value_never_invoke() {
    let fake = fixture(vec![]);
    let control = WindowsPanelBrightness::new(fake.clone());
    let report = control.set_brightness(50, None, ApplyMode::DryRun).unwrap();
    assert_eq!(
        (*report.previous(), *report.requested(), report.actual()),
        (100, 50, None)
    );
    assert!(control.set_brightness(51, None, ApplyMode::Commit).is_err());
    assert!(fake.0.lock().unwrap().calls.is_empty());
}

#[test]
fn stale_expected_brightness_refuses_before_setter() {
    let fake = fixture(vec![]);
    let control = WindowsPanelBrightness::new(fake.clone());
    for apply in [ApplyMode::DryRun, ApplyMode::Commit] {
        assert!(matches!(
            control.set_brightness(50, Some(80), apply),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }
    assert_eq!(control.brightness().unwrap().current_percent, 100);
    assert!(fake.0.lock().unwrap().calls.is_empty());
}

#[test]
fn read_remains_available_without_a_matching_setter() {
    let fake = fixture(vec![]);
    fake.0.lock().unwrap().methods.clear();
    let control = WindowsPanelBrightness::new(fake.clone());
    assert_eq!(control.brightness().unwrap().current_percent, 100);
    assert!(control.set_brightness(50, None, ApplyMode::DryRun).is_err());
    assert!(fake.0.lock().unwrap().calls.is_empty());
}

#[test]
fn commits_with_escaped_identity_and_uint32_result() {
    let fake = fixture(vec![Action::Success]);
    fake.0.lock().unwrap().methods[0].insert("__Path".into(), WmiValue::String("unrelated".into()));
    let report = WindowsPanelBrightness::new(fake.clone())
        .set_brightness(50, Some(100), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&50));
    let state = fake.0.lock().unwrap();
    assert_eq!(
        state.calls[0].0,
        "WmiMonitorBrightnessMethods.InstanceName=\"panel\\\\\\\"key\""
    );
    assert_eq!(state.calls[0].1.get("Brightness"), Some(&WmiValue::U8(50)));
}

#[test]
fn failure_after_effect_restores_only_requested_value() {
    let fake = fixture(vec![Action::FailAfterEffect, Action::Success]);
    let error = WindowsPanelBrightness::new(fake.clone())
        .set_brightness(50, None, ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    let state = fake.0.lock().unwrap();
    assert_eq!(state.calls.len(), 2);
    assert_eq!(state.calls[1].1.get("Brightness"), Some(&WmiValue::U8(100)));
    assert_eq!(
        state.brightness[0].get("CurrentBrightness"),
        Some(&WmiValue::U8(100))
    );
}

#[test]
fn third_party_or_lost_or_changed_target_is_never_overwritten() {
    for action in [
        Action::ThirdParty,
        Action::Lost,
        Action::Changed,
        Action::ObservationFailed,
    ] {
        let fake = fixture(vec![action]);
        let error = WindowsPanelBrightness::new(fake.clone())
            .set_brightness(50, None, ApplyMode::Commit)
            .unwrap_err();
        assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
        assert_eq!(fake.0.lock().unwrap().calls.len(), 1);
    }
}

#[test]
fn setter_requires_uint32_zero_return() {
    for action in [Action::BadReturn, Action::Rejected] {
        let fake = fixture(vec![action]);
        let error = WindowsPanelBrightness::new(fake.clone())
            .set_brightness(50, None, ApplyMode::Commit)
            .unwrap_err();
        assert!(matches!(
            error,
            LctrlError::ChannelUnavailable { .. } | LctrlError::FirmwareRejected { .. }
        ));
        assert_eq!(fake.0.lock().unwrap().calls.len(), 1);
    }
}

#[test]
fn outputless_setter_requires_observed_effect() {
    let applied = fixture(vec![Action::NoOutputApplied, Action::Success]);
    let report = WindowsPanelBrightness::new(applied.clone())
        .set_brightness(50, Some(100), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&50));
    assert_eq!(
        applied.0.lock().unwrap().brightness[0].get("CurrentBrightness"),
        Some(&WmiValue::U8(50))
    );

    let unchanged = fixture(vec![Action::NoOutputUnchanged]);
    assert!(matches!(
        WindowsPanelBrightness::new(unchanged.clone()).set_brightness(
            50,
            Some(100),
            ApplyMode::Commit
        ),
        Err(LctrlError::VerifyMismatch { .. })
    ));
    assert_eq!(unchanged.0.lock().unwrap().calls.len(), 1);
    assert_eq!(
        unchanged.0.lock().unwrap().brightness[0].get("CurrentBrightness"),
        Some(&WmiValue::U8(100))
    );
}

#[test]
fn failed_recovery_retains_both_errors_and_marks_unverified() {
    let fake = fixture(vec![Action::FailAfterEffect, Action::FailWithoutEffect]);
    let error = WindowsPanelBrightness::new(fake.clone())
        .set_brightness(50, None, ApplyMode::Commit)
        .unwrap_err()
        .to_string();
    assert!(error.contains("channel is unavailable"));
    assert!(error.contains("permission denied"));
    assert_eq!(fake.0.lock().unwrap().calls.len(), 2);
}

#[test]
fn failed_set_with_previous_observed_does_not_write_again() {
    let fake = fixture(vec![Action::FailWithoutEffect]);
    assert!(
        WindowsPanelBrightness::new(fake.clone())
            .set_brightness(50, None, ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(fake.0.lock().unwrap().calls.len(), 1);
}

#[test]
fn transport_permission_errors_remain_permission_errors_without_identity_leak() {
    struct Denied;
    impl WmiTransport for Denied {
        fn query(&self, _: &str, _: &str) -> Result<Vec<WmiObject>> {
            Err(LctrlError::PermissionDenied {
                need: "private-monitor-identity".into(),
            })
        }
        fn invoke_instance(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &WmiObject,
        ) -> Result<WmiObject> {
            unreachable!()
        }
    }
    let error = WindowsPanelBrightness::new(Denied)
        .brightness()
        .unwrap_err();
    assert!(matches!(error, LctrlError::PermissionDenied { .. }));
    assert!(!error.to_string().contains("private-monitor-identity"));
}
