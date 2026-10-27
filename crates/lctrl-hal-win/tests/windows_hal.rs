use parking_lot::Mutex;

use lctrl_core::{Availability, Platform};
use lctrl_hal::Hal;
use lctrl_hal_win::{IoctlTransport, WindowsHal, WmiObject, WmiTransport, WmiValue};

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
    let hal = WindowsHal::new(FakeWmi::default(), FakeIoctl::default());

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
    let hal = WindowsHal::new(FakeWmi::default(), FakeIoctl::default());
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

#[test]
fn capability_probe_never_queries_gamezone_methods() {
    let wmi = FakeWmi::default();
    let hal = WindowsHal::new(wmi, FakeIoctl::default());

    hal.capabilities().unwrap();

    assert!(
        hal.wmi()
            .queries
            .lock()
            .iter()
            .all(|query| !query.contains("GAMEZONE"))
    );
}
