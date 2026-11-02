use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, ProcessAffinityPolicy, ProcessInventory,
    ProcessInventoryEntry, ProcessPolicy, ProcessPriorityClass, ProcessSnapshot, ProcessState,
    ProcessTarget, ProcessWorkingSet, Result, validate_process_top_limit,
};
use lctrl_hal::{ProcessControl, poll_readback};

/// Low-level Windows process operations. Every setter receives the exact
/// identity so the native implementation can open and verify one process handle
/// immediately before mutating it.
pub trait ProcessApi: Send + Sync {
    fn inventory(&self, limit: usize) -> Result<ProcessInventory>;
    fn resolve_target(&self, pid: u32) -> Result<ProcessTarget>;
    fn snapshot(&self, identity: &lctrl_core::ProcessIdentity) -> Result<ProcessSnapshot>;
    fn set_priority_class(
        &self,
        identity: &lctrl_core::ProcessIdentity,
        priority: ProcessPriorityClass,
    ) -> Result<()>;
    fn set_affinity_mask(&self, identity: &lctrl_core::ProcessIdentity, mask: usize) -> Result<()>;
    fn set_working_set(
        &self,
        identity: &lctrl_core::ProcessIdentity,
        working_set: ProcessWorkingSet,
    ) -> Result<()>;
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessReadbackPolicy {
    pub attempts: usize,
    pub delay: Duration,
}

impl Default for ProcessReadbackPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            delay: Duration::from_millis(25),
        }
    }
}

/// Transactional per-process control with bounded independent readback.
/// Failures after a possible write restore the fields touched by the request.
pub struct WindowsProcessController<A> {
    api: A,
    readback: ProcessReadbackPolicy,
}

impl<A> WindowsProcessController<A> {
    #[must_use]
    pub fn new(api: A) -> Self {
        Self {
            api,
            readback: ProcessReadbackPolicy::default(),
        }
    }

    pub fn with_readback_policy(api: A, readback: ProcessReadbackPolicy) -> Result<Self> {
        if readback.attempts == 0
            || readback.attempts > 10
            || readback.delay > Duration::from_secs(1)
        {
            return Err(LctrlError::InvalidArgument {
                detail: "process readback requires 1..=10 attempts and at most 1s between reads"
                    .into(),
            });
        }
        Ok(Self { api, readback })
    }

    #[must_use]
    pub const fn api(&self) -> &A {
        &self.api
    }

    #[must_use]
    pub const fn readback_policy(&self) -> ProcessReadbackPolicy {
        self.readback
    }
}

#[derive(Clone, Copy)]
enum ProcessOperation {
    Boost,
    Throttle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessField {
    Priority,
    Affinity,
    WorkingSet,
}

const PROCESS_FIELDS: [ProcessField; 3] = [
    ProcessField::Priority,
    ProcessField::Affinity,
    ProcessField::WorkingSet,
];

#[derive(Clone, Copy, Default)]
struct ProcessFields {
    priority: bool,
    affinity: bool,
    working_set: bool,
}

impl ProcessFields {
    fn between(previous: &ProcessState, requested: &ProcessState) -> Self {
        Self {
            priority: previous.priority_class != requested.priority_class,
            affinity: previous.affinity_mask != requested.affinity_mask,
            working_set: previous.working_set != requested.working_set,
        }
    }

    fn contains(self, field: ProcessField) -> bool {
        match field {
            ProcessField::Priority => self.priority,
            ProcessField::Affinity => self.affinity,
            ProcessField::WorkingSet => self.working_set,
        }
    }

    fn insert(&mut self, field: ProcessField) {
        match field {
            ProcessField::Priority => self.priority = true,
            ProcessField::Affinity => self.affinity = true,
            ProcessField::WorkingSet => self.working_set = true,
        }
    }

    fn is_empty(self) -> bool {
        !self.priority && !self.affinity && !self.working_set
    }

