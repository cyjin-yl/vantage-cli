#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use std::time::Duration;

use lctrl_core::{
    ApplyMode, LctrlError, MAX_PROCESS_TOP_LIMIT, ProcessAffinityPolicy, ProcessIdentity,
    ProcessInventory, ProcessInventoryEntry, ProcessPolicy, ProcessPriorityClass, ProcessSnapshot,
    ProcessState, ProcessTarget, ProcessWorkingSet, Result,
};
use lctrl_hal::ProcessControl;
use lctrl_hal_win::{ProcessApi, ProcessReadbackPolicy, WindowsProcessController};
use parking_lot::Mutex;

struct FakeProcessApi {
    current: Mutex<ProcessSnapshot>,
    inventory: Mutex<ProcessInventory>,
    writes: Mutex<Vec<&'static str>>,
    snapshot_calls: Mutex<usize>,
    mismatch_readbacks: Mutex<usize>,
    access_lost: Mutex<bool>,
    reuse_before_priority: Mutex<bool>,
}

impl FakeProcessApi {
    fn new(current: ProcessSnapshot) -> Self {
        Self {
            current: Mutex::new(current),
            inventory: Mutex::new(ProcessInventory::new(Vec::new(), 0)),
            writes: Mutex::new(Vec::new()),
            snapshot_calls: Mutex::new(0),
            mismatch_readbacks: Mutex::new(0),
            access_lost: Mutex::new(false),
            reuse_before_priority: Mutex::new(false),
        }
    }

    fn guard(&self, identity: &ProcessIdentity) -> Result<()> {
        if *self.access_lost.lock() {
            return Err(LctrlError::PermissionDenied {
                need: "process access".into(),
            });
        }
        if &self.current.lock().identity != identity {
            return Err(LctrlError::InvalidArgument {
                detail: "PID was reused before native mutation".into(),
            });
        }
        Ok(())
    }
}

impl ProcessApi for FakeProcessApi {
    fn inventory(&self, _limit: usize) -> Result<ProcessInventory> {
        Ok(self.inventory.lock().clone())
    }

    fn snapshot(&self, _identity: &ProcessIdentity) -> Result<ProcessSnapshot> {
        *self.snapshot_calls.lock() += 1;
        if *self.access_lost.lock() {
            return Err(LctrlError::PermissionDenied {
                need: "process access".into(),
            });
        }
        let mut snapshot = self.current.lock().clone();
        let mut mismatches = self.mismatch_readbacks.lock();
        if *self.snapshot_calls.lock() > 1 && *mismatches > 0 {
            *mismatches -= 1;
            snapshot.state.priority_class = ProcessPriorityClass::Normal;
        }
        Ok(snapshot)
    }

    fn set_priority_class(
        &self,
        identity: &ProcessIdentity,
        priority: ProcessPriorityClass,
    ) -> Result<()> {
        if std::mem::take(&mut *self.reuse_before_priority.lock()) {
            self.current.lock().identity.creation_time_100ns += 1;
        }
        self.guard(identity)?;
        self.writes.lock().push("priority");
        self.current.lock().state.priority_class = priority;
        Ok(())
    }

    fn set_affinity_mask(&self, identity: &ProcessIdentity, mask: usize) -> Result<()> {
        self.guard(identity)?;
        self.writes.lock().push("affinity");
        self.current.lock().state.affinity_mask = mask;
        Ok(())
    }

    fn set_working_set(
        &self,
        identity: &ProcessIdentity,
        working_set: ProcessWorkingSet,
    ) -> Result<()> {
        self.guard(identity)?;
        self.writes.lock().push("working_set");
        self.current.lock().state.working_set = working_set;
        Ok(())
    }
}

fn snapshot() -> ProcessSnapshot {
    ProcessSnapshot {
        identity: ProcessIdentity::new(123, 1_234_567).unwrap(),
        image_name: "authorized.exe".into(),
        image_path: Some(r"C:\Apps\authorized.exe".into()),
        system_affinity_mask: 0b1111,
        state: ProcessState {
            priority_class: ProcessPriorityClass::Normal,
            affinity_mask: 0b1111,
            working_set: ProcessWorkingSet::new(
                4096,
                8192,
                ProcessWorkingSet::HARDWS_MIN_ENABLE | ProcessWorkingSet::HARDWS_MAX_DISABLE,
            )
            .unwrap(),
        },
    }
}

