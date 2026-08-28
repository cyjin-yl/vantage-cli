use std::collections::BTreeMap;

use lctrl_core::{DiagnosticKind, DiagnosticOutcome, UpdateCapability};
use lctrl_hal::{DiagnosticsControl, UpdateControl};
use lctrl_hal_win::{WindowsSystemInventory, WmiObject, WmiTransport};

#[derive(Default)]
struct FakeWmi;

impl WmiTransport for FakeWmi {
    fn query(&self, _namespace: &str, _wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        Ok(vec![BTreeMap::new()])
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _object_path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        unreachable!("diagnostic inventory is read-only")
    }
}

#[test]
fn public_wmi_diagnostics_report_inventory_without_fake_deep_pass() {
    let inventory = WindowsSystemInventory::new(FakeWmi);

    let results = inventory
        .run_diagnostics(&[DiagnosticKind::Storage, DiagnosticKind::Firmware])
        .unwrap();

    assert!(
        results
            .iter()
            .all(|result| result.outcome == DiagnosticOutcome::Warning)
    );
    assert!(results.iter().all(|result| {
        result
            .detail
            .contains("proprietary-driver diagnostics are excluded")
    }));
}

#[test]
fn update_capability_fails_closed_without_authenticated_catalog() {
    let inventory = WindowsSystemInventory::new(FakeWmi);

    assert!(matches!(
        inventory.update_capability().unwrap(),
        UpdateCapability::Unavailable { reason } if reason.contains("authenticated public Lenovo update catalog")
    ));
}