    fn iter(self) -> impl Iterator<Item = ProcessField> {
        PROCESS_FIELDS
            .into_iter()
            .filter(move |field| self.contains(*field))
    }

    fn iter_reversed(self) -> impl Iterator<Item = ProcessField> {
        PROCESS_FIELDS
            .into_iter()
            .rev()
            .filter(move |field| self.contains(*field))
    }
}

impl<A> WindowsProcessController<A>
where
    A: ProcessApi,
{
    fn validated_snapshot(&self, target: &ProcessTarget) -> Result<ProcessSnapshot> {
        target.identity().validate()?;
        let snapshot = self.api.snapshot(target.identity())?;
        if &snapshot.identity != target.identity() {
            return Err(identity_mismatch(target.identity(), &snapshot.identity));
        }
        if !target.accepts_image_name(&snapshot.image_name) {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "authorized image {} does not match {} for {}",
                    target.image_name(),
                    snapshot.image_name,
                    target.identity()
                ),
            });
        }
        if snapshot.system_affinity_mask == 0 {
            return Err(LctrlError::Unsupported {
                feature: "empty or multi-group system affinity mask".into(),
            });
        }
        snapshot.state.validate()?;
        Ok(snapshot)
    }

    fn requested_state(policy: ProcessPolicy, current: &ProcessSnapshot) -> Result<ProcessState> {
        let mut requested = current.state;
        if let Some(priority) = policy.priority_class {
            requested.priority_class = priority;
        }
        match policy.affinity {
            ProcessAffinityPolicy::Preserve => {}
            ProcessAffinityPolicy::Mask(mask) => {
                validate_affinity_mask(mask, current.system_affinity_mask)?;
                requested.affinity_mask = mask;
            }
            ProcessAffinityPolicy::PerformanceCores | ProcessAffinityPolicy::EfficiencyCores => {
                return Err(LctrlError::Unsupported {
                    feature: "P/E-core affinity labels require a topology-derived explicit mask"
                        .into(),
                });
            }
        }
        if let Some(working_set) = policy.working_set {
            requested.working_set = working_set;
        }
        Ok(requested)
    }

    fn transact(
        &self,
        target: &ProcessTarget,
        current: ProcessSnapshot,
        requested: ProcessState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        ensure_restorable(&current.state)?;
        ensure_restorable(&requested)?;
        validate_affinity_mask(requested.affinity_mask, current.system_affinity_mask)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(current.state, requested));
        }

        let fields = ProcessFields::between(&current.state, &requested);
        let identity = current.identity;
        let mut attempted = ProcessFields::default();
        let mut completed = ProcessFields::default();
        for field in fields.iter() {
            attempted.insert(field);
            if let Err(error) = self.set_field(&identity, field, &requested) {
                if completed.is_empty() && is_definite_prewrite_error(&error) {
                    return Err(error);
                }
                return Err(self.rollback(target, &current.state, attempted, error));
            }
            completed.insert(field);
        }

        match self.read_back(target, &requested) {
            Ok(actual) => Ok(ChangeReport::committed(current.state, requested, actual)),
            Err(error) if fields.is_empty() => Err(error),
            Err(error) => Err(self.rollback(target, &current.state, fields, error)),
        }
    }

    fn read_back(&self, target: &ProcessTarget, requested: &ProcessState) -> Result<ProcessState> {
        poll_readback(
            requested,
            self.readback.attempts,
            self.readback.delay,
            || {
                self.validated_snapshot(target)
                    .map(|snapshot| snapshot.state)
            },
        )
    }

    fn set_field(
        &self,
        identity: &lctrl_core::ProcessIdentity,
        field: ProcessField,
        state: &ProcessState,
    ) -> Result<()> {
        match field {
            ProcessField::Priority => self.api.set_priority_class(identity, state.priority_class),
            ProcessField::Affinity => self.api.set_affinity_mask(identity, state.affinity_mask),
            ProcessField::WorkingSet => self.api.set_working_set(identity, state.working_set),
        }
    }

    fn rollback(
        &self,
        target: &ProcessTarget,
        previous: &ProcessState,
        fields: ProcessFields,
        primary: LctrlError,
    ) -> LctrlError {
        for field in fields.iter_reversed() {
            if let Err(rollback) = self.set_field(target.identity(), field, previous) {
                return rollback_failure(primary, rollback);
            }
        }
        match self.read_back(target, previous) {
            Ok(_) => primary,
            Err(rollback) => rollback_failure(primary, rollback),
        }
    }
}

