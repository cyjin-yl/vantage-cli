#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use parking_lot::Mutex;

use lctrl_core::{Availability, CapabilitySet, LctrlError, Platform};
use lctrl_hal::Hal;
use lctrl_hal_win::{
    IoctlTransport, PerformanceRegistryReader, WindowsHal, WmiObject, WmiTransport, WmiValue,
    record_fan_capabilities,
};

#[derive(Default)]
struct FakeWmi {
    queries: Mutex<Vec<String>>,
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        self.queries.lock().push(format!("{namespace}:{wql}"));
        let object = if wql.contains("Win32_ComputerSystemProduct") {
            WmiObject::from([("Name".into(), WmiValue::String("21VG".into()))])
        } else if wql.contains("Win32_ComputerSystem") {
            WmiObject::from([
                (
                    "Model".into(),
                    WmiValue::String("THINKBOOK_14_G8+_IPH".into()),
                ),
                ("SystemFamily".into(), WmiValue::String("ThinkBook".into())),
            ])
        } else if wql.contains("Win32_BIOS") {
            WmiObject::from([(
                "SMBIOSBIOSVersion".into(),
                WmiValue::String("N3GET18W".into()),
            )])
        } else if wql.contains("LENOVO_UTILITY_DATA") {
            WmiObject::from([
                (
                    "__Path".into(),
                    WmiValue::String("LENOVO_UTILITY_DATA.InstanceName=\"ACPI\\VPC\"".into()),
                ),
                ("Active".into(), WmiValue::Bool(true)),
            ])
        } else {
            return Ok(Vec::new());
        };
        Ok(vec![object])
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        unreachable!("root HAL probing must not invoke a WMI method")
    }
}

#[derive(Default)]
struct FakeIoctl {
    calls: Mutex<Vec<(u32, Vec<u8>, usize)>>,
}

impl IoctlTransport for FakeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> lctrl_core::Result<Vec<u8>> {
        self.calls.lock().push((code, input.to_vec(), output_len));
        Ok(0x0086_0004_u32.to_le_bytes().to_vec())
    }
}

#[test]
fn windows_hal_reports_platform_and_hardware_info() {
    let hal = WindowsHal::new(FakeWmi::default(), FakeIoctl::default());

    assert_eq!(hal.platform(), Platform::Windows);
    let info = hal.hardware_info().unwrap();
    assert_eq!(info.product_name.as_deref(), Some("21VG"));
    assert_eq!(info.family.as_deref(), Some("ThinkBook"));
    assert_eq!(info.bios_version.as_deref(), Some("N3GET18W"));
}

#[test]
fn capability_probe_reports_real_channels_and_fixed_dead_ends() {
    let hal =
        WindowsHal::with_electrical(FakeWmi::default(), FakeIoctl::default(), AbsentElectrical);

    let capabilities = hal.capabilities().unwrap();

    assert_eq!(capabilities.platform, Platform::Windows);
    assert_eq!(
        capabilities.get("channel.wmi.root").unwrap().availability,
        Availability::Available
    );
    assert_eq!(
        capabilities
            .get("channel.energy_driver")
            .unwrap()
            .availability,
        Availability::Available
    );
    for feature in [
        "battery.thresholds",
        "tune.windows_rapl",
        "wmi.gamezone.methods",
    ] {
        assert_eq!(
            capabilities.get(feature).unwrap().availability,
            Availability::Unavailable,
            "{feature} must remain unavailable on 21VG"
        );
    }
}

#[test]
fn capability_probe_requires_battery_detail_for_battery_telemetry() {
    let hal =
        WindowsHal::with_electrical(FakeWmi::default(), FakeIoctl::default(), AbsentElectrical);
    let capabilities = hal.capabilities().unwrap();

    assert_eq!(
        capabilities.get("battery.info").unwrap().availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("battery.status").unwrap().availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("battery.adapter").unwrap().availability,
        Availability::Available
    );
    assert_eq!(
        capabilities.get("power.scheme").unwrap().availability,
        Availability::Limited
    );
}

/// Scripted `ITS_FanMode` reader: `None` models a machine without the
/// dispatcher PowerSlider key.
struct FakeRegistry(Option<u32>);

impl PerformanceRegistryReader for FakeRegistry {
    fn read_dword(&self, value: &str) -> lctrl_core::Result<u32> {
        match self.0 {
            Some(raw) if value == "ITS_FanMode" => Ok(raw),
            _ => Err(LctrlError::ChannelUnavailable {
                channel: format!("{value} is not present"),
            }),
        }
    }
}

