use std::collections::BTreeMap;

use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, BatteryTelemetry, ChangeReport, ChargeMode,
    ChargeModeActual, FanDescriptor, FanId, FanMode, LctrlError, Result, SensorId,
    TemperatureLocation, TemperatureSource,
};
use lctrl_hal::{BatteryControl, FanControl, TemperatureControl};
use lctrl_hal_win::{WindowsThermal, WmiObject, WmiTransport, WmiValue};

#[derive(Clone, Copy)]
enum BatteryReply {
    Telemetry(Option<u16>),
    Unsupported,
    Unavailable,
    PermissionDenied,
}

struct FakeBattery {
    reply: BatteryReply,
    indices: parking_lot::Mutex<Vec<u32>>,
}

impl FakeBattery {
    fn new(reply: BatteryReply) -> Self {
        Self {
            reply,
            indices: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn telemetry(temperature: Option<u16>) -> Self {
        Self::new(BatteryReply::Telemetry(temperature))
    }

    fn unavailable() -> Self {
        Self::new(BatteryReply::Unavailable)
    }
}

impl BatteryControl for FakeBattery {
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        self.indices.lock().push(index);
        match self.reply {
            BatteryReply::Telemetry(temperature) => {
                let mut telemetry = BatteryTelemetry::parse(&[0; 83]).unwrap();
                telemetry.temperature_deci_kelvin = temperature;
                Ok(telemetry)
            }
            BatteryReply::Unsupported => Err(LctrlError::Unsupported {
                feature: "battery.status".into(),
            }),
            BatteryReply::Unavailable => Err(LctrlError::ChannelUnavailable {
                channel: "test battery channel".into(),
            }),
            BatteryReply::PermissionDenied => Err(LctrlError::PermissionDenied {
                need: "administrator".into(),
            }),
        }
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        unreachable!("temperature tests do not query adapter information")
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        unreachable!("temperature tests do not query charge mode")
    }

    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        unreachable!("temperature tests do not change charge mode")
    }
}

#[derive(Clone)]
enum WmiReply {
    Objects(Vec<WmiObject>),
    Unsupported,
    PermissionDenied,
}

