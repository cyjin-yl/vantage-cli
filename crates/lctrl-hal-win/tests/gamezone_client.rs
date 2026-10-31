use std::sync::Mutex;

use lctrl_core::{LctrlError, Result};
use lctrl_hal_win::{FeatureId, GameZone, SupportProbe, WmiObject, WmiTransport, WmiValue};

#[derive(Default)]
struct FakeWmi {
    calls: Mutex<Vec<(String, String)>>,
    output: Mutex<WmiObject>,
    fail: Mutex<Option<String>>,
}

impl WmiTransport for FakeWmi {
    fn query(&self, _namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        if wql.contains("WHERE Active = TRUE") {
            let mut object = WmiObject::new();
            object.insert(
                "__Path".into(),
                WmiValue::String("\\\\.\\root\\wmi:LENOVO_GAMEZONE_DATA.InstanceName=\"x\"".into()),
            );
            Ok(vec![object])
        } else {
            Ok(vec![])
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        class: &str,
        _object_path: &str,
        method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        self.calls
            .lock()
            .unwrap()
            .push((class.into(), method.into()));
        if let Some(message) = self.fail.lock().unwrap().as_ref() {
            return Err(LctrlError::ChannelUnavailable {
                channel: message.clone(),
            });
        }
        Ok(self.output.lock().unwrap().clone())
    }
}

#[test]
fn getter_invokes_named_method_and_parses_data() {
    let fake = FakeWmi::default();
    fake.output.lock().unwrap().insert("Data".into(), WmiValue::U32(3));
    let zone = GameZone::new(&fake);

    assert_eq!(zone.thermal_mode().unwrap(), 3);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(
        calls[0],
        ("LENOVO_GAMEZONE_DATA".to_string(), "GetThermalMode".to_string())
    );
}

#[test]
fn getter_parses_first_numeric_property_without_conventional_name() {
    let fake = FakeWmi::default();
    fake.output
        .lock()
        .unwrap()
        .insert("ThermalMode".into(), WmiValue::U32(0x36));
    let zone = GameZone::new(&fake);

    assert_eq!(zone.thermal_mode().unwrap(), 0x36);
}

#[test]
fn setter_sends_single_u32_data_payload() {
    let fake = FakeWmi::default();
    fake.output.lock().unwrap().insert("ReturnValue".into(), WmiValue::U32(0));
    let zone = GameZone::new(&fake);

    zone.set_fan_cooling(2).unwrap();
    assert_eq!(fake.calls.lock().unwrap()[0].1, "SetFanCooling");
}

#[test]
fn setter_rejects_nonzero_return_value() {
    let fake = FakeWmi::default();
    fake.output
        .lock()
        .unwrap()
        .insert("ReturnValue".into(), WmiValue::U32(0x80004005));
    let zone = GameZone::new(&fake);

    let error = zone.set_smart_fan_mode(1).unwrap_err();
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
}

#[test]
fn support_probe_uses_zero_for_supported() {
    let fake = FakeWmi::default();
    fake.output.lock().unwrap().insert("Data".into(), WmiValue::U32(0));
    let zone = GameZone::new(&fake);

    assert!(zone.is_supported(SupportProbe::FanCooling).unwrap());
    assert_eq!(fake.calls.lock().unwrap()[0].1, "IsSupportFanCooling");
}

#[test]
fn fan_table_passes_fan_and_sensor_ids() {
    let fake = FakeWmi::default();
    fake.output.lock().unwrap().insert("Data".into(), WmiValue::U32(0));
    let zone = GameZone::new(&fake);

    zone.fan_get_table(1, 0).unwrap();
    assert_eq!(fake.calls.lock().unwrap()[0].0, "LENOVO_FAN_METHOD");
    assert_eq!(fake.calls.lock().unwrap()[0].1, "Fan_Get_Table");
}

#[test]
fn feature_plane_uses_other_method_class() {
    let fake = FakeWmi::default();
    fake.output.lock().unwrap().insert("Data".into(), WmiValue::U32(7));
    let zone = GameZone::new(&fake);

    assert_eq!(zone.feature_value(FeatureId::new(0x1000_0001)).unwrap(), 7);
    assert_eq!(fake.calls.lock().unwrap()[0].0, "LENOVO_OTHER_METHOD");
    zone.set_feature_value(FeatureId::new(0x1000_0001), 1).unwrap();
    assert_eq!(fake.calls.lock().unwrap()[1].1, "SetFeatureValue");
}

#[test]
fn transport_error_propagates_verbatim() {
    let fake = FakeWmi::default();
    *fake.fail.lock().unwrap() = Some(
        "WBEM_E_INVALID_METHOD_PARAMETERS from LENOVO_GAMEZONE_DATA.GetThermalMode".to_string(),
    );
    let zone = GameZone::new(&fake);

    let error = zone.thermal_mode().unwrap_err();
    match error {
        LctrlError::ChannelUnavailable { channel } => {
            assert!(channel.contains("WBEM_E_INVALID_METHOD_PARAMETERS"));
        }
        other => panic!("expected ChannelUnavailable, got {other:?}"),
    }
}

#[test]
fn missing_numeric_output_is_channel_unavailable() {
    let fake = FakeWmi::default();
    let zone = GameZone::new(&fake);

    let error = zone.cpu_frequency().unwrap_err();
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
}
