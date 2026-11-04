//! CLI contract for the per-process telemetry and policy commands.
//!
//! The service is a fake, so these tests prove the CLI contract only: the
//! command routing, the dry-run/commit split, the target resolution and the
//! restore path. Native effect on a real process is proven by the platform
//! smoke, not here.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use lctrl_core::{
    ApplyMode, Availability, CapabilitySet, ChangeReport, HardwareInfo, LctrlError, Platform,
    ProcessAffinityPolicy, ProcessIdentity, ProcessInventory, ProcessInventoryEntry, ProcessPolicy,
    ProcessPriorityClass, ProcessSnapshot, ProcessState, ProcessTarget, ProcessWorkingSet, Result,
};
use lctrl_hal::{Hal, ProcessControl};
use vantage_cli::{Cli, CommandServices, execute_with_services};

struct FakeHal;

impl Hal for FakeHal {
    fn platform(&self) -> Platform {
        Platform::Windows
    }

    fn hardware_info(&self) -> Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: Some("21VG".into()),
            family: Some("21VG".into()),
            bios_version: Some("1.07".into()),
        })
    }

    fn capabilities(&self) -> Result<CapabilitySet> {
        let mut capabilities = CapabilitySet::new(Platform::Windows);
        capabilities.record("perf.top", Availability::Available, None)?;
        capabilities
            .record("perf.boost", Availability::Available, None)
            .map(|_| ())?;
        capabilities
            .record("perf.throttle", Availability::Available, None)
            .map(|_| ())?;
        Ok(capabilities)
    }
}

#[derive(Default)]
struct FakeProcess {
    writes: AtomicUsize,
    restores: AtomicUsize,
    requested_limit: AtomicUsize,
    boosted: Mutex<Vec<u32>>,
    throttled: Mutex<Vec<u32>>,
    known_pids: Mutex<Vec<u32>>,
}

impl FakeProcess {
    fn with_pids(pids: &[u32]) -> Self {
        Self {
            known_pids: Mutex::new(pids.to_vec()),
            ..Self::default()
        }
    }
}

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1_000 + u64::from(pid)).expect("valid identity")
}

fn snapshot(pid: u32) -> ProcessSnapshot {
    ProcessSnapshot {
        identity: identity(pid),
        image_name: format!("worker{pid}.exe"),
        image_path: Some(format!(r"C:\Apps\worker{pid}.exe")),
        system_affinity_mask: 0b1111,
        state: ProcessState {
            priority_class: ProcessPriorityClass::Normal,
            affinity_mask: 0b1111,
            working_set: Some(
                ProcessWorkingSet::new(
                    4096,
                    8192,
                    ProcessWorkingSet::HARDWS_MIN_ENABLE | ProcessWorkingSet::HARDWS_MAX_DISABLE,
                )
                .expect("valid working set"),
            ),
        },
    }
}

fn inventory_entry(pid: u32) -> ProcessInventoryEntry {
    ProcessInventoryEntry {
        identity: identity(pid),
        image_name: format!("worker{pid}.exe"),
        image_path: Some(format!(r"C:\Apps\worker{pid}.exe")),
        total_cpu_time_100ns: 5_000,
        interval_cpu_time_100ns: 1_500,
        interval_wall_time_100ns: 1_000_000,
        working_set_bytes: Some(4096),
    }
}

impl ProcessControl for FakeProcess {
    fn top_processes(&self, limit: usize) -> Result<ProcessInventory> {
        self.requested_limit.store(limit, Ordering::SeqCst);
        Ok(ProcessInventory::new(
            (1..=3).map(inventory_entry).collect(),
            4,
        ))
    }

    fn resolve_target(&self, pid: u32) -> Result<ProcessTarget> {
        if !self.known_pids.lock().expect("pid lock").contains(&pid) {
            return Err(LctrlError::InvalidArgument {
                detail: format!("no process with pid {pid}"),
            });
        }
        ProcessTarget::new(identity(pid), format!("worker{pid}.exe"))
    }

    fn process_snapshot(&self, target: &ProcessTarget) -> Result<ProcessSnapshot> {
        Ok(snapshot(target.identity().pid))
    }

    fn boost_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        self.boosted
            .lock()
            .expect("boost lock")
            .push(target.identity().pid);
        let previous = snapshot(target.identity().pid).state;
        let requested = ProcessState {
            priority_class: policy.priority_class.unwrap_or(previous.priority_class),
            ..previous
        };
        if apply == ApplyMode::Commit {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(ChangeReport::committed(previous, requested, requested))
        } else {
            Ok(ChangeReport::dry_run(previous, requested))
        }
    }

    fn throttle_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        self.throttled
            .lock()
            .expect("throttle lock")
            .push(target.identity().pid);
        let previous = snapshot(target.identity().pid).state;
        let requested = ProcessState {
            priority_class: policy.priority_class.unwrap_or(previous.priority_class),
            ..previous
        };
        if apply == ApplyMode::Commit {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(ChangeReport::committed(previous, requested, requested))
        } else {
            Ok(ChangeReport::dry_run(previous, requested))
        }
    }

    fn restore_process(
        &self,
        _target: &ProcessTarget,
        snapshot: &ProcessSnapshot,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        self.restores.fetch_add(1, Ordering::SeqCst);
        if apply == ApplyMode::Commit {
            self.writes.fetch_add(1, Ordering::SeqCst);
        }
        Ok(ChangeReport::committed(
            snapshot.state,
            snapshot.state,
            snapshot.state,
        ))
    }
}

