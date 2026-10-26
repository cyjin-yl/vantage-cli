use std::collections::BTreeMap;

use lctrl_core::{LctrlError, OsBatterySnapshot, SystemPowerStatus};
use lctrl_hal_win::{
    OsBatteryChannel, SystemPowerStatusReader, WindowsOsBattery, WmiObject, WmiTransport, WmiValue,
};

const ROOT_WMI: &str = "ROOT\\WMI";

#[derive(Clone, Debug)]
struct Query {
    namespace: String,
    wql: String,
}

struct FakeWmi {
    replies: parking_lot::Mutex<Vec<(String, WmiObject)>>,
    queries: parking_lot::Mutex<Vec<Query>>,
}

impl FakeWmi {
    fn new(replies: impl IntoIterator<Item = (&'static str, WmiObject)>) -> Self {
        let mut replies: Vec<_> = replies
            .into_iter()
            .map(|(class, object)| (class.to_owned(), object))
            .collect();
        replies.reverse();
        Self {
            replies: parking_lot::Mutex::new(replies),
            queries: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        self.queries.lock().push(Query {
            namespace: namespace.to_owned(),
            wql: wql.to_owned(),
        });
        for (class, object) in self.replies.lock().iter() {
            if wql.contains(class.as_str()) {
                return Ok(vec![object.clone()]);
            }
        }
        Ok(Vec::new())
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        Err(LctrlError::Unsupported {
            feature: "test.invoke".into(),
        })
    }
}

struct FakePowerStatus(SystemPowerStatus);

impl SystemPowerStatusReader for FakePowerStatus {
    fn read(&self) -> lctrl_core::Result<SystemPowerStatus> {
        Ok(self.0)
    }
}

fn object(entries: &[(&str, WmiValue)]) -> WmiObject {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>()
}

fn status_object() -> WmiObject {
    object(&[
        ("Active", WmiValue::Bool(true)),
        (
            "InstanceName",
            WmiValue::String("ACPI\\PNP0C0A\\1_0".into()),
        ),
        ("PowerOnline", WmiValue::Bool(true)),
        ("Charging", WmiValue::Bool(false)),
        ("Discharging", WmiValue::Bool(false)),
        ("ChargeRate", WmiValue::I32(0)),
        ("DischargeRate", WmiValue::I32(0)),
        ("RemainingCapacity", WmiValue::U32(95_110)),
        ("Voltage", WmiValue::U32(17_715)),
        ("Critical", WmiValue::Bool(false)),
    ])
}

fn full_charge_object() -> WmiObject {
    object(&[
        ("Active", WmiValue::Bool(true)),
        (
            "InstanceName",
            WmiValue::String("ACPI\\PNP0C0A\\1_0".into()),
        ),
        ("FullChargedCapacity", WmiValue::U32(95_110)),
    ])
}

fn cycle_object() -> WmiObject {
    object(&[
        ("Active", WmiValue::Bool(true)),
        (
            "InstanceName",
            WmiValue::String("ACPI\\PNP0C0A\\1_0".into()),
        ),
        ("CycleCount", WmiValue::U32(224)),
    ])
}

#[test]
fn os_battery_reads_the_public_battery_classes() {
    let wmi = FakeWmi::new([
        ("BatteryStatus", status_object()),
        ("BatteryFullChargedCapacity", full_charge_object()),
        ("BatteryCycleCount", cycle_object()),
    ]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.remaining_capacity_mwh, Some(95_110));
    assert_eq!(snapshot.full_charge_capacity_mwh, Some(95_110));
    assert_eq!(snapshot.cycle_count, Some(224));
    assert_eq!(snapshot.voltage_mv, Some(17_715));
    assert_eq!(snapshot.remaining_percent, None);
    assert_eq!(snapshot.power_online, Some(true));
    assert_eq!(snapshot.charging, Some(false));
    assert_eq!(snapshot.discharging, Some(false));
}

#[test]
fn os_battery_queries_only_the_root_wmi_battery_namespace() {
    let wmi = FakeWmi::new([("BatteryStatus", status_object())]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    let _ = channel.snapshot();

    for query in channel.wmi().queries.lock().iter() {
        assert_eq!(query.namespace, ROOT_WMI);
        assert!(query.wql.contains("Battery"));
    }
}

#[test]
fn os_battery_reports_missing_optional_classes_without_failing() {
    let wmi = FakeWmi::new([("BatteryStatus", status_object())]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.remaining_capacity_mwh, Some(95_110));
    assert_eq!(snapshot.full_charge_capacity_mwh, None);
    assert_eq!(snapshot.cycle_count, None);
}

#[test]
fn os_battery_fails_when_the_battery_class_is_absent() {
    let wmi = FakeWmi::new([]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    assert!(matches!(
        channel.snapshot(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn os_battery_reads_the_system_power_percentage() {
    let wmi = FakeWmi::new([("BatteryStatus", status_object())]);
    let power = FakePowerStatus(SystemPowerStatus {
        ac_line_status: Some(true),
        battery_flag: Some(1),
        battery_life_percent: Some(100),
        battery_life_time: None,
        battery_full_life_time: None,
    });
    let channel = WindowsOsBattery::new(wmi, power);

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.remaining_percent, Some(100));
}

#[test]
fn os_battery_ignores_a_sentinel_runtime_value() {
    let wmi = FakeWmi::new([
        ("BatteryStatus", status_object()),
        (
            "BatteryRuntime",
            object(&[
                ("Active", WmiValue::Bool(true)),
                ("EstimatedRuntime", WmiValue::U32(u32::MAX)),
            ]),
        ),
    ]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.estimated_runtime_min, None);
}

#[test]
fn os_battery_converts_a_runtime_value_to_minutes() {
    let wmi = FakeWmi::new([
        ("BatteryStatus", status_object()),
        (
            "BatteryRuntime",
            object(&[
                ("Active", WmiValue::Bool(true)),
                ("EstimatedRuntime", WmiValue::U32(7_200)),
            ]),
        ),
    ]);
    let channel = WindowsOsBattery::new(wmi, FakePowerStatus(SystemPowerStatus::default()));

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.estimated_runtime_min, Some(120));
}

#[test]
fn system_power_status_reads_the_aggregate_line_state() {
    let power = FakePowerStatus(SystemPowerStatus {
        ac_line_status: Some(true),
        battery_flag: Some(1),
        battery_life_percent: Some(87),
        battery_life_time: None,
        battery_full_life_time: None,
    });
    let wmi = FakeWmi::new([("BatteryStatus", status_object())]);
    let channel = WindowsOsBattery::new(wmi, power);

    let status = channel.system_power_status().unwrap();

    assert_eq!(status.ac_line_status, Some(true));
    assert_eq!(status.battery_life_percent(), Some(87));
}

#[test]
fn snapshot_exposes_a_default_empty_reading() {
    let snapshot = OsBatterySnapshot::default();

    assert_eq!(snapshot.remaining_capacity_mwh, None);
    assert_eq!(snapshot.cycle_count, None);
    assert_eq!(snapshot.power_online, None);
}
