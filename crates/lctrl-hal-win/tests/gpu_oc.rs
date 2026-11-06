//! GPU OC status against a scripted firmware transport.
//!
//! The live 21VG answers `IsSupportGpuOC -> Data=0` and rejects the parameter
//! class with `WBEM_E_NOT_SUPPORTED`; these fixtures pin both the honest
//! unsupported report and the schema-faithful decode a capable machine would
//! take, without asserting any hardware value.

use std::sync::Mutex;

use lctrl_core::{LctrlError, Result};
use lctrl_hal::GpuOcControl;
use lctrl_hal_win::{WmiObject, WmiTransport, WmiValue, WindowsGpuOc};

struct FakeWmi {
    /// `(ReturnValue, Data)` for every `Is*` probe invocation.
    support: Mutex<(bool, u32)>,
    /// `None` makes the parameter query fail the way the firmware does.
    oc_rows: Mutex<Option<Vec<WmiObject>>>,
    calls: Mutex<Vec<String>>,
}

impl Default for FakeWmi {
    fn default() -> Self {
        Self {
            support: Mutex::new((true, 0)),
            oc_rows: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, _namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        self.calls.lock().unwrap().push(wql.to_owned());
        if wql.contains("LENOVO_GAMEZONE_GPU_OC_DATA") {
            let rows = self.oc_rows.lock().unwrap().clone();
            if let Some(rows) = rows {
                return Ok(rows);
            }
            return Err(LctrlError::Unsupported {
                feature: format!("ROOT\\WMI query: {wql}"),
            });
        }
        if wql.contains("LENOVO_GAMEZONE_DATA") {
            let mut instance = WmiObject::new();
            instance.insert(
                "__Path".into(),
                WmiValue::String(
                    "\\\\.\\root\\wmi:LENOVO_GAMEZONE_DATA.InstanceName=\"ACPI\\\\PNP0C14\\\\GMZN_0\""
                        .into(),
                ),
            );
            instance.insert("Active".into(), WmiValue::Bool(true));
            return Ok(vec![instance]);
        }
        Ok(Vec::new())
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        self.calls.lock().unwrap().push(method.to_owned());
        let (return_value, data) = *self.support.lock().unwrap();
        let mut output = WmiObject::new();
        output.insert("ReturnValue".into(), WmiValue::Bool(return_value));
        output.insert("Data".into(), WmiValue::U32(data));
        Ok(output)
    }
}

#[test]
fn unsupported_machine_reports_probe_and_provider_verdict() {
    let fake = FakeWmi::default();
    let gpu = WindowsGpuOc::new(&fake);

    let status = gpu.gpu_oc_status().unwrap();

    assert!(!status.supported, "Data=0 means the firmware does not support GPU OC");
    assert_eq!(status.probe.method, "LENOVO_GAMEZONE_DATA.IsSupportGpuOC");
    assert!(status.probe.return_value);
    assert_eq!(status.probe.data, 0);
    assert!(status.parameter.is_none(), "no instance means no parameter block");
    let reason = status.parameter_error.expect("the verdict is kept as text");
    assert!(
        reason.contains("LENOVO_GAMEZONE_GPU_OC_DATA"),
        "the reason names the missing class: {reason}"
    );
    let calls = fake.calls.lock().unwrap();
    assert!(
        calls[0].contains("LENOVO_GAMEZONE_DATA WHERE Active = TRUE"),
        "the active instance is resolved first: {}",
        calls[0]
    );
    assert_eq!(calls[1], "IsSupportGpuOC");
    assert!(calls[2].contains("SELECT * FROM LENOVO_GAMEZONE_GPU_OC_DATA"));
}

#[test]
fn published_parameter_block_is_reported_verbatim() {
    let fake = FakeWmi::default();
    *fake.support.lock().unwrap() = (true, 1);
    *fake.oc_rows.lock().unwrap() = Some(vec![WmiObject::from([
        ("Capability".into(), WmiValue::U32(3)),
        ("ClockID".into(), WmiValue::U32(7)),
        ("defaultvalue".into(), WmiValue::U32(150)),
        ("Interval".into(), WmiValue::U32(5)),
        ("NOCOrderid".into(), WmiValue::U32(11)),
        ("OCMaxOffset".into(), WmiValue::U32(200)),
        ("OCMinOffset".into(), WmiValue::U32(0)),
        ("OCOffsetFreq".into(), WmiValue::U32(25)),
        ("OCOffsetScale".into(), WmiValue::U32(1)),
        ("OCOrderid".into(), WmiValue::U32(12)),
        ("PStateID".into(), WmiValue::U32(0)),
    ])]);
    let gpu = WindowsGpuOc::new(&fake);

    let status = gpu.gpu_oc_status().unwrap();

    assert!(status.supported);
    assert!(status.parameter_error.is_none());
    let parameter = status.parameter.expect("a published row is decoded");
    assert_eq!(parameter.capability, 3);
    assert_eq!(parameter.clock_id, 7);
    assert_eq!(parameter.default_value, 150);
    assert_eq!(parameter.interval, 5);
    assert_eq!(parameter.no_oc_order_id, 11);
    assert_eq!(parameter.oc_max_offset, 200);
    assert_eq!(parameter.oc_min_offset, 0);
    assert_eq!(parameter.oc_offset_freq, 25);
    assert_eq!(parameter.oc_offset_scale, 1);
    assert_eq!(parameter.oc_order_id, 12);
    assert_eq!(parameter.pstate_id, 0);
}

#[test]
fn incomplete_parameter_row_is_refused_instead_of_defaulted() {
    let fake = FakeWmi::default();
    *fake.support.lock().unwrap() = (true, 1);
    // Every declared field except `PStateID`: a reader must not invent a
    // default for the omitted property.
    *fake.oc_rows.lock().unwrap() = Some(vec![WmiObject::from([
        ("Capability".into(), WmiValue::U32(3)),
        ("ClockID".into(), WmiValue::U32(7)),
        ("defaultvalue".into(), WmiValue::U32(150)),
        ("Interval".into(), WmiValue::U32(5)),
        ("NOCOrderid".into(), WmiValue::U32(11)),
        ("OCMaxOffset".into(), WmiValue::U32(200)),
        ("OCMinOffset".into(), WmiValue::U32(0)),
        ("OCOffsetFreq".into(), WmiValue::U32(25)),
        ("OCOffsetScale".into(), WmiValue::U32(1)),
        ("OCOrderid".into(), WmiValue::U32(12)),
    ])]);
    let gpu = WindowsGpuOc::new(&fake);

    let status = gpu.gpu_oc_status().unwrap();

    assert!(status.supported);
    assert!(status.parameter.is_none());
    let reason = status.parameter_error.expect("the omission is reported");
    assert!(reason.contains("PStateID"), "missing field is named: {reason}");
}

#[test]
fn ambiguous_parameter_rows_are_refused() {
    let fake = FakeWmi::default();
    *fake.support.lock().unwrap() = (true, 1);
    let row = WmiObject::from([("Capability".into(), WmiValue::U32(1))]);
    *fake.oc_rows.lock().unwrap() = Some(vec![row.clone(), row]);
    let gpu = WindowsGpuOc::new(&fake);

    let status = gpu.gpu_oc_status().unwrap();

    assert!(status.parameter.is_none());
    let reason = status.parameter_error.expect("the ambiguity is reported");
    assert!(
        reason.contains("2 parameter blocks"),
        "row count is named: {reason}"
    );
}

#[test]
fn failed_probe_is_an_error_not_an_unsupported_answer() {
    let fake = FakeWmi::default();
    *fake.support.lock().unwrap() = (false, 0);
    let gpu = WindowsGpuOc::new(&fake);

    let error = gpu.gpu_oc_status().unwrap_err();

    assert!(
        matches!(&error, LctrlError::FirmwareRejected { .. }),
        "ReturnValue=False must not be decoded as Data=0: {error:?}"
    );
}
