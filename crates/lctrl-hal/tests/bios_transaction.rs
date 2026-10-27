use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use lctrl_core::{BiosApplyEffect, BiosChange, BiosItem, BiosPasswordStatus, LctrlError};
use lctrl_hal::{BiosControl, apply_bios_transaction, plan_bios_apply};

struct FakeBios {
    catalog: BTreeMap<String, (String, Vec<String>)>,
    committed: Mutex<BTreeMap<String, String>>,
    staged: Mutex<BTreeMap<String, String>>,
    readback_overrides: Mutex<BTreeMap<usize, BTreeMap<String, VecDeque<String>>>>,
    operations: Mutex<Vec<String>>,
    stage_calls: AtomicUsize,
    save_calls: AtomicUsize,
    discard_calls: AtomicUsize,
    fail_stage_at: Option<usize>,
    fail_save_at: Option<usize>,
}

impl FakeBios {
    fn new(settings: &[(&str, &str, &[&str])]) -> Self {
        let mut catalog = BTreeMap::new();
        let mut committed = BTreeMap::new();
        for (name, value, selections) in settings {
            let key = name.to_ascii_lowercase();
            catalog.insert(
                key.clone(),
                (
                    (*name).to_string(),
                    selections
                        .iter()
                        .map(|value| (*value).to_string())
                        .collect(),
                ),
            );
            committed.insert(key, (*value).to_string());
        }
        Self {
            catalog,
            committed: Mutex::new(committed),
            staged: Mutex::new(BTreeMap::new()),
            readback_overrides: Mutex::new(BTreeMap::new()),
            operations: Mutex::new(Vec::new()),
            stage_calls: AtomicUsize::new(0),
            save_calls: AtomicUsize::new(0),
            discard_calls: AtomicUsize::new(0),
            fail_stage_at: None,
            fail_save_at: None,
        }
    }

    fn fixture() -> Self {
        Self::new(&[
            ("Camera", "Enable", &["Enable", "Disable"]),
            ("Microphone", "Enable", &["Enable", "Disable"]),
        ])
    }

    fn fail_stage_at(mut self, call: usize) -> Self {
        self.fail_stage_at = Some(call);
        self
    }

    fn fail_save_at(mut self, call: usize) -> Self {
        self.fail_save_at = Some(call);
        self
    }

    fn with_readback_after_save(self, save: usize, name: &str, value: &str) -> Self {
        self.readback_overrides
            .lock()
            .unwrap()
            .entry(save)
            .or_default()
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push_back(value.to_string());
        self
    }

    fn record(&self, operation: String) {
        self.operations.lock().unwrap().push(operation);
    }

    fn operations(&self) -> Vec<String> {
        self.operations.lock().unwrap().clone()
    }

    fn current(&self, name: &str) -> String {
        self.committed
            .lock()
            .unwrap()
            .get(&name.to_ascii_lowercase())
            .unwrap()
            .clone()
    }

    fn saves(&self) -> usize {
        self.save_calls.load(Ordering::SeqCst)
    }

    fn discards(&self) -> usize {
        self.discard_calls.load(Ordering::SeqCst)
    }
}

impl BiosControl for FakeBios {
    fn list(&self) -> lctrl_core::Result<Vec<BiosItem>> {
        self.record("list".into());
        Ok(self
            .catalog
            .iter()
            .filter_map(|(key, (name, selections))| {
                Some(BiosItem {
                    name: name.clone(),
                    value: self.committed.lock().unwrap().get(key)?.clone(),
                    selections: selections.clone(),
                })
            })
            .collect())
    }

    fn get(&self, name: &str) -> lctrl_core::Result<Option<BiosItem>> {
        self.record(format!("get:{name}"));
        let key = name.to_ascii_lowercase();
        let Some((canonical, _)) = self.catalog.get(&key) else {
            return Ok(None);
        };
        let save = self.saves();
        let override_value = self
            .readback_overrides
            .lock()
            .unwrap()
            .get_mut(&save)
            .and_then(|settings| settings.get_mut(&key))
            .and_then(VecDeque::pop_front);
        let value = match override_value {
            Some(value) => value,
            None => {
                let Some(value) = self.committed.lock().unwrap().get(&key).cloned() else {
                    return Ok(None);
                };
                value
            }
        };
        Ok(Some(BiosItem {
            name: canonical.clone(),
            value,
            selections: Vec::new(),
        }))
    }