fn target(snapshot: &ProcessSnapshot) -> ProcessTarget {
    ProcessTarget::new(snapshot.identity, snapshot.image_name.clone()).unwrap()
}

fn controller(api: FakeProcessApi, attempts: usize) -> WindowsProcessController<FakeProcessApi> {
    WindowsProcessController::with_readback_policy(
        api,
        ProcessReadbackPolicy {
            attempts,
            delay: Duration::ZERO,
        },
    )
    .unwrap()
}

fn explicit_throttle() -> ProcessPolicy {
    ProcessPolicy {
        priority_class: Some(ProcessPriorityClass::BelowNormal),
        affinity: ProcessAffinityPolicy::Mask(0b0011),
        working_set: Some(ProcessWorkingSet::new(2048, 4096, 0).unwrap()),
    }
}

#[test]
fn pid_reuse_is_rejected_before_any_mutation() {
    let original = snapshot();
    let target = target(&original);
    let mut reused = original;
    reused.identity.creation_time_100ns += 1;
    let controller = controller(FakeProcessApi::new(reused), 1);

    assert!(matches!(
        controller.boost_process(&target, ProcessPolicy::boost(), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());
}

#[test]
fn reuse_between_snapshot_and_setter_is_rejected_by_the_identity_guard() {
    let previous = snapshot();
    let target = target(&previous);
    let api = FakeProcessApi::new(previous.clone());
    *api.reuse_before_priority.lock() = true;
    let controller = controller(api, 1);

    assert!(matches!(
        controller.boost_process(&target, ProcessPolicy::boost(), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());
    assert_eq!(controller.api().current.lock().state, previous.state);
}

#[test]
fn dry_run_reads_but_performs_no_mutation() {
    let previous = snapshot();
    let target = target(&previous);
    let controller = controller(FakeProcessApi::new(previous.clone()), 1);

    let report = controller
        .throttle_process(&target, explicit_throttle(), ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.previous(), &previous.state);
    assert_eq!(report.actual(), None);
    assert_eq!(
        report.requested().priority_class,
        ProcessPriorityClass::BelowNormal
    );
    assert_eq!(controller.api().current.lock().state, previous.state);
    assert!(controller.api().writes.lock().is_empty());
    assert_eq!(*controller.api().snapshot_calls.lock(), 1);
}

#[test]
fn commit_uses_bounded_independent_readback() {
    let previous = snapshot();
    let target = target(&previous);
    let api = FakeProcessApi::new(previous.clone());
    *api.mismatch_readbacks.lock() = 1;
    let controller = controller(api, 2);

    let report = controller
        .boost_process(&target, ProcessPolicy::boost(), ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(report.requested()));
    assert_eq!(
        report.requested().priority_class,
        ProcessPriorityClass::AboveNormal
    );
    assert_eq!(
        report.requested().affinity_mask,
        previous.state.affinity_mask
    );
    assert_eq!(report.requested().working_set, previous.state.working_set);
    assert_eq!(*controller.api().snapshot_calls.lock(), 3);
    assert_eq!(&*controller.api().writes.lock(), &["priority"]);
}

#[test]
fn failed_readback_rolls_back_every_touched_field_exactly() {
    let previous = snapshot();
    let target = target(&previous);
    let api = FakeProcessApi::new(previous.clone());
    *api.mismatch_readbacks.lock() = 1;
    let controller = controller(api, 1);

    assert!(matches!(
        controller.throttle_process(&target, explicit_throttle(), ApplyMode::Commit),
        Err(LctrlError::VerifyMismatch { .. })
    ));
    assert_eq!(controller.api().current.lock().state, previous.state);
    assert_eq!(*controller.api().snapshot_calls.lock(), 3);
    assert_eq!(
        &*controller.api().writes.lock(),
        &[
            "priority",
            "affinity",
            "working_set",
            "working_set",
            "affinity",
            "priority"
        ]
    );
}

#[test]
fn explicit_restore_recovers_priority_affinity_and_working_set_flags() {
    let previous = snapshot();
    let target = target(&previous);
    let controller = controller(FakeProcessApi::new(previous.clone()), 1);
    let saved = controller.process_snapshot(&target).unwrap();

    controller
        .throttle_process(&target, explicit_throttle(), ApplyMode::Commit)
        .unwrap();
    assert_ne!(controller.api().current.lock().state, saved.state);
    let report = controller
        .restore_process(&target, &saved, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&saved.state));
    assert_eq!(controller.api().current.lock().state, previous.state);
    assert_eq!(
        controller.api().current.lock().state.working_set.flags,
        ProcessWorkingSet::HARDWS_MIN_ENABLE | ProcessWorkingSet::HARDWS_MAX_DISABLE
    );
}

fn inventory_entry(pid: u32, cpu: u64) -> ProcessInventoryEntry {
    ProcessInventoryEntry {
        identity: ProcessIdentity::new(pid, u64::from(pid) + 1000).unwrap(),
        image_name: format!("process-{pid}.exe"),
        image_path: None,
        total_cpu_time_100ns: cpu + 1000,
        interval_cpu_time_100ns: cpu,
        interval_wall_time_100ns: 1000,
        working_set_bytes: Some(pid as usize * 4096),
    }
}

#[test]
fn top_listing_is_cpu_sorted_and_bounded() {
    let api = FakeProcessApi::new(snapshot());
    *api.inventory.lock() = ProcessInventory::new(
        vec![
            inventory_entry(1, 10),
            inventory_entry(2, 400),
            inventory_entry(3, 200),
            inventory_entry(4, 300),
        ],
        3,
    );
    let controller = controller(api, 1);

    let inventory = controller.top_processes(2).unwrap();
    assert_eq!(inventory.processes.len(), 2);
    assert_eq!(inventory.processes[0].identity.pid, 2);
    assert_eq!(inventory.processes[1].identity.pid, 4);
    assert_eq!(inventory.skipped_processes, 3);
    assert_eq!(inventory.processes[0].cpu_utilization_ppm(), Some(400_000));
    assert!(matches!(
        controller.top_processes(0),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(matches!(
        controller.top_processes(MAX_PROCESS_TOP_LIMIT + 1),
        Err(LctrlError::InvalidArgument { .. })
    ));
}

#[test]
fn unsupported_topology_and_realtime_policies_never_mutate() {
    let previous = snapshot();
    let target = target(&previous);
    let controller = controller(FakeProcessApi::new(previous), 1);
    let mut policy = ProcessPolicy::boost();
    policy.affinity = ProcessAffinityPolicy::EfficiencyCores;
    assert!(matches!(
        controller.boost_process(&target, policy, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
    policy.affinity = ProcessAffinityPolicy::Preserve;
    policy.priority_class = Some(ProcessPriorityClass::Realtime);
    assert!(matches!(
        controller.boost_process(&target, policy, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());
    assert_eq!(*controller.api().snapshot_calls.lock(), 0);
}

#[test]
fn access_loss_and_wrong_authorized_name_reject_before_mutation() {
    let previous = snapshot();
    let target = target(&previous);
    let api = FakeProcessApi::new(previous.clone());
    *api.access_lost.lock() = true;
    let controller = controller(api, 1);
    assert!(matches!(
        controller.boost_process(&target, ProcessPolicy::boost(), ApplyMode::Commit),
        Err(LctrlError::PermissionDenied { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());

    *controller.api().access_lost.lock() = false;
    let wrong_name = ProcessTarget::new(previous.identity, "another.exe").unwrap();
    assert!(matches!(
        controller.boost_process(&wrong_name, ProcessPolicy::boost(), ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());
}

#[test]
fn restore_rejects_a_saved_snapshot_from_another_instance() {
    let previous = snapshot();
    let target = target(&previous);
    let controller = controller(FakeProcessApi::new(previous.clone()), 1);
    let mut wrong_snapshot = previous;
    wrong_snapshot.identity.creation_time_100ns += 1;
    assert!(matches!(
        controller.restore_process(&target, &wrong_snapshot, ApplyMode::Commit),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert!(controller.api().writes.lock().is_empty());
}