impl<A> ProcessControl for WindowsProcessController<A>
where
    A: ProcessApi,
{
    fn top_processes(&self, limit: usize) -> Result<ProcessInventory> {
        validate_process_top_limit(limit)?;
        let mut inventory = self.api.inventory(limit)?;
        sort_and_truncate(&mut inventory.processes, limit);
        Ok(inventory)
    }

    fn resolve_target(&self, pid: u32) -> Result<ProcessTarget> {
        self.api.resolve_target(pid)
    }

    fn process_snapshot(&self, target: &ProcessTarget) -> Result<ProcessSnapshot> {
        self.validated_snapshot(target)
    }

    fn boost_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        validate_policy(ProcessOperation::Boost, &policy)?;
        let current = self.validated_snapshot(target)?;
        let requested = Self::requested_state(policy, &current)?;
        self.transact(target, current, requested, apply)
    }

    fn throttle_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        validate_policy(ProcessOperation::Throttle, &policy)?;
        let current = self.validated_snapshot(target)?;
        let requested = Self::requested_state(policy, &current)?;
        self.transact(target, current, requested, apply)
    }

    fn restore_process(
        &self,
        target: &ProcessTarget,
        snapshot: &ProcessSnapshot,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        if &snapshot.identity != target.identity() {
            return Err(identity_mismatch(target.identity(), &snapshot.identity));
        }
        if !target.accepts_image_name(&snapshot.image_name) {
            return Err(LctrlError::InvalidArgument {
                detail: "restore snapshot image name differs from authorized process".into(),
            });
        }
        snapshot.state.validate()?;
        let current = self.validated_snapshot(target)?;
        self.transact(target, current, snapshot.state, apply)
    }
}

fn validate_policy(operation: ProcessOperation, policy: &ProcessPolicy) -> Result<()> {
    let Some(priority) = policy.priority_class else {
        return Err(LctrlError::InvalidArgument {
            detail: "process policy must explicitly select a priority class".into(),
        });
    };
    match operation {
        ProcessOperation::Boost => {
            if !matches!(
                priority,
                ProcessPriorityClass::AboveNormal | ProcessPriorityClass::High
            ) {
                return Err(LctrlError::Unsupported {
                    feature: format!("boost process priority {priority}"),
                });
            }
            if policy.working_set.is_some() {
                return Err(LctrlError::Unsupported {
                    feature: "boost working-set mutation".into(),
                });
            }
        }
        ProcessOperation::Throttle => {
            if priority != ProcessPriorityClass::BelowNormal {
                return Err(LctrlError::Unsupported {
                    feature: format!("throttle process priority {priority}"),
                });
            }
        }
    }
    if let Some(working_set) = &policy.working_set {
        working_set.validate()?;
    }
    match policy.affinity {
        ProcessAffinityPolicy::Preserve => Ok(()),
        ProcessAffinityPolicy::Mask(0) => Err(LctrlError::InvalidArgument {
            detail: "process affinity mask must be nonzero".into(),
        }),
        ProcessAffinityPolicy::Mask(_) => Ok(()),
        ProcessAffinityPolicy::PerformanceCores | ProcessAffinityPolicy::EfficiencyCores => {
            Err(LctrlError::Unsupported {
                feature: "P/E-core affinity labels require a topology-derived explicit mask".into(),
            })
        }
    }
}

