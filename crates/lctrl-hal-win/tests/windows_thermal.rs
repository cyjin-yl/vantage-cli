use std::collections::BTreeMap;

use lctrl_core::{
    AdapterInfo, ApplyMode, Availability, BatteryElectricalObservation, BatteryTelemetry,
    ChangeReport, ChargeMode, ChargeModeActual, FanDescriptor, FanId, FanMode, LctrlError, Result,
    SensorId, TemperatureLocation, TemperatureSource,
};
use lctrl_hal::{BatteryControl, DbdcControl, FanControl, TemperatureControl};
use lctrl_hal_win::{WindowsThermal, WmiObject, WmiTransport, WmiValue};

#[derive(Clone, Copy)]
enum BatteryReply {
    Telemetry(Option<u32>),
    Unsupported,
    Unavailable,
    PermissionDenied,
    SourceIssue,
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

    fn telemetry(temperature: Option<u32>) -> Self {
        Self::new(BatteryReply::Telemetry(temperature))
    }

    fn unavailable() -> Self {
        Self::new(BatteryReply::Unsupported)
    }
}

impl BatteryControl for FakeBattery {
    fn battery_telemetry(&self, _index: u32) -> Result<BatteryTelemetry> {
        panic!("thermal observations must not read vendor telemetry")
    }

