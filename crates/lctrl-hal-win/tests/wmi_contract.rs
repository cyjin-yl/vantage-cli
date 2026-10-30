use std::collections::BTreeMap;

use parking_lot::Mutex;

use lctrl_core::LctrlError;
use lctrl_hal::ControlConflictDetection;
use lctrl_hal_win::{
    WindowsControlConflictDetector, WmiMethodResult, WmiObject, WmiTransport, WmiValue,
    active_instance,
};

fn output(accepted: bool, data: WmiValue) -> WmiObject {
    BTreeMap::from([
        ("ReturnValue".into(), WmiValue::Bool(accepted)),
        ("Data".into(), data),
    ])
}

#[test]
fn setter_acceptance_requires_true_and_zero_status() {
    let result = WmiMethodResult::parse(output(true, WmiValue::U32(0))).unwrap();

    assert_eq!(result.require_zero_status("kbd.backlight").unwrap(), 0);
}

#[test]
fn setter_nonzero_status_is_firmware_rejection() {
    let result = WmiMethodResult::parse(output(true, WmiValue::U32(42))).unwrap();

    let error = result
        .require_zero_status("kbd.backlight")
        .expect_err("nonzero status must fail");
    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { detail } if detail.contains("42")
    ));
}

#[test]
fn false_return_value_is_firmware_rejection() {
    let result = WmiMethodResult::parse(output(false, WmiValue::U32(0))).unwrap();

    assert!(matches!(
        result.require_accepted("panel.rate"),
        Err(LctrlError::FirmwareRejected { .. })
    ));
}

#[test]
fn getter_may_return_nonzero_data_as_value() {
    let result = WmiMethodResult::parse(output(true, WmiValue::U32(42))).unwrap();

    result.require_accepted("utility.version").unwrap();
    assert_eq!(result.data_u32().unwrap(), 42);
}

