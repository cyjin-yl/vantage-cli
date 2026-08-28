use std::collections::BTreeMap;

use lctrl_core::{ApplyMode, LightingEffect, RefreshMode};
use lctrl_hal::{KeyboardControl, PanelControl};
use lctrl_hal_win::{WindowsPeripheralController, WmiObject, WmiTransport, WmiValue};
use parking_lot::Mutex;

#[derive(Default)]
struct FakeWmi {
    replies: Mutex<Vec<WmiObject>>,
    invocations: Mutex<Vec<(String, WmiObject)>>,
}

impl FakeWmi {
    fn new(replies: impl IntoIterator<Item = WmiObject>) -> Self {
        let mut replies: Vec<_> = replies.into_iter().collect();
        replies.reverse();
        Self {
            replies: Mutex::new(replies),
            invocations: Mutex::new(Vec::new()),
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, _namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        if wql.contains("LENOVO_LIGHTING_DATA") {
            return Ok(vec![BTreeMap::from([
                ("Lighting_Type".into(), WmiValue::U8(1)),
                ("State_Type_Num".into(), WmiValue::U8(4)),
            ])]);
        }
        if wql.contains("LENOVO_LIGHTING_METHOD") {
            return Ok(vec![BTreeMap::from([
                ("__Path".into(), WmiValue::String("lighting-path".into())),
                ("Active".into(), WmiValue::Bool(true)),
            ])]);
        }
        if wql.contains("LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA") {
            return Ok(vec![BTreeMap::from([
                ("MinimumRefreshRate".into(), WmiValue::U16(60)),
                ("MaximumRefreshRate".into(), WmiValue::U16(120)),
                ("DefaultRefreshRate".into(), WmiValue::U16(60)),
                ("Mode".into(), WmiValue::U16(1)),
            ])]);
        }
        Ok(Vec::new())
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _path: &str,
        method: &str,
        input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        self.invocations
            .lock()
            .push((method.to_string(), input.clone()));
        self.replies
            .lock()
            .pop()
            .ok_or_else(|| lctrl_core::LctrlError::ChannelUnavailable {
                channel: "fake WMI reply exhausted".into(),
            })
    }
}

fn getter(level: u8, effect: u8) -> WmiObject {
    BTreeMap::from([
        ("ReturnValue".into(), WmiValue::Bool(true)),
        ("Current_Brightness_Level".into(), WmiValue::U8(level)),
        ("Current_State_Type".into(), WmiValue::U8(effect)),
    ])
}

fn setter() -> WmiObject {
    BTreeMap::from([("ReturnValue".into(), WmiValue::Bool(true))])
}

#[test]
fn backlight_dry_run_reads_but_never_sets() {
    let controller = WindowsPeripheralController::new(FakeWmi::new([getter(1, 0)]));

    let report = controller
        .set_backlight(2, LightingEffect::Static, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.previous().level, 1);
    assert_eq!(report.requested().level, 2);
    assert_eq!(report.actual(), None);
    assert_eq!(controller.transport().invocations.lock().len(), 1);
}

#[test]
fn backlight_set_uses_exact_typed_fields_and_readback() {
    let controller =
        WindowsPeripheralController::new(FakeWmi::new([getter(1, 0), setter(), getter(2, 0)]));

    let report = controller
        .set_backlight(2, LightingEffect::Static, ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual().unwrap().level, 2);
    let calls = controller.transport().invocations.lock();
    assert_eq!(calls[1].0, "Set_Lighting_Current_Status");
    assert_eq!(
        calls[1].1.get("Current_Brightness_Level"),
        Some(&WmiValue::U8(2))
    );
    assert_eq!(calls[1].1.get("Current_State_Type"), Some(&WmiValue::U8(0)));
    assert_eq!(calls[1].1.get("Lighting_ID"), Some(&WmiValue::U8(0)));
}

#[test]
fn single_color_keyboard_rejects_non_static_effect_before_set() {
    let controller = WindowsPeripheralController::new(FakeWmi::new([getter(1, 0)]));

    assert!(
        controller
            .set_backlight(2, LightingEffect::Breathing, ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(controller.transport().invocations.lock().len(), 1);
}

#[test]
fn panel_refresh_is_read_only_and_preserves_mode() {
    let controller = WindowsPeripheralController::new(FakeWmi::default());

    let capability = controller.refresh_capability().unwrap();
    assert_eq!(capability.min_hz, 60);
    assert_eq!(capability.max_hz, 120);
    assert_eq!(capability.default_hz, 60);
    assert_eq!(controller.refresh_mode().unwrap(), RefreshMode::Adaptive);
}