    fn battery_electrical(&self) -> Result<BatteryElectricalObservation> {
        match self.reply {
            BatteryReply::SourceIssue => Ok(BatteryElectricalObservation {
                voltage_mv: None,
                rate_mw: None,
                power_state: 0,
                temperature_deci_kelvin: None,
                source_issues: vec![lctrl_core::BatterySourceIssue {
                    source: "Windows battery class temperature",
                    detail: "native query failed".into(),
                }],
            }),
            BatteryReply::Telemetry(temperature) => Ok(BatteryElectricalObservation {
                voltage_mv: None,
                rate_mw: None,
                power_state: 0,
                temperature_deci_kelvin: temperature,
                source_issues: Vec::new(),
            }),
            BatteryReply::Unsupported => Err(LctrlError::Unsupported {
                feature: "Windows battery class".into(),
            }),
            BatteryReply::Unavailable => Err(LctrlError::ChannelUnavailable {
                channel: "test native battery failure".into(),
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

impl WmiReply {
    fn get_single(&self) -> Result<WmiObject> {
        match self {
            Self::Objects(objects) => {
                objects
                    .first()
                    .cloned()
                    .ok_or_else(|| LctrlError::ChannelUnavailable {
                        channel: "fake returned no WMI object".into(),
                    })
            }
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
    gamezone: WmiReply,
    dbdc: WmiReply,
    invoke_replies: Vec<(String, WmiObject)>,
    invocations: parking_lot::Mutex<Vec<String>>,
    queries: parking_lot::Mutex<Vec<Query>>,
}

impl FakeWmi {
    fn new(thermal: WmiReply, fans: WmiReply) -> Self {
        Self {
            thermal,
            fans,
            gamezone: WmiReply::Unsupported,
            dbdc: WmiReply::Unsupported,
            invoke_replies: Vec::new(),
            invocations: parking_lot::Mutex::new(Vec::new()),
            queries: parking_lot::Mutex::new(Vec::new()),
        }
    }

    fn with_gamezone(mut self, gamezone: WmiReply) -> Self {
        self.gamezone = gamezone;
        self
    }

    fn with_dbdc(mut self, dbdc: WmiReply) -> Self {
        self.dbdc = dbdc;
        self
    }

    fn with_invoke_reply(mut self, method: &str, object: WmiObject) -> Self {
        self.invoke_replies.push((method.into(), object));
        self
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
        } else if wql.contains("LENOVO_GAMEZONE_DATA") {
            self.gamezone.get()
        } else if wql.contains("LENOVO_REPORT_DBDC_DATA") {
            self.dbdc.get()
        } else {
            panic!("unexpected WMI query: {wql}")
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
        self.invocations.lock().push(format!("{class}.{method}"));
        for (reply_method, object) in &self.invoke_replies {
            if reply_method == method {
                return Ok(object.clone());
            }
        }
        Err(LctrlError::Unsupported {
            feature: format!("test.invoke.{method}"),
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
        ("InstanceName", WmiValue::String(instance_name.to_owned())),
        ("CurrentTemperature", WmiValue::U32(deci_kelvin)),
    ])
}

fn fan_inventory() -> WmiObject {
    object(&[
        ("NumOfFans", WmiValue::U32(1)),
        ("FanId", WmiValue::Array(vec![WmiValue::U32(1)])),
        ("FanMinSpeed", WmiValue::Array(vec![WmiValue::U32(2100)])),
        ("FanMaxSpeed", WmiValue::Array(vec![WmiValue::U32(5100)])),
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
        WmiReply::Objects(vec![thermal_zone(second, 3152), thermal_zone(first, 3000)]),
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
            "windows-battery-class/temperature",
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
        .find(|sensor| sensor.metadata.id == "windows-battery-class/temperature")
        .unwrap();
    assert_eq!(
        battery.metadata.name,
        "Windows battery class temperature (unassociated)"
    );
    assert_eq!(
        battery.metadata.source,
        TemperatureSource::WindowsBatteryClass
    );
    assert_eq!(battery.metadata.location, TemperatureLocation::Battery);
    assert_eq!(battery.metadata.availability, Availability::Available);
    assert_celsius(battery.value_c, 28.55);

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
    assert!(controller.battery().indices.lock().is_empty());
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
    assert_eq!(sensors[0].metadata.id, "windows-battery-class/temperature");
    assert_celsius(sensors[0].value_c, 28.55);
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
    assert_eq!(
        sensors[0].metadata.source,
        TemperatureSource::WindowsBatteryClass
    );
    assert_eq!(sensors[0].metadata.availability, Availability::Unavailable);
    assert_eq!(sensors[0].value_c, None);
}

#[test]
fn battery_failures_other_than_absence_are_preserved() {
    let wmi = FakeWmi::new(
        WmiReply::Objects(vec![thermal_zone("zone0", 3152)]),
        WmiReply::Objects(Vec::new()),
    );
    let controller = WindowsThermal::new(wmi, FakeBattery::new(BatteryReply::PermissionDenied));

    assert!(matches!(
        controller.temperature_sensors(),
        Err(LctrlError::PermissionDenied { .. })
    ));
}

#[test]
fn native_channel_failures_and_partial_source_issues_are_visible() {
    for reply in [BatteryReply::Unavailable, BatteryReply::SourceIssue] {
        let controller = WindowsThermal::new(FakeWmi::empty(), FakeBattery::new(reply));
        assert!(matches!(controller.temperature_sensors(),
            Err(LctrlError::ChannelUnavailable { channel }) if channel.contains("native")));
    }
}

#[test]
fn native_temperature_has_no_invented_sentinel() {
    for raw in [0, u32::MAX] {
        let controller = WindowsThermal::new(FakeWmi::empty(), FakeBattery::telemetry(Some(raw)));
        let sensors = controller.temperature_sensors().unwrap();
        assert_eq!(
            sensors[0].value_c,
            Some((f64::from(raw) / 10.0 - 273.15) as f32)
        );
    }
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
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(matches!(
        controller.fan_table(FanId::new(1), SensorId::new(0)),
        Err(LctrlError::Unsupported { feature }) if feature == "perf.fan.curve"
    ));
    for apply in [ApplyMode::DryRun, ApplyMode::Commit] {
        assert!(matches!(
            controller.set_fan_mode(FanMode::Standard, apply),
            Err(LctrlError::Unsupported { .. })
        ));
    }
}

fn gamezone_instance() -> WmiObject {
    object(&[("__Path", WmiValue::String("fake/gamezone".into()))])
}

#[test]
fn fan_mode_reads_smart_fan_raw_and_maps_semantic_mode() {
    let wmi = FakeWmi::empty()
        .with_gamezone(WmiReply::Objects(vec![gamezone_instance()]))
        .with_invoke_reply("GetSmartFanMode", object(&[("Data", WmiValue::U32(1))]));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    assert_eq!(controller.fan_mode().unwrap(), FanMode::Silent);
}

#[test]
fn set_fan_mode_commit_sends_raw_and_verifies_readback() {
    let wmi = FakeWmi::empty()
        .with_gamezone(WmiReply::Objects(vec![gamezone_instance()]))
        .with_invoke_reply("GetSmartFanMode", object(&[("Data", WmiValue::U32(2))]))
        .with_invoke_reply(
            "SetSmartFanMode",
            object(&[("ReturnValue", WmiValue::U32(0))]),
        );
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let report = controller
        .set_fan_mode(FanMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.mode(), ApplyMode::Commit);
    assert_eq!(report.requested(), &FanMode::Performance);
    assert_eq!(report.actual(), Some(&FanMode::Performance));
    let wmi = controller.wmi();
    let invocations = wmi.invocations.lock();
    assert!(invocations.contains(&"LENOVO_GAMEZONE_DATA.SetSmartFanMode".to_string()));
}

#[test]
fn set_fan_mode_dry_run_reads_but_never_invokes_the_setter() {
    let wmi = FakeWmi::empty()
        .with_gamezone(WmiReply::Objects(vec![gamezone_instance()]))
        .with_invoke_reply("GetSmartFanMode", object(&[("Data", WmiValue::U32(0))]));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let report = controller
        .set_fan_mode(FanMode::Silent, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.mode(), ApplyMode::DryRun);
    assert_eq!(report.actual(), None);
    let wmi = controller.wmi();
    let invocations = wmi.invocations.lock();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0], "LENOVO_GAMEZONE_DATA.GetSmartFanMode");
}

#[test]
fn set_fan_mode_rejects_unknown_semantic_mode_without_touching_wmi() {
    let wmi = FakeWmi::empty()
        .with_gamezone(WmiReply::Objects(vec![gamezone_instance()]))
        .with_invoke_reply("GetSmartFanMode", object(&[("Data", WmiValue::U32(0))]));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let error = controller
        .set_fan_mode(FanMode::Unknown(9), ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
    assert!(controller.wmi().invocations.lock().is_empty());
}

#[test]
fn set_fan_mode_surfaces_verify_mismatch_when_readback_disagrees() {
    let wmi = FakeWmi::empty()
        .with_gamezone(WmiReply::Objects(vec![gamezone_instance()]))
        .with_invoke_reply("GetSmartFanMode", object(&[("Data", WmiValue::U32(0))]))
        .with_invoke_reply(
            "SetSmartFanMode",
            object(&[("ReturnValue", WmiValue::U32(0))]),
        );
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let error = controller
        .set_fan_mode(FanMode::Performance, ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
}

#[test]
fn dbdc_profile_parses_vendor_table() {
    let row = object(&[
        ("Counts", WmiValue::U32(3)),
        (
            "CurrentLimit",
            WmiValue::Array(vec![
                WmiValue::U32(7500),
                WmiValue::U32(5000),
                WmiValue::U32(4500),
            ]),
        ),
        (
            "ROS_Power",
            WmiValue::Array(vec![
                WmiValue::U32(65000),
                WmiValue::U32(55000),
                WmiValue::U32(35000),
            ]),
        ),
        (
            "Threshold",
            WmiValue::Array(vec![
                WmiValue::U32(100),
                WmiValue::U32(40),
                WmiValue::U32(20),
            ]),
        ),
    ]);
    let wmi = FakeWmi::empty().with_dbdc(WmiReply::Objects(vec![row]));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let profile = controller.dbdc_profile().expect("dbdc table");

    assert_eq!(profile.counts, 3);
    assert_eq!(profile.current_limit_ma, vec![7500, 5000, 4500]);
    assert_eq!(profile.ros_power_mw, vec![65000, 55000, 35000]);
    assert_eq!(profile.threshold_pct, vec![100, 40, 20]);
    assert!(profile.is_consistent());
}

#[test]
fn dbdc_profile_rejects_missing_instance() {
    let wmi = FakeWmi::empty().with_dbdc(WmiReply::Objects(Vec::new()));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let error = controller.dbdc_profile().unwrap_err();
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
}

#[test]
fn dbdc_profile_rejects_inconsistent_arrays() {
    let row = object(&[
        ("Counts", WmiValue::U32(3)),
        ("CurrentLimit", WmiValue::Array(vec![WmiValue::U32(7500)])),
        ("ROS_Power", WmiValue::Array(vec![WmiValue::U32(65000)])),
        ("Threshold", WmiValue::Array(vec![WmiValue::U32(100)])),
    ]);
    let wmi = FakeWmi::empty().with_dbdc(WmiReply::Objects(vec![row]));
    let controller = WindowsThermal::new(wmi, FakeBattery::unavailable());

    let error = controller.dbdc_profile().unwrap_err();
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
}