#[test]
fn fan_capabilities_separate_the_firmware_read_from_the_dispatcher_projection() {
    // No firmware fan getter, no dispatcher projection: both the read and the
    // write are unavailable and the reason names the firmware call.
    let wmi = FakeWmi::default();
    let mut capabilities = CapabilitySet::new(Platform::Windows);
    record_fan_capabilities(&mut capabilities, &wmi, &FakeRegistry(None)).unwrap();

    let fan_mode = capabilities.get("perf.fan.mode").unwrap();
    assert_eq!(fan_mode.availability, Availability::Unavailable);
    assert!(
        fan_mode
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("GAMEZONE")),
        "expected the GAMEZONE rejection detail, got {:?}",
        fan_mode.detail
    );
    let fan_write = capabilities.get("perf.fan.mode.write").unwrap();
    assert_eq!(fan_write.availability, Availability::Unavailable);
    assert!(
        fan_write
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("GetSmartFanMode")),
        "expected the readback requirement in the write detail, got {:?}",
        fan_write.detail
    );
    // The capability probe is read-only: it queries the GAMEZONE class but
    // must never invoke a WMI method (`FakeWmi::invoke_instance` is
    // `unreachable!`).
    assert!(
        wmi.queries
            .lock()
            .iter()
            .any(|query| query.contains("LENOVO_GAMEZONE_DATA"))
    );
}

#[test]
fn dispatcher_fan_projection_is_limited_and_never_enables_the_write() {
    let mut capabilities = CapabilitySet::new(Platform::Windows);
    record_fan_capabilities(
        &mut capabilities,
        &FakeWmi::default(),
        &FakeRegistry(Some(4)),
    )
    .unwrap();

    let fan_mode = capabilities.get("perf.fan.mode").unwrap();
    assert_eq!(fan_mode.availability, Availability::Limited);
    assert!(
        fan_mode
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("ITS_FanMode=4")),
        "expected the projection value in the detail, got {:?}",
        fan_mode.detail
    );
    assert_eq!(
        capabilities
            .get("perf.fan.mode.write")
            .unwrap()
            .availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("channel.fan").unwrap().availability,
        Availability::Available
    );
}

struct ElectricalTemperature(Option<u32>, bool);

struct ThermalWmi(Option<WmiValue>);

impl WmiTransport for ThermalWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        if wql.contains("MSAcpi_ThermalZoneTemperature") {
            return match &self.0 {
                Some(value) => Ok(vec![WmiObject::from([(
                    "CurrentTemperature".into(),
                    value.clone(),
                )])]),
                None => Err(lctrl_core::LctrlError::PermissionDenied {
                    need: "ACPI access denied".into(),
                }),
            };
        }
        FakeWmi::default().query(namespace, wql)
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        unreachable!("capability probing does not invoke methods")
    }
}

#[test]
fn acpi_capability_requires_numeric_evidence_and_retains_access_failures() {
    for (value, expected) in [
        (Some(WmiValue::U32(3000)), Availability::Available),
        (
            Some(WmiValue::String("3000".into())),
            Availability::Unavailable,
        ),
        (None, Availability::Unavailable),
    ] {
        let denied = value.is_none();
        let hal = WindowsHal::with_electrical(
            ThermalWmi(value),
            FakeIoctl::default(),
            ElectricalTemperature(None, false),
        );
        let capabilities = hal.capabilities().unwrap();
        let capability = capabilities.get("perf.temp").unwrap();
        assert_eq!(capability.availability, expected);
        if denied {
            assert!(
                capability
                    .detail
                    .as_deref()
                    .unwrap()
                    .contains("ACPI access denied")
            );
        }
    }
}

struct ReadableVendorBattery;

struct FailedElectrical(bool);

struct AbsentElectrical;

impl lctrl_hal::BatteryElectricalSource for AbsentElectrical {
    fn read_electrical(&self) -> lctrl_core::Result<lctrl_core::BatteryElectricalObservation> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "Windows battery class".into(),
        })
    }
}

#[test]
fn optional_native_absence_does_not_hide_acpi_temperature() {
    let hal = WindowsHal::with_electrical(
        ThermalWmi(Some(WmiValue::U32(3000))),
        FakeIoctl::default(),
        AbsentElectrical,
    );
    let capabilities = hal.capabilities().unwrap();
    assert_eq!(
        capabilities.get("perf.temp").unwrap().availability,
        Availability::Available
    );
}

