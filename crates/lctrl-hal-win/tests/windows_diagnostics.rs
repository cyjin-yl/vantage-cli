use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Condvar, atomic::{AtomicUsize, Ordering}},
    thread,
    time::{Duration, Instant},
};

use lctrl_core::{
    DiagnosticCancellationReason, DiagnosticCancellationToken, DiagnosticKind, DiagnosticOutcome,
    DiagnosticReason, DiagnosticRunOptions, DiagnosticRunState, DiagnosticValue, LctrlError,
    UpdateCapability,
};
use lctrl_hal::{DiagnosticsControl, UpdateControl};
use lctrl_hal_win::{WindowsSystemInventory, WmiObject, WmiTransport, WmiValue};

#[derive(Clone)]
enum Reply {
    Objects(Vec<WmiObject>),
    Unavailable,
    ResourceError,
}

#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn wait_entered(&self) {
        let entered = self.state.lock().unwrap();
        let (entered, timeout) = self.changed.wait_timeout_while(
            entered, Duration::from_secs(2), |entered| !entered.0,
        ).unwrap();
        assert!(entered.0 && !timeout.timed_out(), "provider never started");
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }

    fn block(&self) {
        self.state.lock().unwrap().0 = true;
        self.changed.notify_all();
        let released = self.state.lock().unwrap();
        drop(self.changed.wait_timeout_while(
            released, Duration::from_secs(2), |released| !released.1,
        ).unwrap());
    }
}

struct FakeWmi {
    reply: Reply,
    queries: Arc<AtomicUsize>,
    mutations: Arc<AtomicUsize>,
    gate: Option<Arc<Gate>>,
}

impl FakeWmi {
    fn objects(objects: Vec<WmiObject>) -> Self {
        Self {
            reply: Reply::Objects(objects),
            queries: Arc::new(AtomicUsize::new(0)),
            mutations: Arc::new(AtomicUsize::new(0)),
            gate: None,
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        assert!(namespace == "ROOT\\CIMV2" || namespace == "ROOT\\WMI");
        assert!(wql.starts_with("SELECT "));
        self.queries.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.block();
        }
        match &self.reply {
            Reply::Objects(objects) => Ok(objects.clone()),
            Reply::Unavailable => Err(LctrlError::ChannelUnavailable { channel: "missing public class".into() }),
            Reply::ResourceError => Err(LctrlError::PermissionDenied { need: "WMI access".into() }),
        }
    }

    fn invoke_instance(
        &self, _namespace: &str, _class: &str, _object_path: &str,
        _method: &str, _input: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        self.mutations.fetch_add(1, Ordering::SeqCst);
        panic!("diagnostics must not invoke device or service mutations")
    }
}

fn object(entries: &[(&str, WmiValue)]) -> WmiObject {
    entries.iter().map(|(key, value)| (key.to_string(), value.clone())).collect::<BTreeMap<_, _>>()
}

fn run(kind: DiagnosticKind, entries: &[(&str, WmiValue)]) -> lctrl_core::DiagnosticResult {
    WindowsSystemInventory::new(FakeWmi::objects(vec![object(entries)]))
        .run_diagnostics(&[kind], DiagnosticRunOptions::default()).unwrap().remove(0)
}

#[test]
fn battery_uses_reported_state_not_an_invented_charge_threshold() {
    let healthy = run(DiagnosticKind::Battery, &[
        ("BatteryStatus", WmiValue::U16(1)),
        ("EstimatedChargeRemaining", WmiValue::U16(1)),
    ]);
    assert_eq!(healthy.outcome, DiagnosticOutcome::Passed);
    assert_eq!(healthy.measurement("EstimatedChargeRemaining").unwrap().value, DiagnosticValue::Unsigned(1));
    assert!(healthy.threshold.is_none());
    let critical = run(DiagnosticKind::Battery, &[
        ("BatteryStatus", WmiValue::U16(5)),
        ("EstimatedChargeRemaining", WmiValue::U16(80)),
    ]);
    assert_eq!(critical.outcome, DiagnosticOutcome::Failed);
    let invalid = run(DiagnosticKind::Battery, &[
        ("BatteryStatus", WmiValue::U16(1)),
        ("EstimatedChargeRemaining", WmiValue::U16(101)),
    ]);
    assert_eq!(invalid.outcome, DiagnosticOutcome::ResourceError);
}