#[test]
fn malformed_return_value_is_channel_error() {
    let malformed = BTreeMap::from([("ReturnValue".into(), WmiValue::U32(1))]);

    assert!(matches!(
        WmiMethodResult::parse(malformed),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(matches!(
        WmiMethodResult::parse(BTreeMap::new()),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn parse_accepts_void_lenovo_bios_business_result() {
    let output = BTreeMap::from([
        ("return".into(), WmiValue::String("Success".into())),
        (
            "Selections".into(),
            WmiValue::String("Enable,Disable".into()),
        ),
    ]);
    let result = WmiMethodResult::parse(output).unwrap();
    result.require_accepted("bios.selections").unwrap();
    assert_eq!(
        result.output().get("Selections"),
        Some(&WmiValue::String("Enable,Disable".into()))
    );
}

#[test]
fn parse_accepts_declared_out_params_without_return_marker() {
    // On 21VG the raw `ExecMethod` out-params object for
    // `Lenovo_GetBiosSelections` carries only `Selections`; Invoke-CimMethod
    // synthesizes `ReturnValue` but `IWbemServices::ExecMethod` does not.
    let output = BTreeMap::from([(
        "Selections".into(),
        WmiValue::String("Enable,Disable".into()),
    )]);
    let result = WmiMethodResult::parse(output).unwrap();
    result.require_accepted("bios.selections").unwrap();
    assert_eq!(
        result.output().get("Selections"),
        Some(&WmiValue::String("Enable,Disable".into()))
    );
}

#[test]
fn wrong_data_type_is_channel_error() {
    let result = WmiMethodResult::parse(output(true, WmiValue::String("42".into()))).unwrap();

    assert!(matches!(
        result.data_u32(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[derive(Default)]
struct FakeTransport {
    objects: Vec<WmiObject>,
    queries: Mutex<Vec<(String, String)>>,
}

impl WmiTransport for FakeTransport {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        self.queries.lock().push((namespace.into(), wql.into()));
        Ok(self.objects.clone())
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        unreachable!("active_instance does not invoke methods")
    }
}

#[test]
fn active_instance_uses_root_wmi_and_binds_path() {
    let transport = FakeTransport {
        objects: vec![BTreeMap::from([
            (
                "__Path".into(),
                WmiValue::String("LENOVO_UTILITY_DATA.InstanceName=\"ACPI\\VPC\"".into()),
            ),
            ("Active".into(), WmiValue::Bool(true)),
        ])],
        ..Default::default()
    };

    let instance = active_instance(&transport, "LENOVO_UTILITY_DATA").unwrap();

    assert_eq!(instance.class(), "LENOVO_UTILITY_DATA");
    assert_eq!(
        instance.path(),
        "LENOVO_UTILITY_DATA.InstanceName=\"ACPI\\VPC\""
    );
    assert_eq!(
        &*transport.queries.lock(),
        &[(
            "ROOT\\WMI".into(),
            "SELECT __Path, Active, InstanceName FROM LENOVO_UTILITY_DATA WHERE Active = TRUE"
                .into()
        )]
    );
}

#[test]
fn absent_active_instance_is_unsupported() {
    let transport = FakeTransport::default();

    assert!(matches!(
        active_instance(&transport, "LENOVO_UTILITY_DATA"),
        Err(LctrlError::Unsupported { .. })
    ));
}

#[test]
fn multiple_active_instances_select_documented_first_instance() {
    let object = |path: &str| {
        BTreeMap::from([
            ("__Path".into(), WmiValue::String(path.into())),
            ("Active".into(), WmiValue::Bool(true)),
        ])
    };
    let transport = FakeTransport {
        objects: vec![object("path-a"), object("path-b")],
        ..Default::default()
    };

    assert_eq!(
        active_instance(&transport, "LENOVO_UTILITY_DATA")
            .unwrap()
            .path(),
        "path-a"
    );
}

#[test]
fn invalid_class_identifier_is_rejected_before_query() {
    let transport = FakeTransport::default();

    assert!(matches!(
        active_instance(&transport, "LENOVO_X; DELETE *"),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(transport.queries.lock().is_empty());
}

#[test]
fn firmware_instance_without_system_path_uses_its_instance_key() {
    let transport = FakeTransport {
        objects: vec![BTreeMap::from([
            (
                "InstanceName".into(),
                WmiValue::String(r#"ACPI\PNP0C14\LSK20_0"#.into()),
            ),
            ("Active".into(), WmiValue::Bool(true)),
        ])],
        ..Default::default()
    };

    let instance = active_instance(&transport, "LENOVO_UTILITY_DATA").unwrap();

    assert_eq!(
        instance.path(),
        r#"LENOVO_UTILITY_DATA.InstanceName="ACPI\\PNP0C14\\LSK20_0""#
    );
}

#[test]
fn missing_both_path_forms_is_a_channel_error() {
    let transport = FakeTransport {
        objects: vec![BTreeMap::from([("Active".into(), WmiValue::Bool(true))])],
        ..Default::default()
    };

    assert!(matches!(
        active_instance(&transport, "LENOVO_UTILITY_DATA"),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn pathless_row_is_skipped_for_the_first_invokable_instance() {
    let transport = FakeTransport {
        objects: vec![
            BTreeMap::from([("Active".into(), WmiValue::Bool(true))]),
            BTreeMap::from([
                (
                    "InstanceName".into(),
                    WmiValue::String(r#"ACPI\PNP0C14\LSK20_0"#.into()),
                ),
                ("Active".into(), WmiValue::Bool(true)),
            ]),
        ],
        ..Default::default()
    };

    assert_eq!(
        active_instance(&transport, "LENOVO_UTILITY_DATA")
            .unwrap()
            .path(),
        r#"LENOVO_UTILITY_DATA.InstanceName="ACPI\\PNP0C14\\LSK20_0""#
    );
}

#[test]
fn vendor_control_conflicts_are_discovered_through_read_only_process_inventory() {
    let transport = FakeTransport {
        objects: vec![
            BTreeMap::from([(
                "Name".into(),
                WmiValue::String("LenovoVantageService.exe".into()),
            )]),
            BTreeMap::from([("Name".into(), WmiValue::String("explorer.exe".into()))]),
        ],
        ..Default::default()
    };
    let detector = WindowsControlConflictDetector::new(transport);

    assert_eq!(
        detector.active_vendor_controllers().unwrap(),
        vec!["LenovoVantageService.exe"]
    );
}