    fn selections(&self, name: &str) -> lctrl_core::Result<Vec<String>> {
        self.record(format!("selections:{name}"));
        self.catalog
            .get(&name.to_ascii_lowercase())
            .map(|(_, selections)| selections.clone())
            .ok_or_else(|| LctrlError::Unsupported {
                feature: format!("bios.selections.{name}"),
            })
    }

    fn stage(&self, change: BiosChange) -> lctrl_core::Result<()> {
        let call = self.stage_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.record(format!("stage:{}={}", change.name, change.value));
        if self.fail_stage_at == Some(call) {
            return Err(LctrlError::FirmwareRejected {
                detail: "injected stage failure".into(),
            });
        }
        self.staged.lock().unwrap().insert(
            change.name.as_str().to_ascii_lowercase(),
            change.value.as_str().to_string(),
        );
        Ok(())
    }

    fn save(&self) -> lctrl_core::Result<()> {
        let call = self.save_calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.record("save".into());
        if self.fail_save_at == Some(call) {
            return Err(LctrlError::FirmwareRejected {
                detail: "injected save failure".into(),
            });
        }
        let staged = std::mem::take(&mut *self.staged.lock().unwrap());
        let mut committed = self.committed.lock().unwrap();
        for (name, value) in staged {
            committed.insert(name, value);
        }
        Ok(())
    }

    fn discard(&self) -> lctrl_core::Result<()> {
        self.discard_calls.fetch_add(1, Ordering::SeqCst);
        self.record("discard".into());
        self.staged.lock().unwrap().clear();
        Ok(())
    }

    fn password_status(&self) -> lctrl_core::Result<BiosPasswordStatus> {
        Ok(BiosPasswordStatus::from_raw(1, 128, 0))
    }
}

fn change(name: &str, value: &str) -> BiosChange {
    BiosChange::new(name, value).unwrap()
}

fn requested_batch() -> Vec<BiosChange> {
    vec![change("Camera", "Disable"), change("Microphone", "Disable")]
}

#[test]
fn duplicate_names_are_rejected_before_any_firmware_access() {
    let bios = FakeBios::fixture();

    let error = apply_bios_transaction(
        &bios,
        &[change("Camera", "Disable"), change("camera", "Enable")],
    )
    .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::InvalidArgument { detail } if detail.contains("duplicate BIOS setting")
    ));
    assert!(bios.operations().is_empty());
}

#[test]
fn every_exact_selection_is_validated_before_the_first_stage() {
    let bios = FakeBios::fixture();

    let error = apply_bios_transaction(
        &bios,
        &[
            change("Camera", "Disable"),
            change("Microphone", "disabled"),
        ],
    )
    .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::InvalidArgument { detail }
            if detail.contains("exact selections") && detail.contains("disabled")
    ));
    assert_eq!(bios.saves(), 0);
    assert_eq!(bios.discards(), 0);
    assert!(
        !bios
            .operations()
            .iter()
            .any(|call| call.starts_with("stage:"))
    );
    assert_eq!(bios.current("Camera"), "Enable");
    assert_eq!(bios.current("Microphone"), "Enable");
}

#[test]
fn planning_captures_previous_values_without_staging() {
    let bios = FakeBios::fixture();

    let plan = plan_bios_apply(&bios, &requested_batch()).unwrap();

    assert_eq!(plan.changes.len(), 2);
    assert_eq!(plan.changes[0].previous, change("Camera", "Enable"));
    assert_eq!(plan.changes[0].requested, change("Camera", "Disable"));
    assert_eq!(plan.changes[1].previous, change("Microphone", "Enable"));
    assert_eq!(plan.changes[1].requested, change("Microphone", "Disable"));
    assert_eq!(bios.saves(), 0);
    assert!(
        !bios
            .operations()
            .iter()
            .any(|call| call.starts_with("stage:"))
    );
}

