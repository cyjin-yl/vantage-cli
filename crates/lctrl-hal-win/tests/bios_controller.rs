use std::collections::BTreeMap;

use lctrl_core::{BiosChange, BiosItem, BiosPasswordStatus, LctrlError};
use lctrl_hal::BiosControl;
use lctrl_hal_win::{WindowsBiosController, WmiObject, WmiTransport, WmiValue};
use parking_lot::Mutex;

#[derive(Clone, Debug, PartialEq)]
struct Invocation {
    namespace: String,
    class: String,
    path: String,
    method: String,
    input: WmiObject,
}

#[derive(Default)]
struct FakeWmi {
    bios_settings: Vec<WmiObject>,
    set_instances: Vec<WmiObject>,
    save_instances: Vec<WmiObject>,
    discard_instances: Vec<WmiObject>,
    selection_instances: Vec<WmiObject>,
    password_settings: Vec<WmiObject>,
    method_replies: Mutex<Vec<WmiObject>>,
    queries: Mutex<Vec<(String, String)>>,
    invocations: Mutex<Vec<Invocation>>,
}

impl FakeWmi {
    fn invocations(&self) -> Vec<Invocation> {
        self.invocations.lock().clone()
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        self.queries
            .lock()
            .push((namespace.to_string(), wql.to_string()));
        let objects = if wql.contains("Lenovo_BiosSetting") {
            self.bios_settings.clone()
        } else if wql.contains("Lenovo_SetBiosSetting") {
            self.set_instances.clone()
        } else if wql.contains("Lenovo_SaveBiosSettings") {
            self.save_instances.clone()
        } else if wql.contains("Lenovo_DiscardBiosSettings") {
            self.discard_instances.clone()
        } else if wql.contains("Lenovo_GetBiosSelections") {
            self.selection_instances.clone()
        } else if wql.contains("Lenovo_BiosPasswordSettings") {
            self.password_settings.clone()
        } else {
            Vec::new()
        };
        Ok(objects)
    }

    fn invoke_instance(
        &self,
        namespace: &str,
        class: &str,
        object_path: &str,
        method: &str,
        input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        self.invocations.lock().push(Invocation {
            namespace: namespace.to_string(),
            class: class.to_string(),
            path: object_path.to_string(),
            method: method.to_string(),
            input: input.clone(),
        });
        self.method_replies
            .lock()
            .pop()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "fake WMI reply exhausted".into(),
            })
    }
}

fn method_reply(accepted: bool, return_value: &str) -> WmiObject {
    BTreeMap::from([
        ("ReturnValue".into(), WmiValue::Bool(accepted)),
        ("return".into(), WmiValue::String(return_value.into())),
    ])
}

fn path(path: &str) -> WmiObject {
    BTreeMap::from([("__Path".into(), WmiValue::String(path.into()))])
}