fn ensure_restorable(state: &ProcessState) -> Result<()> {
    state.validate()?;
    if state.priority_class == ProcessPriorityClass::Realtime {
        return Err(LctrlError::Unsupported {
            feature: "mutating a realtime-priority process prevents safe realtime-free rollback"
                .into(),
        });
    }
    Ok(())
}

fn validate_affinity_mask(mask: usize, system_mask: usize) -> Result<()> {
    if mask == 0 {
        return Err(LctrlError::InvalidArgument {
            detail: "process affinity mask must be nonzero".into(),
        });
    }
    if system_mask == 0 || mask & !system_mask != 0 {
        return Err(LctrlError::InvalidArgument {
            detail: format!(
                "process affinity mask 0x{mask:x} is not a subset of system mask 0x{system_mask:x}"
            ),
        });
    }
    Ok(())
}

fn identity_mismatch(
    expected: &lctrl_core::ProcessIdentity,
    actual: &lctrl_core::ProcessIdentity,
) -> LctrlError {
    LctrlError::InvalidArgument {
        detail: format!("process identity changed; expected {expected}, actual {actual}"),
    }
}

fn is_definite_prewrite_error(error: &LctrlError) -> bool {
    matches!(
        error,
        LctrlError::PermissionDenied { .. }
            | LctrlError::ChannelUnavailable { .. }
            | LctrlError::InvalidArgument { .. }
            | LctrlError::Unsupported { .. }
    )
}

fn rollback_failure(primary: LctrlError, rollback: LctrlError) -> LctrlError {
    LctrlError::Io(std::io::Error::other(format!(
        "process mutation failed ({primary}); exact rollback also failed ({rollback})"
    )))
}

fn sort_and_truncate(entries: &mut Vec<ProcessInventoryEntry>, limit: usize) {
    entries.sort_unstable_by(|left, right| {
        right
            .interval_cpu_time_100ns
            .cmp(&left.interval_cpu_time_100ns)
            .then_with(|| left.identity.pid.cmp(&right.identity.pid))
            .then_with(|| {
                left.identity
                    .creation_time_100ns
                    .cmp(&right.identity.creation_time_100ns)
            })
    });
    entries.truncate(limit);
}

#[cfg(windows)]
mod native {
    use std::{
        collections::HashMap,
        thread,
        time::{Duration, Instant},
    };