#[test]
fn thermal_only_compares_a_source_supplied_critical_trip_point() {
    let readout = run(DiagnosticKind::Thermal, &[("CurrentTemperature", WmiValue::U32(3200))]);
    assert_eq!(readout.outcome, DiagnosticOutcome::Warning);
    assert!(readout.detail.contains("sensor_readout"));
    assert!(readout.threshold.is_none());
    for (current, expected) in [(3200, DiagnosticOutcome::Passed), (3500, DiagnosticOutcome::Failed)] {
        let result = run(DiagnosticKind::Thermal, &[
            ("CurrentTemperature", WmiValue::U32(current)),
            ("CriticalTripPoint", WmiValue::U32(3500)),
        ]);
        assert_eq!(result.outcome, expected);
        assert!(result.threshold.is_some());
        assert!(result.source.unwrap().contains("MSAcpi_ThermalZoneTemperature"));
    }
}

#[test]
fn real_status_failures_are_not_inventory_successes() {
    for kind in [DiagnosticKind::Storage, DiagnosticKind::Memory, DiagnosticKind::Firmware] {
        assert_eq!(run(kind, &[("Status", WmiValue::String("Error".into()))]).outcome, DiagnosticOutcome::Failed);
        assert_ne!(run(kind, &[]).outcome, DiagnosticOutcome::Passed);
    }
    let storage = run(DiagnosticKind::Storage, &[("Status", WmiValue::String("OK".into()))]);
    assert_eq!(storage.outcome, DiagnosticOutcome::Warning);
    assert_eq!(storage.reason, Some(DiagnosticReason::DeepTestUnavailable { test: "SMART".into() }));
    let memory = run(DiagnosticKind::Memory, &[("Status", WmiValue::String("OK".into()))]);
    assert_eq!(memory.reason, Some(DiagnosticReason::DeepTestUnavailable { test: "destructive memory stress".into() }));
    assert_eq!(run(DiagnosticKind::Firmware, &[("SMBIOSBIOSVersion", WmiValue::String("1.0".into()))]).outcome, DiagnosticOutcome::Warning);
}

#[test]
fn network_disabled_is_skipped_and_enabled_is_not_connectivity_proof() {
    assert_eq!(run(DiagnosticKind::Network, &[("NetEnabled", WmiValue::Bool(false))]).outcome, DiagnosticOutcome::Skipped);
    assert_eq!(run(DiagnosticKind::Network, &[("NetEnabled", WmiValue::Bool(true))]).outcome, DiagnosticOutcome::Warning);
    let result = run(DiagnosticKind::Network, &[
        ("NetEnabled", WmiValue::Bool(true)), ("Status", WmiValue::String("OK".into())),
    ]);
    assert_eq!(result.outcome, DiagnosticOutcome::Passed);
    assert!(result.detail.contains("not a connectivity guarantee"));
}

#[test]
fn unavailable_provider_and_resource_errors_remain_distinct() {
    for (reply, expected) in [
        (Reply::Objects(Vec::new()), DiagnosticOutcome::Unavailable),
        (Reply::Unavailable, DiagnosticOutcome::Unavailable),
        (Reply::ResourceError, DiagnosticOutcome::ResourceError),
    ] {
        let mut fake = FakeWmi::objects(Vec::new());
        fake.reply = reply;
        let result = WindowsSystemInventory::new(fake).run_diagnostics(
            &[DiagnosticKind::Firmware], DiagnosticRunOptions::default(),
        ).unwrap();
        assert_eq!(result[0].outcome, expected);
    }
}

#[test]
fn cancellation_before_execution_performs_no_reads_or_mutations() {
    let fake = FakeWmi::objects(Vec::new());
    let queries = Arc::clone(&fake.queries);
    let mutations = Arc::clone(&fake.mutations);
    let inventory = WindowsSystemInventory::new(fake);
    let token = DiagnosticCancellationToken::new();
    token.cancel();
    let results = inventory.run_diagnostics(
        &[DiagnosticKind::Battery, DiagnosticKind::Network],
        DiagnosticRunOptions::default().with_cancellation(token),
    ).unwrap();
    assert!(results.iter().all(|result| result.outcome == DiagnosticOutcome::Cancelled));
    assert_eq!(results[0].reason, Some(DiagnosticReason::Cancelled { reason: DiagnosticCancellationReason::Requested }));
    assert_eq!(queries.load(Ordering::SeqCst), 0);
    assert_eq!(mutations.load(Ordering::SeqCst), 0);
    assert_eq!(inventory.diagnostic_status().state, DiagnosticRunState::Cancelled);
}