impl WmiReply {
    fn get(&self) -> Result<Vec<WmiObject>> {
        match self {
            Self::Objects(objects) => Ok(objects.clone()),
            Self::Unsupported => Err(LctrlError::Unsupported {
                feature: "test WMI class".into(),
            }),
            Self::PermissionDenied => Err(LctrlError::PermissionDenied {
                need: "administrator".into(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Query {
    namespace: String,
    wql: String,
}

struct FakeWmi {
    thermal: WmiReply,
    fans: WmiReply,
    queries: parking_lot::Mutex<Vec<Query>>,
}

impl FakeWmi {
    fn new(thermal: WmiReply, fans: WmiReply) -> Self {
        Self {
            thermal,
            fans,
            queries: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn empty() -> Self {
        Self::new(WmiReply::Objects(Vec::new()), WmiReply::Objects(Vec::new()))
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        self.queries.lock().push(Query {
            namespace: namespace.into(),
            wql: wql.into(),
        });
        if wql.contains("MSAcpi_ThermalZoneTemperature") {
            self.thermal.get()
        } else if wql.contains("LENOVO_FAN_TEST_DATA") {
            self.fans.get()
        } else {
            panic!("unexpected WMI query: {wql}")
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        Err(LctrlError::Unsupported {
            feature: "test.invoke".into(),
        })
    }
}

fn object(entries: &[(&str, WmiValue)]) -> WmiObject {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>()
}

fn thermal_zone(instance_name: &str, deci_kelvin: u32) -> WmiObject {
    object(&[
        (
            "InstanceName",
            WmiValue::String(instance_name.to_owned()),
        ),
        ("CurrentTemperature", WmiValue::U32(deci_kelvin)),
    ])
}

fn fan_inventory() -> WmiObject {
    object(&[
        ("NumOfFans", WmiValue::U32(1)),
        ("FanId", WmiValue::Array(vec![WmiValue::U32(1)])),
        (
            "FanMinSpeed",
            WmiValue::Array(vec![WmiValue::U32(2100)]),
        ),
        (
            "FanMaxSpeed",
            WmiValue::Array(vec![WmiValue::U32(5100)]),
        ),
    ])
}

fn assert_celsius(actual: Option<f32>, expected: f32) {
    let actual = actual.expect("temperature reading");
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected} degrees Celsius, got {actual}"
    );
}

#[test]
fn temperatures_preserve_source_identity_and_convert_deci_kelvin() {
    let first = r"ACPI\ThermalZone\TZ00_0";
    let second = r"ACPI\ThermalZone\TZ01_0";
    let wmi = FakeWmi::new(
        WmiReply::Objects(vec![
            thermal_zone(second, 3152),
            thermal_zone(first, 3000),
        ]),
        WmiReply::Objects(Vec::new()),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::telemetry(Some(3017)));

    let sensors = controller.temperature_sensors().unwrap();

    let ids: Vec<_> = sensors
        .iter()
        .map(|sensor| sensor.metadata.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            r"acpi/ACPI\ThermalZone\TZ00_0",
            r"acpi/ACPI\ThermalZone\TZ01_0",
            "battery/0",
        ]
    );

    let first_zone = sensors
        .iter()
        .find(|sensor| sensor.metadata.id == format!("acpi/{first}"))
        .unwrap();
    // 3000 deci-Kelvin is 300.0 K = 26.85 degrees Celsius.
    assert_celsius(first_zone.value_c, 26.85);

    let battery = sensors
        .iter()
        .find(|sensor| sensor.metadata.id == "battery/0")
        .unwrap();
    assert_eq!(battery.metadata.name, "Battery pack 0 (SMBus)");
    assert_eq!(battery.metadata.source, TemperatureSource::BatterySmbus);
    assert_eq!(battery.metadata.location, TemperatureLocation::Battery);
    assert_eq!(battery.metadata.availability, Availability::Available);
    assert_celsius(battery.value_c, 28.54);

    let acpi = sensors
        .iter()
        .find(|sensor| sensor.metadata.id == format!("acpi/{second}"))
        .unwrap();
    assert!(acpi.metadata.name.contains(second));
    assert_eq!(acpi.metadata.source, TemperatureSource::Acpi);
    assert_eq!(acpi.metadata.location, TemperatureLocation::Unknown);
    assert_eq!(acpi.metadata.availability, Availability::Available);
    // 3152 deci-Kelvin is 315.2 K = 42.05 degrees Celsius.
    assert_celsius(acpi.value_c, 42.05);
    assert_eq!(controller.battery().indices.lock().as_slice(), &[0]);
}

#[test]
fn temperature_by_id_returns_the_same_stable_instance_identity() {
    let instance = r"ACPI\ThermalZone\TZ00_0";
    let wmi = FakeWmi::new(
        WmiReply::Objects(vec![thermal_zone(instance, 3152)]),
        WmiReply::Objects(Vec::new()),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let sensor = controller.temperature(&format!("acpi/{instance}")).unwrap();

    assert_eq!(sensor.metadata.id, format!("acpi/{instance}"));
    assert!(sensor.metadata.name.contains(instance));
    assert_eq!(sensor.metadata.location, TemperatureLocation::Unknown);
    assert_celsius(sensor.value_c, 42.05);
}

#[test]
fn absent_battery_does_not_hide_an_acpi_temperature() {
    let wmi = FakeWmi::new(
        WmiReply::Objects(vec![thermal_zone("zone0", 3152)]),
        WmiReply::Objects(Vec::new()),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::new(BatteryReply::Unsupported));

    let sensors = controller.temperature_sensors().unwrap();

    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].metadata.id, "acpi/zone0");
    assert_eq!(sensors[0].metadata.source, TemperatureSource::Acpi);
}

#[test]
fn absent_acpi_class_does_not_hide_the_battery_temperature() {
    let wmi = FakeWmi::new(WmiReply::Unsupported, WmiReply::Objects(Vec::new()));
    let controller = WindowsThermal::new(wmi, FakeBattery::telemetry(Some(3017)));

    let sensors = controller.temperature_sensors().unwrap();

    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].metadata.id, "battery/0");
    assert_celsius(sensors[0].value_c, 28.54);
}

#[test]
fn no_available_temperature_sources_is_unsupported() {
    let controller = WindowsThermal::new(FakeWmi::empty(), FakeBattery::unavailable());

    assert!(matches!(
        controller.temperature_sensors(),
        Err(LctrlError::Unsupported { feature }) if feature == "perf.temp"
    ));
}

#[test]
fn missing_battery_temperature_is_unavailable_without_a_fabricated_value() {
    let controller = WindowsThermal::new(FakeWmi::empty(), FakeBattery::telemetry(None));

    let sensors = controller.temperature_sensors().unwrap();

    assert_eq!(sensors.len(), 1);
    assert_eq!(sensors[0].metadata.source, TemperatureSource::BatterySmbus);
    assert_eq!(sensors[0].metadata.availability, Availability::Unavailable);
    assert_eq!(sensors[0].value_c, None);
}

#[test]
fn battery_failures_other_than_absence_are_preserved() {
    let wmi = FakeWmi::new(
        WmiReply::Objects(vec![thermal_zone("zone0", 3152)]),
        WmiReply::Objects(Vec::new()),
    );
    let controller = WindowsThermal::new(
        wmi,
        FakeBattery::new(BatteryReply::PermissionDenied),
    );

    assert!(matches!(
        controller.temperature_sensors(),
        Err(LctrlError::PermissionDenied { .. })
    ));
}

#[test]
fn acpi_query_failures_other_than_absence_are_preserved() {
    let controller = WindowsThermal::new(
        FakeWmi::new(WmiReply::PermissionDenied, WmiReply::Objects(Vec::new())),
        FakeBattery::telemetry(Some(3017)),
    );

    assert!(matches!(
        controller.temperature_sensors(),
        Err(LctrlError::PermissionDenied { .. })
    ));
}

#[test]
fn fan_inventory_uses_the_real_id_and_rpm_bounds() {
    let wmi = FakeWmi::new(
        WmiReply::Objects(Vec::new()),
        WmiReply::Objects(vec![fan_inventory()]),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let fans = controller.fans().unwrap();

    assert_eq!(
        fans,
        vec![FanDescriptor::new(FanId::new(1), 2100, 5100).unwrap()]
    );
    let queries = controller.wmi().queries.lock();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].namespace, r"ROOT\WMI");
    assert!(queries[0].wql.contains("LENOVO_FAN_TEST_DATA"));
}

#[test]
fn fan_inventory_rejects_array_length_mismatches_instead_of_truncating() {
    let mut malformed = fan_inventory();
    malformed.insert("NumOfFans".into(), WmiValue::U32(2));
    let wmi = FakeWmi::new(
        WmiReply::Objects(Vec::new()),
        WmiReply::Objects(vec![malformed]),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    assert!(matches!(
        controller.fans(),
        Err(LctrlError::ChannelUnavailable { channel })
            if channel.contains("NumOfFans is 2")
    ));
}

#[test]
fn fan_mode_table_and_writes_remain_unsupported_without_real_channels() {
    let controller = WindowsThermal::new(FakeWmi::empty(), FakeBattery::unavailable());

    assert!(matches!(
        controller.fan_mode(),
        Err(LctrlError::Unsupported { feature }) if feature == "perf.fan.mode"
    ));
    assert!(matches!(
        controller.fan_table(FanId::new(1), SensorId::new(0)),
        Err(LctrlError::Unsupported { feature }) if feature == "perf.fan.curve"
    ));
    for apply in [ApplyMode::DryRun, ApplyMode::Commit] {
        assert!(matches!(
            controller.set_fan_mode(FanMode::Standard, apply),
            Err(LctrlError::Unsupported { feature }) if feature == "perf.fan.mode.set"
        ));
    }
}