    use lctrl_core::{
        LctrlError, ProcessIdentity, ProcessInventory, ProcessInventoryEntry, ProcessPriorityClass,
        ProcessSnapshot, ProcessState, ProcessTarget, ProcessWorkingSet, Result,
        validate_process_top_limit,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, FILETIME, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Memory::{
                GetProcessWorkingSetSizeEx, QUOTA_LIMITS_HARDWS_MAX_DISABLE,
                QUOTA_LIMITS_HARDWS_MAX_ENABLE, QUOTA_LIMITS_HARDWS_MIN_DISABLE,
                QUOTA_LIMITS_HARDWS_MIN_ENABLE, SetProcessWorkingSetSizeEx,
            },
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{
                ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetPriorityClass,
                GetProcessAffinityMask, GetProcessTimes, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
                NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA, QueryFullProcessImageNameW,
                REALTIME_PRIORITY_CLASS, SetPriorityClass, SetProcessAffinityMask,
            },
        },
    };

    use crate::map_win_error;

    use super::{ProcessApi, sort_and_truncate};

    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_NO_MORE_FILES: u32 = 18;
    const ERROR_INVALID_PARAMETER: u32 = 87;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    /// Native ToolHelp32/GetProcessTimes process source. CPU ranking uses two
    /// samples joined by exact PID + creation-time identity.
    #[derive(Clone, Copy, Debug)]
    pub struct NativeProcessApi {
        sample_interval: Duration,
    }

    impl Default for NativeProcessApi {
        fn default() -> Self {
            Self {
                sample_interval: Duration::from_millis(100),
            }
        }
    }

    impl NativeProcessApi {
        #[must_use]
        pub const fn new(sample_interval: Duration) -> Self {
            Self { sample_interval }
        }

        #[must_use]
        pub const fn sample_interval(&self) -> Duration {
            self.sample_interval
        }

        fn sampled_inventory(&self, limit: usize) -> Result<ProcessInventory> {
            validate_process_top_limit(limit)?;
            if self.sample_interval.is_zero() || self.sample_interval > Duration::from_secs(5) {
                return Err(LctrlError::InvalidArgument {
                    detail: "process CPU sample interval must be nonzero and at most 5s".into(),
                });
            }
            let started = Instant::now();
            let first = sample_processes(false)?;
            thread::sleep(self.sample_interval);
            let second = sample_processes(true)?;
            let elapsed = duration_100ns(started.elapsed());

            let mut baseline = HashMap::with_capacity(first.samples.len());
            for sample in first.samples {
                baseline.insert(sample.identity, sample);
            }
            let mut skipped = second.skipped;
            let mut entries = Vec::with_capacity(second.samples.len());
            for current in second.samples {
                let Some(previous) = baseline.get(&current.identity) else {
                    skipped += 1;
                    continue;
                };
                let Some(interval_cpu) = current
                    .times
                    .total_cpu_time_100ns
                    .checked_sub(previous.times.total_cpu_time_100ns)
                else {
                    skipped += 1;
                    continue;
                };
                entries.push(ProcessInventoryEntry {
                    identity: current.identity,
                    image_name: current.image_name,
                    image_path: current.image_path,
                    total_cpu_time_100ns: current.times.total_cpu_time_100ns,
                    interval_cpu_time_100ns: interval_cpu,
                    interval_wall_time_100ns: elapsed,
                    working_set_bytes: current.working_set_bytes,
                });
            }
            sort_and_truncate(&mut entries, limit);
            Ok(ProcessInventory::new(entries, skipped))
        }
    }

    impl ProcessApi for NativeProcessApi {
        fn inventory(&self, limit: usize) -> Result<ProcessInventory> {
            self.sampled_inventory(limit)
        }

        fn resolve_target(&self, pid: u32) -> Result<ProcessTarget> {
            // The creation time is read from the live process, so the target
            // carries the identity that later mutations re-verify.
            let process = ProcessHandle::open_any(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
            let image_path = query_process_image_path(process.raw())?;
            let image_name = image_name_from_path(&image_path)?.to_owned();
            ProcessTarget::new(process.identity, image_name)
        }

        fn snapshot(&self, identity: &ProcessIdentity) -> Result<ProcessSnapshot> {
            let process =
                ProcessHandle::open_expected(identity, PROCESS_QUERY_LIMITED_INFORMATION)?;
            let image_path = query_process_image_path(process.raw())?;
            let image_name = image_name_from_path(&image_path)?.to_owned();
            let (state, system_affinity_mask) = query_process_state(process.raw())?;
            Ok(ProcessSnapshot {
                identity: process.identity,
                image_name,
                image_path: Some(image_path),
                system_affinity_mask,
                state,
            })
        }

        fn set_priority_class(
            &self,
            identity: &ProcessIdentity,
            priority: ProcessPriorityClass,
        ) -> Result<()> {
            let priority = native_priority_for_write(priority)?;
            let process = ProcessHandle::open_expected(
                identity,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            )?;
            if unsafe { SetPriorityClass(process.raw(), priority) } == 0 {
                return Err(last_error("SetPriorityClass"));
            }
            Ok(())
        }

        fn set_affinity_mask(&self, identity: &ProcessIdentity, mask: usize) -> Result<()> {
            if mask == 0 {
                return Err(LctrlError::InvalidArgument {
                    detail: "process affinity mask must be nonzero".into(),
                });
            }
            let process = ProcessHandle::open_expected(
                identity,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            )?;
            let mut current_process_mask = 0usize;
            let mut system_mask = 0usize;
            if unsafe {
                GetProcessAffinityMask(process.raw(), &mut current_process_mask, &mut system_mask)
            } == 0
            {
                return Err(last_error("GetProcessAffinityMask before set"));
            }
            if current_process_mask == 0 || system_mask == 0 {
                return Err(LctrlError::Unsupported {
                    feature: "empty or multi-group process affinity mask".into(),
                });
            }
            if mask & !system_mask != 0 {
                return Err(LctrlError::InvalidArgument {
                    detail: format!(
                        "process affinity mask 0x{mask:x} exceeds system mask 0x{system_mask:x}"
                    ),
                });
            }
            if unsafe { SetProcessAffinityMask(process.raw(), mask) } == 0 {
                return Err(last_error("SetProcessAffinityMask"));
            }
            Ok(())
        }

        fn set_working_set(
            &self,
            identity: &ProcessIdentity,
            working_set: ProcessWorkingSet,
        ) -> Result<()> {
            working_set.validate()?;
            let native_flags = QUOTA_LIMITS_HARDWS_MIN_ENABLE
                | QUOTA_LIMITS_HARDWS_MIN_DISABLE
                | QUOTA_LIMITS_HARDWS_MAX_ENABLE
                | QUOTA_LIMITS_HARDWS_MAX_DISABLE;
            if working_set.flags & !native_flags != 0 {
                return Err(LctrlError::Unsupported {
                    feature: format!("native process working-set flags 0x{:x}", working_set.flags),
                });
            }
            let process = ProcessHandle::open_expected(
                identity,
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA,
            )?;
            if unsafe {
                SetProcessWorkingSetSizeEx(
                    process.raw(),
                    working_set.minimum_bytes,
                    working_set.maximum_bytes,
                    working_set.flags,
                )
            } == 0
            {
                return Err(last_error("SetProcessWorkingSetSizeEx"));
            }
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    struct ProcessTimes {
        creation_time_100ns: u64,
        total_cpu_time_100ns: u64,
    }

    struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        fn open_process(pid: u32, access: u32) -> Result<Self> {
            let handle = unsafe { OpenProcess(access, 0, pid) };
            if handle.is_null() {
                let error = unsafe { GetLastError() };
                return Err(match error {
                    ERROR_ACCESS_DENIED => LctrlError::PermissionDenied {
                        need: format!("access to process {pid} (administrator may be required)"),
                    },
                    ERROR_INVALID_PARAMETER => LctrlError::ChannelUnavailable {
                        channel: format!("process {pid} is no longer available"),
                    },
                    _ => map_win_error(error, &format!("OpenProcess {pid}")),
                });
            }
            Ok(Self(handle))
        }

        fn process_snapshot() -> Result<Self> {
            let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
            if handle == INVALID_HANDLE_VALUE {
                return Err(last_error("CreateToolhelp32Snapshot"));
            }
            Ok(Self(handle))
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct ProcessHandle {
        owned: OwnedHandle,
        identity: ProcessIdentity,
        times: ProcessTimes,
    }

    impl ProcessHandle {
        fn open_any(pid: u32, access: u32) -> Result<Self> {
            let owned = OwnedHandle::open_process(pid, access)?;
            let times = query_process_times(owned.0)?;
            let identity = ProcessIdentity::new(pid, times.creation_time_100ns)?;
            Ok(Self {
                owned,
                identity,
                times,
            })
        }

        fn open_expected(identity: &ProcessIdentity, access: u32) -> Result<Self> {
            identity.validate()?;
            let process = Self::open_any(identity.pid, access)?;
            if &process.identity != identity {
                return Err(super::identity_mismatch(identity, &process.identity));
            }
            Ok(process)
        }

        fn raw(&self) -> HANDLE {
            self.owned.0
        }
    }

    struct ProcessSample {
        identity: ProcessIdentity,
        image_name: String,
        image_path: Option<String>,
        times: ProcessTimes,
        working_set_bytes: Option<usize>,
    }

    struct ProcessSampleBatch {
        samples: Vec<ProcessSample>,
        skipped: usize,
    }

    fn sample_processes(include_details: bool) -> Result<ProcessSampleBatch> {
        let snapshot = OwnedHandle::process_snapshot()?;
        let mut samples = Vec::new();
        let mut skipped = 0usize;
        let mut entry = process_entry();
        if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                return Ok(ProcessSampleBatch { samples, skipped });
            }
            return Err(map_win_error(error, "Process32FirstW"));
        }
        loop {
            let image_name = if include_details {
                toolhelp_image_name(&entry)
            } else {
                Ok(String::new())
            };
            let sampled = image_name.and_then(|image_name| {
                sample_process(entry.th32ProcessID, image_name, include_details)
            });
            match sampled {
                Ok(sample) => samples.push(sample),
                Err(_) => skipped += 1,
            }

            entry = process_entry();
            if unsafe { Process32NextW(snapshot.0, &mut entry) } != 0 {
                continue;
            }
            let error = unsafe { GetLastError() };
            if error == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(map_win_error(error, "Process32NextW"));
        }
        Ok(ProcessSampleBatch { samples, skipped })
    }

    fn process_entry() -> PROCESSENTRY32W {
        PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..PROCESSENTRY32W::default()
        }
    }

    fn sample_process(
        pid: u32,
        image_name: String,
        include_details: bool,
    ) -> Result<ProcessSample> {
        let process = ProcessHandle::open_any(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let image_path = if include_details {
            query_process_image_path(process.raw()).ok()
        } else {
            None
        };
        let working_set_bytes = if include_details {
            query_working_set_bytes(process.raw()).ok()
        } else {
            None
        };
        Ok(ProcessSample {
            identity: process.identity,
            image_name,
            image_path,
            times: process.times,
            working_set_bytes,
        })
    }

    fn toolhelp_image_name(entry: &PROCESSENTRY32W) -> Result<String> {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        String::from_utf16(&entry.szExeFile[..length]).map_err(|error| {
            LctrlError::ChannelUnavailable {
                channel: format!("ToolHelp process image name is invalid UTF-16: {error}"),
            }
        })
    }

    fn query_process_times(handle: HANDLE) -> Result<ProcessTimes> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0
        {
            return Err(last_error("GetProcessTimes"));
        }
        Ok(ProcessTimes {
            creation_time_100ns: filetime_value(&creation),
            total_cpu_time_100ns: filetime_value(&kernel).saturating_add(filetime_value(&user)),
        })
    }

    fn filetime_value(value: &FILETIME) -> u64 {
        u64::from(value.dwLowDateTime) | (u64::from(value.dwHighDateTime) << 32)
    }

    fn query_process_image_path(handle: HANDLE) -> Result<String> {
        let mut buffer = vec![0u16; 1024];
        for attempt in 0..2 {
            let mut length = buffer.len() as u32;
            if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) }
                != 0
            {
                buffer.truncate(length as usize);
                return String::from_utf16(&buffer).map_err(|error| {
                    LctrlError::ChannelUnavailable {
                        channel: format!("process image path is invalid UTF-16: {error}"),
                    }
                });
            }
            let error = unsafe { GetLastError() };
            if attempt == 0 && error == ERROR_INSUFFICIENT_BUFFER {
                buffer.resize(32_768, 0);
                continue;
            }
            return Err(map_win_error(error, "QueryFullProcessImageNameW"));
        }
        unreachable!("bounded process image query returns or resizes once")
    }

    fn image_name_from_path(path: &str) -> Result<&str> {
        let start = path.rfind(['\\', '/']).map_or(0, |index| index + 1);
        let name = &path[start..];
        if name.is_empty() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "QueryFullProcessImageNameW returned an empty image name".into(),
            });
        }
        Ok(name)
    }

    fn query_working_set_bytes(handle: HANDLE) -> Result<usize> {
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..PROCESS_MEMORY_COUNTERS::default()
        };
        if unsafe {
            K32GetProcessMemoryInfo(
                handle,
                &mut counters,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        } == 0
        {
            return Err(last_error("K32GetProcessMemoryInfo"));
        }
        Ok(counters.WorkingSetSize)
    }

    fn query_process_state(handle: HANDLE) -> Result<(ProcessState, usize)> {
        let native_priority = unsafe { GetPriorityClass(handle) };
        if native_priority == 0 {
            return Err(last_error("GetPriorityClass"));
        }
        let priority_class = domain_priority(native_priority)?;

        let mut affinity_mask = 0usize;
        let mut system_affinity_mask = 0usize;
        if unsafe { GetProcessAffinityMask(handle, &mut affinity_mask, &mut system_affinity_mask) }
            == 0
        {
            return Err(last_error("GetProcessAffinityMask"));
        }
        if affinity_mask == 0 || system_affinity_mask == 0 {
            return Err(LctrlError::Unsupported {
                feature: "empty or multi-group process affinity mask".into(),
            });
        }

        let mut minimum_bytes = 0usize;
        let mut maximum_bytes = 0usize;
        let mut flags = 0u32;
        if unsafe {
            GetProcessWorkingSetSizeEx(handle, &mut minimum_bytes, &mut maximum_bytes, &mut flags)
        } == 0
        {
            return Err(last_error("GetProcessWorkingSetSizeEx"));
        }
        let working_set = ProcessWorkingSet::new(minimum_bytes, maximum_bytes, flags)?;
        Ok((
            ProcessState {
                priority_class,
                affinity_mask,
                working_set,
            },
            system_affinity_mask,
        ))
    }

    fn domain_priority(native: u32) -> Result<ProcessPriorityClass> {
        match native {
            IDLE_PRIORITY_CLASS => Ok(ProcessPriorityClass::Idle),
            BELOW_NORMAL_PRIORITY_CLASS => Ok(ProcessPriorityClass::BelowNormal),
            NORMAL_PRIORITY_CLASS => Ok(ProcessPriorityClass::Normal),
            ABOVE_NORMAL_PRIORITY_CLASS => Ok(ProcessPriorityClass::AboveNormal),
            HIGH_PRIORITY_CLASS => Ok(ProcessPriorityClass::High),
            REALTIME_PRIORITY_CLASS => Ok(ProcessPriorityClass::Realtime),
            _ => Err(LctrlError::Unsupported {
                feature: format!("Windows process priority class 0x{native:08x}"),
            }),
        }
    }

    fn native_priority_for_write(priority: ProcessPriorityClass) -> Result<u32> {
        match priority {
            ProcessPriorityClass::Idle => Ok(IDLE_PRIORITY_CLASS),
            ProcessPriorityClass::BelowNormal => Ok(BELOW_NORMAL_PRIORITY_CLASS),
            ProcessPriorityClass::Normal => Ok(NORMAL_PRIORITY_CLASS),
            ProcessPriorityClass::AboveNormal => Ok(ABOVE_NORMAL_PRIORITY_CLASS),
            ProcessPriorityClass::High => Ok(HIGH_PRIORITY_CLASS),
            ProcessPriorityClass::Realtime => Err(LctrlError::Unsupported {
                feature: "realtime process priority writes".into(),
            }),
        }
    }

    fn duration_100ns(duration: Duration) -> u64 {
        (duration.as_nanos() / 100).min(u128::from(u64::MAX)) as u64
    }

    fn last_error(context: &str) -> LctrlError {
        let error = unsafe { GetLastError() };
        map_win_error(error, context)
    }
}

#[cfg(windows)]
pub use native::NativeProcessApi;
