use std::collections::{BTreeMap, VecDeque};

use lctrl_core::{BiosAssistantCapabilities, BiosAssistantValue, LctrlError};
use lctrl_hal::BiosAssistantControl;
use lctrl_hal_win::{WindowsBiosController, WmiObject, WmiTransport, WmiValue};
use parking_lot::Mutex;

#[derive(Clone, Debug, PartialEq)]
struct Invocation {
    class: String,
    path: String,
    method: String,
    input: WmiObject,
}

#[derive(Default)]
struct FakeAssistantWmi {
    instances: Vec<WmiObject>,
    replies: Mutex<VecDeque<WmiObject>>,
    invocations: Mutex<Vec<Invocation>>,
}

impl WmiTransport for FakeAssistantWmi {
    fn query(&self, _namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        if wql.contains("LENOVO_BIOS_ASSISTANT") {
            Ok(self.instances.clone())
        } else {
            Ok(Vec::new())
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        class: &str,
        object_path: &str,
        method: &str,
        input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        self.invocations.lock().push(Invocation {
            class: class.to_string(),
            path: object_path.to_string(),
            method: method.to_string(),
            input: input.clone(),
        });
        self.replies
            .lock()
            .pop_front()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "fake Assistant reply exhausted".into(),
            })
    }
}

fn controller(raw: u32) -> WindowsBiosController<FakeAssistantWmi> {
    WindowsBiosController::new(FakeAssistantWmi {
        instances: vec![BTreeMap::from([(
            "__Path".into(),
            WmiValue::String("assistant-a".into()),
        )])],
        replies: Mutex::new(VecDeque::from([BTreeMap::from([
            ("ReturnValue".into(), WmiValue::Bool(true)),
            ("Data".into(), WmiValue::U32(raw)),
        ])])),
        invocations: Mutex::new(Vec::new()),
    })
}

#[test]
fn capability_decodes_command_status_separately_from_support_bits() {
    let controller = controller(0x8000_0008);

    let capabilities = controller.assistant_capabilities().unwrap();

    assert_eq!(capabilities.raw, 0x8000_0008);
    assert!(capabilities.command_accepted);
    assert!(capabilities.fn_ctrl_swap_supported);
    assert!(!capabilities.super_boost_supported);
    assert_eq!(capabilities.gpu_mode_support, 0);
    assert_eq!(capabilities.major_version, 0);
    assert_eq!(capabilities.minor_version, 0);

    let calls = controller.transport().invocations.lock().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].class, "LENOVO_BIOS_ASSISTANT");
    assert_eq!(calls[0].path, "assistant-a");
    assert_eq!(calls[0].method, "GetCapabilityValue");
    assert!(calls[0].input.is_empty());
}

#[test]
fn raw_capability_decoder_preserves_only_documented_fields() {
    let decoded = BiosAssistantCapabilities::from_raw(0x8057_000f);

    assert!(decoded.command_accepted);
    assert!(decoded.super_boost_supported);
    assert_eq!(decoded.gpu_mode_support, 3);
    assert!(decoded.fn_ctrl_swap_supported);
    assert_eq!(decoded.major_version, 5);
    assert_eq!(decoded.minor_version, 7);

    let rejected = BiosAssistantCapabilities::from_raw(0x0000_0008);
    assert!(!rejected.command_accepted);
    assert!(rejected.fn_ctrl_swap_supported);
    assert!(!rejected.super_boost_supported);
    assert_eq!(rejected.gpu_mode_support, 0);
}

#[test]
fn capability_without_bit31_is_rejected_not_exposed_as_supported() {
    let controller = controller(0x0000_0008);

    assert!(matches!(
        controller.assistant_capabilities(),
        Err(LctrlError::FirmwareRejected { detail })
            if detail.contains("command status") && detail.contains("0x00000008")
    ));
}

#[test]
fn get_value_uses_exact_index_and_decodes_accepted_payload() {
    let controller = controller(0x8000_0001);

    let value = controller.assistant_value(1).unwrap();

    assert_eq!(
        value,
        BiosAssistantValue {
            index_data: 1,
            raw: 0x8000_0001,
            command_accepted: true,
            payload: 1,
        }
    );
    let calls = controller.transport().invocations.lock().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "GetValue");
    assert_eq!(
        calls[0].input.get("IndexData"),
        Some(&WmiValue::U32(1))
    );
    assert!(!calls[0].input.contains_key("ValueData"));
}

#[test]
fn get_value_without_bit31_is_rejected_even_when_payload_is_set() {
    let controller = controller(0x0000_0001);

    assert!(matches!(
        controller.assistant_value(1),
        Err(LctrlError::FirmwareRejected { detail })
            if detail.contains("command status") && detail.contains("0x00000001")
    ));
}