#[test]
fn perf_top_reports_inventory_with_the_documented_limit() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "top"]).expect("parse perf top");
    let hal = FakeHal;
    let process = FakeProcess::default();

    let output = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf top succeeds");

    assert_eq!(process.requested_limit.load(Ordering::SeqCst), 10);
    assert_eq!(output.json["processes"].as_array().map(Vec::len), Some(3));
    assert_eq!(output.json["skipped_processes"], 4);
    assert_eq!(process.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn perf_top_without_service_is_structured_unsupported() {
    let cli = Cli::try_parse_from(["vantage", "perf", "top"]).expect("parse perf top");
    let hal = FakeHal;

    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("missing process service must not succeed");

    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert!(error.to_string().contains("perf.top"));
}

#[test]
fn perf_boost_dry_run_reports_the_change_without_writing() {
    let cli = Cli::try_parse_from(["vantage", "--json", "--dry-run", "perf", "boost", "4242"])
        .expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    let output = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf boost dry run succeeds");

    assert_eq!(process.writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        process.boosted.lock().expect("boost lock").as_slice(),
        &[4242]
    );
    assert_eq!(output.json[0]["pid"], 4242);
    assert_eq!(output.json[0]["applied"], false);
    assert_eq!(output.json[0]["previous"]["priority_class"], "normal");
    assert_eq!(
        output.json[0]["requested"]["priority_class"],
        "above_normal"
    );
    assert!(output.json[0]["actual"].is_null());
}

#[test]
fn perf_boost_commits_the_write_and_reports_the_readback() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "boost", "4242"])
        .expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    let output = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf boost succeeds");

    assert_eq!(process.writes.load(Ordering::SeqCst), 1);
    assert_eq!(output.json[0]["applied"], true);
    assert_eq!(output.json[0]["actual"]["priority_class"], "above_normal");
}

#[test]
fn perf_throttle_applies_the_inverse_priority() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "throttle", "4242"])
        .expect("parse perf throttle");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    let output = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf throttle succeeds");

    assert_eq!(
        process.throttled.lock().expect("throttle lock").as_slice(),
        &[4242]
    );
    assert_eq!(
        output.json[0]["requested"]["priority_class"],
        "below_normal"
    );
    assert_eq!(process.writes.load(Ordering::SeqCst), 1);
}

#[test]
fn perf_boost_auto_restore_puts_the_previous_state_back() {
    let cli = Cli::try_parse_from([
        "vantage",
        "--json",
        "--auto-restore",
        "perf",
        "boost",
        "4242",
    ])
    .expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf boost with auto restore succeeds");

    assert_eq!(process.restores.load(Ordering::SeqCst), 1);
    assert_eq!(process.writes.load(Ordering::SeqCst), 2);
}

#[test]
fn perf_boost_rejects_a_pid_that_cannot_be_resolved() {
    let cli = Cli::try_parse_from(["vantage", "perf", "boost", "7"]).expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    let error = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect_err("an unknown pid must not succeed");

    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
    assert_eq!(process.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn perf_boost_requires_at_least_one_pid() {
    let cli = Cli::try_parse_from(["vantage", "perf", "boost"]).expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::default();

    let error = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect_err("an empty pid list must not succeed");

    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
    assert_eq!(process.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn perf_boost_without_service_is_structured_unsupported() {
    let cli = Cli::try_parse_from(["vantage", "perf", "boost", "4242"]).expect("parse perf boost");
    let hal = FakeHal;

    let error = execute_with_services(cli, CommandServices::new(&hal))
        .expect_err("missing process service must not succeed");

    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert!(error.to_string().contains("perf.boost"));
}

#[test]
fn perf_policy_never_touches_affinity_or_working_set() {
    let cli = Cli::try_parse_from(["vantage", "--json", "perf", "boost", "4242"])
        .expect("parse perf boost");
    let hal = FakeHal;
    let process = FakeProcess::with_pids(&[4242]);

    let output = execute_with_services(cli, CommandServices::new(&hal).with_process(&process))
        .expect("perf boost succeeds");

    let requested = &output.json[0]["requested"];
    assert_eq!(
        requested["affinity_mask"],
        output.json[0]["previous"]["affinity_mask"]
    );
    assert_eq!(
        requested["working_set"],
        output.json[0]["previous"]["working_set"]
    );
    assert_eq!(
        ProcessAffinityPolicy::Preserve,
        ProcessPolicy::boost().affinity
    );
    assert!(ProcessPolicy::boost().working_set.is_none());
}