#[test]
fn partial_stage_failure_discards_without_saving() {
    let bios = FakeBios::fixture().fail_stage_at(2);

    let error = apply_bios_transaction(&bios, &requested_batch()).unwrap_err();

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { detail } if detail == "injected stage failure"
    ));
    assert_eq!(bios.saves(), 0);
    assert_eq!(bios.discards(), 1);
    assert_eq!(
        bios.operations().last().map(String::as_str),
        Some("discard")
    );
    assert_eq!(bios.current("Camera"), "Enable");
    assert_eq!(bios.current("Microphone"), "Enable");
}

#[test]
fn failed_save_discards_the_uncommitted_batch() {
    let bios = FakeBios::fixture().fail_save_at(1);

    let error = apply_bios_transaction(&bios, &requested_batch()).unwrap_err();

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { detail } if detail == "injected save failure"
    ));
    assert_eq!(bios.saves(), 1);
    assert_eq!(bios.discards(), 1);
    assert_eq!(bios.current("Camera"), "Enable");
    assert_eq!(bios.current("Microphone"), "Enable");
}

#[test]
fn successful_batch_saves_once_and_reads_back_every_mirror() {
    let bios = FakeBios::fixture();

    let result = apply_bios_transaction(&bios, &requested_batch()).unwrap();

    assert!(result.accepted);
    assert!(result.saved);
    assert_eq!(result.effect, BiosApplyEffect::PendingRebootOrUnknown);
    assert_eq!(result.changes.len(), 2);
    assert_eq!(result.changes[0].previous, change("Camera", "Enable"));
    assert_eq!(result.changes[0].requested, change("Camera", "Disable"));
    assert_eq!(result.changes[0].readback.value, "Disable");
    assert_eq!(result.changes[1].previous, change("Microphone", "Enable"));
    assert_eq!(result.changes[1].requested, change("Microphone", "Disable"));
    assert_eq!(result.changes[1].readback.value, "Disable");
    assert_eq!(bios.saves(), 1);
    assert_eq!(bios.discards(), 0);
    let operations = bios.operations();
    assert_eq!(
        &operations[operations.len() - 3..],
        ["save", "get:Camera", "get:Microphone"]
            .map(str::to_string)
            .as_slice()
    );
}

#[test]
fn readback_mismatch_restores_and_verifies_every_previous_value() {
    let bios = FakeBios::fixture().with_readback_after_save(1, "Camera", "Enable");

    let error = apply_bios_transaction(&bios, &requested_batch()).unwrap_err();

    assert!(matches!(
        error,
        LctrlError::VerifyMismatch { requested, actual }
            if requested == "Camera=Disable" && actual == "Camera=Enable"
    ));
    assert_eq!(bios.saves(), 2);
    assert_eq!(bios.discards(), 0);
    assert_eq!(bios.current("Camera"), "Enable");
    assert_eq!(bios.current("Microphone"), "Enable");
    let operations = bios.operations();
    assert_eq!(
        &operations[operations.len() - 6..],
        [
            "get:Camera",
            "stage:Camera=Enable",
            "stage:Microphone=Enable",
            "save",
            "get:Camera",
            "get:Microphone",
        ]
        .map(str::to_string)
        .as_slice()
    );
}

#[test]
fn recovery_failure_is_reported_with_the_original_readback_failure() {
    let bios = FakeBios::fixture()
        .with_readback_after_save(1, "Camera", "Enable")
        .fail_save_at(2);

    let error = apply_bios_transaction(&bios, &requested_batch()).unwrap_err();

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { detail }
            if detail.contains("BIOS apply readback failed")
                && detail.contains("Camera=Disable")
                && detail.contains("injected save failure")
    ));
    assert_eq!(bios.saves(), 2);
    assert_eq!(bios.discards(), 1);
}