impl lctrl_hal::BatteryElectricalSource for FailedElectrical {
    fn read_electrical(&self) -> lctrl_core::Result<lctrl_core::BatteryElectricalObservation> {
        if self.0 {
            Err(lctrl_core::LctrlError::PermissionDenied {
                need: "native temperature denied".into(),
            })
        } else {
            Err(lctrl_core::LctrlError::ChannelUnavailable {
                channel: "native temperature failed".into(),
            })
        }
    }
}

#[test]
fn acpi_temperature_does_not_hide_fatal_native_source_errors() {
    for permission in [false, true] {
        let hal = WindowsHal::with_electrical(
            ThermalWmi(Some(WmiValue::U32(3000))),
            FakeIoctl::default(),
            FailedElectrical(permission),
        );
        let capabilities = hal.capabilities().unwrap();
        let capability = capabilities.get("perf.temp").unwrap();
        assert_eq!(capability.availability, Availability::Unavailable);
        assert!(
            capability
                .detail
                .as_deref()
                .unwrap()
                .contains("native temperature")
        );
    }
    let hal = WindowsHal::with_electrical(
        ThermalWmi(Some(WmiValue::U32(3000))),
        FakeIoctl::default(),
        ElectricalTemperature(None, true),
    );
    let capabilities = hal.capabilities().unwrap();
    assert_eq!(
        capabilities.get("perf.temp").unwrap().availability,
        Availability::Unavailable
    );
}

#[test]
fn native_temperature_does_not_hide_fatal_acpi_source_errors() {
    for value in [None, Some(WmiValue::String("malformed".into()))] {
        let denied = value.is_none();
        let hal = WindowsHal::with_electrical(
            ThermalWmi(value),
            FakeIoctl::default(),
            ElectricalTemperature(Some(3000), false),
        );
        let capabilities = hal.capabilities().unwrap();
        let capability = capabilities.get("perf.temp").unwrap();
        assert_eq!(capability.availability, Availability::Unavailable);
        let detail = capability.detail.as_deref().unwrap();
        assert!(detail.contains("ACPI"));
        if denied {
            assert!(detail.contains("ACPI access denied"));
        } else {
            assert!(detail.contains("CurrentTemperature"));
        }
    }
}

impl IoctlTransport for ReadableVendorBattery {
    fn call(&self, _code: u32, _input: &[u8], output_len: usize) -> lctrl_core::Result<Vec<u8>> {
        if output_len == 83 {
            Ok(vec![0; 83])
        } else {
            Ok(0x0086_0004_u32.to_le_bytes().to_vec())
        }
    }
}

#[test]
fn readable_vendor_telemetry_does_not_establish_temperature_capability() {
    let hal = WindowsHal::with_electrical(
        FakeWmi::default(),
        ReadableVendorBattery,
        ElectricalTemperature(None, false),
    );
    let capabilities = hal.capabilities().unwrap();
    assert_eq!(
        capabilities.get("perf.temp").unwrap().availability,
        Availability::Unavailable
    );
    assert_eq!(
        capabilities.get("battery.info").unwrap().availability,
        Availability::Available
    );
}

impl lctrl_hal::BatteryElectricalSource for ElectricalTemperature {
    fn read_electrical(&self) -> lctrl_core::Result<lctrl_core::BatteryElectricalObservation> {
        Ok(lctrl_core::BatteryElectricalObservation {
            voltage_mv: None,
            rate_mw: None,
            power_state: 0,
            temperature_deci_kelvin: self.0,
            source_issues: if self.1 {
                vec![lctrl_core::BatterySourceIssue {
                    source: "Windows battery class temperature",
                    detail: "temperature request denied".into(),
                }]
            } else {
                Vec::new()
            },
        })
    }
}

#[test]
fn temperature_capability_requires_real_native_temperature() {
    for (temperature, issue, expected) in [
        (None, false, Availability::Unavailable),
        (Some(3017), false, Availability::Available),
        (Some(3017), true, Availability::Unavailable),
    ] {
        let hal = WindowsHal::with_electrical(
            FakeWmi::default(),
            FakeIoctl::default(),
            ElectricalTemperature(temperature, issue),
        );
        let capabilities = hal.capabilities().unwrap();
        let capability = capabilities.get("perf.temp").unwrap();
        assert_eq!(capability.availability, expected);
        let detail = capability.detail.as_deref().unwrap();
        assert!(detail.contains("ACPI"));
        if issue {
            assert!(detail.contains("temperature request denied"));
        } else if temperature.is_none() {
            assert!(detail.contains("Windows battery class exposed no temperature"));
        }
    }
}