#[test]
fn list_skips_empty_and_malformed_current_setting_rows() {
    let transport = FakeWmi {
        bios_settings: vec![
            BTreeMap::from([(
                "CurrentSetting".into(),
                WmiValue::String(" Camera , Enable ".into()),
            )]),
            BTreeMap::from([("CurrentSetting".into(), WmiValue::String(String::new()))]),
            BTreeMap::from([(
                "CurrentSetting".into(),
                WmiValue::String("malformed".into()),
            )]),
            BTreeMap::from([("CurrentSetting".into(), WmiValue::U32(7))]),
            BTreeMap::from([(
                "CurrentSetting".into(),
                WmiValue::String("SecureBoot,Disable,ignored".into()),
            )]),
        ],
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    assert_eq!(
        controller.list().unwrap(),
        vec![
            BiosItem {
                name: "Camera".into(),
                value: "Enable".into(),
                selections: Vec::new(),
            },
            BiosItem {
                name: "SecureBoot".into(),
                value: "Disable,ignored".into(),
                selections: Vec::new(),
            },
        ]
    );
}

#[test]
fn selections_trim_and_drop_empty_entries() {
    let transport = FakeWmi {
        selection_instances: vec![path("selection-a")],
        method_replies: Mutex::new(vec![BTreeMap::from([
            ("ReturnValue".into(), WmiValue::Bool(true)),
            (
                "Selections".into(),
                WmiValue::String(" , Enable, Disable ,  ,".into()),
            ),
        ])]),
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    assert_eq!(
        controller.selections(" Camera ").unwrap(),
        vec!["Enable".to_string(), "Disable".to_string()]
    );
    let calls = controller.transport().invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, "selection-a");
    assert_eq!(
        calls[0].input.get("Item"),
        Some(&WmiValue::String(" Camera ".into()))
    );
}

#[test]
fn get_returns_none_for_an_absent_item() {
    let transport = FakeWmi {
        bios_settings: vec![BTreeMap::from([(
            "CurrentSetting".into(),
            WmiValue::String("Camera,Enable".into()),
        )])],
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    assert_eq!(controller.get("Missing").unwrap(), None);
    assert_eq!(controller.get("camera").unwrap().unwrap().name, "Camera");
}

#[test]
fn stage_uses_first_valid_method_path_and_exact_payload() {
    let transport = FakeWmi {
        set_instances: vec![BTreeMap::new(), path("set-a"), path("set-b")],
        method_replies: Mutex::new(vec![method_reply(true, "Success")]),
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    controller
        .stage(BiosChange::new("IntegratedCamera", "Disable").unwrap())
        .unwrap();

    let calls = controller.transport().invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].namespace, "ROOT\\WMI");
    assert_eq!(calls[0].class, "Lenovo_SetBiosSetting");
    assert_eq!(calls[0].path, "set-a");
    assert_eq!(calls[0].method, "SetBiosSetting");
    assert_eq!(
        calls[0].input.get("parameter"),
        Some(&WmiValue::String("IntegratedCamera,Disable;".into()))
    );
}

#[test]
fn save_uses_exact_semicolon_parameter() {
    let transport = FakeWmi {
        save_instances: vec![path("save-a")],
        method_replies: Mutex::new(vec![method_reply(true, "Success")]),
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    controller.save().unwrap();

    let calls = controller.transport().invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].class, "Lenovo_SaveBiosSettings");
    assert_eq!(calls[0].path, "save-a");
    assert_eq!(calls[0].method, "SaveBiosSettings");
    assert_eq!(
        calls[0].input.get("parameter"),
        Some(&WmiValue::String(";".into()))
    );
}

#[test]
fn discard_uses_verified_transaction_method() {
    let transport = FakeWmi {
        discard_instances: vec![path("discard-a")],
        method_replies: Mutex::new(vec![method_reply(true, "Success")]),
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    controller.discard().unwrap();

    let calls = controller.transport().invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].class, "Lenovo_DiscardBiosSettings");
    assert_eq!(calls[0].method, "DiscardBiosSettings");
    assert_eq!(
        calls[0].input.get("parameter"),
        Some(&WmiValue::String(";".into()))
    );
}

#[test]
fn stage_rejects_false_return_value_and_non_success_business_return() {
    for reply in [method_reply(false, "Success"), method_reply(true, "Denied")] {
        let transport = FakeWmi {
            set_instances: vec![path("set-a")],
            method_replies: Mutex::new(vec![reply]),
            ..FakeWmi::default()
        };
        let controller = WindowsBiosController::new(transport);

        assert!(matches!(
            controller.stage(BiosChange::new("Camera", "Enable").unwrap()),
            Err(LctrlError::FirmwareRejected { .. })
        ));
    }
}

#[test]
fn password_status_decodes_supervisor_bit_one() {
    let transport = FakeWmi {
        password_settings: vec![BTreeMap::from([
            ("MinLength".into(), WmiValue::U32(1)),
            ("MaxLength".into(), WmiValue::U32(128)),
            ("PasswordState".into(), WmiValue::U32(2)),
        ])],
        ..FakeWmi::default()
    };
    let controller = WindowsBiosController::new(transport);

    assert_eq!(
        controller.password_status().unwrap(),
        BiosPasswordStatus::from_raw(1, 128, 2)
    );
}