#[test]
fn per_item_deadline_returns_without_waiting_for_a_blocked_provider() {
    let gate = Arc::new(Gate::default());
    let mut fake = FakeWmi::objects(Vec::new());
    fake.gate = Some(Arc::clone(&gate));
    let queries = Arc::clone(&fake.queries);
    let inventory = WindowsSystemInventory::new(fake);
    let started = Instant::now();
    let results = inventory.run_diagnostics(
        &[DiagnosticKind::Storage, DiagnosticKind::Memory],
        DiagnosticRunOptions::default().with_per_item_deadline(Duration::from_millis(30)),
    ).unwrap();
    let elapsed = started.elapsed();
    gate.release();
    assert!(elapsed < Duration::from_secs(1));
    assert_eq!(results[0].outcome, DiagnosticOutcome::ResourceError);
    assert_eq!(results[1].outcome, DiagnosticOutcome::Skipped);
    assert_eq!(results[0].reason, Some(DiagnosticReason::DeadlineExceeded { deadline_ms: 30 }));
    assert_eq!(queries.load(Ordering::SeqCst), 1);
    assert_eq!(inventory.diagnostic_status().state, DiagnosticRunState::ResourceError);
}

#[test]
fn cancellation_during_read_has_a_snapshot_and_does_not_start_the_next_item() {
    let gate = Arc::new(Gate::default());
    let mut fake = FakeWmi::objects(Vec::new());
    fake.gate = Some(Arc::clone(&gate));
    let mutations = Arc::clone(&fake.mutations);
    let queries = Arc::clone(&fake.queries);
    let inventory = Arc::new(WindowsSystemInventory::new(fake));
    let worker_inventory = Arc::clone(&inventory);
    let token = DiagnosticCancellationToken::new();
    let worker_token = token.clone();
    let handle = thread::spawn(move || worker_inventory.run_diagnostics(
        &[DiagnosticKind::Battery, DiagnosticKind::Network],
        DiagnosticRunOptions::default().with_cancellation(worker_token),
    ).unwrap());
    gate.wait_entered();
    let snapshot = inventory.diagnostic_status();
    assert_eq!(snapshot.state, DiagnosticRunState::Running);
    assert_eq!(snapshot.current_item, Some(DiagnosticKind::Battery));
    let cancelled_at = Instant::now();
    token.cancel();
    let results = handle.join().unwrap();
    let cancellation_elapsed = cancelled_at.elapsed();
    gate.release();
    assert!(cancellation_elapsed < Duration::from_secs(1));
    assert!(results.iter().all(|result| result.outcome == DiagnosticOutcome::Cancelled));
    assert_eq!(queries.load(Ordering::SeqCst), 1);
    assert_eq!(mutations.load(Ordering::SeqCst), 0);
    assert_eq!(inventory.diagnostic_status().state, DiagnosticRunState::Cancelled);
}

#[test]
fn update_capability_is_preserved_without_catalog_claims() {
    let inventory = WindowsSystemInventory::new(FakeWmi::objects(Vec::new()));
    assert!(matches!(inventory.update_capability().unwrap(), UpdateCapability::Unavailable { reason }
        if reason.contains("authenticated public Lenovo update catalog")));
}

#[test]
fn concurrent_run_is_a_resource_error_not_a_second_device_read() {
    let gate = Arc::new(Gate::default());
    let mut fake = FakeWmi::objects(Vec::new());
    fake.gate = Some(Arc::clone(&gate));
    let queries = Arc::clone(&fake.queries);
    let inventory = Arc::new(WindowsSystemInventory::new(fake));
    let active = Arc::clone(&inventory);
    let handle = thread::spawn(move || active.run_diagnostics(
        &[DiagnosticKind::Firmware], DiagnosticRunOptions::default(),
    ).unwrap());
    gate.wait_entered();
    let second = inventory.run_diagnostics(
        &[DiagnosticKind::Network], DiagnosticRunOptions::default(),
    ).unwrap();
    gate.release();
    handle.join().unwrap();
    assert_eq!(second[0].outcome, DiagnosticOutcome::ResourceError);
    assert_eq!(second[0].reason, Some(DiagnosticReason::ExecutorBusy));
    assert_eq!(queries.load(Ordering::SeqCst), 1);
}

#[test]
fn zero_deadline_does_not_start_a_provider_read() {
    let fake = FakeWmi::objects(Vec::new());
    let queries = Arc::clone(&fake.queries);
    let inventory = WindowsSystemInventory::new(fake);
    let results = inventory.run_diagnostics(
        &[DiagnosticKind::Memory],
        DiagnosticRunOptions::default().with_per_item_deadline(Duration::ZERO),
    ).unwrap();
    assert_eq!(results[0].outcome, DiagnosticOutcome::ResourceError);
    assert_eq!(queries.load(Ordering::SeqCst), 0);
}
