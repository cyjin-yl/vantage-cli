use std::{
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use lctrl_core::{
    DiagnosticCancellationReason, DiagnosticKind, DiagnosticMeasurement, DiagnosticOutcome,
    DiagnosticReason, DiagnosticResult, DiagnosticRunOptions, DiagnosticRunState,
    DiagnosticStatusSnapshot, DiagnosticThreshold, LctrlError, Result, UpdateCapability,
};
use lctrl_hal::{DiagnosticsControl, UpdateControl};

use crate::{WmiObject, WmiTransport, WmiValue};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";
const ROOT_WMI: &str = "ROOT\\WMI";
const CANCELLATION_POLL: Duration = Duration::from_millis(10);

/// Read-only Windows diagnostics built on public CIM/WMI classes. The
/// transport is shared with one bounded worker; no vendor diagnostics driver,
/// SMART command, service control, or memory stress operation is used.
pub struct WindowsSystemInventory<W> {
    transport: Arc<W>,
    executor: DiagnosticExecutor,
}

impl<W> WindowsSystemInventory<W> {
    #[must_use]
    pub fn new(transport: W) -> Self {
        Self {
            transport: Arc::new(transport),
            executor: DiagnosticExecutor::new(),
        }
    }

    #[must_use]
    pub fn transport(&self) -> &W {
        &self.transport
    }
}

impl<W> DiagnosticsControl for WindowsSystemInventory<W>
where
    W: WmiTransport + 'static,
{
    fn diagnostic_items(&self) -> Result<Vec<DiagnosticKind>> {
        Ok(vec![
            DiagnosticKind::Battery,
            DiagnosticKind::Thermal,
            DiagnosticKind::Storage,
            DiagnosticKind::Memory,
            DiagnosticKind::Firmware,
            DiagnosticKind::Network,
        ])
    }

    fn run_diagnostics(
        &self,
        items: &[DiagnosticKind],
        options: DiagnosticRunOptions,
    ) -> Result<Vec<DiagnosticResult>> {
        let transport: Arc<dyn WmiTransport> = self.transport.clone();
        self.executor.run(items, options, transport)
    }

    fn diagnostic_status(&self) -> DiagnosticStatusSnapshot {
        self.executor.status_snapshot()
    }
}

impl<W> UpdateControl for WindowsSystemInventory<W>
where
    W: WmiTransport,
{
    fn update_capability(&self) -> Result<UpdateCapability> {
        Ok(UpdateCapability::Unavailable {
            reason: "no authenticated public Lenovo update catalog/manifest contract is specified; private MCP packages and firmware flashing are excluded".into(),
        })
    }
}

struct DiagnosticJob {
    run_id: u64,
    items: Vec<DiagnosticKind>,
    options: DiagnosticRunOptions,
    transport: Arc<dyn WmiTransport>,
    response: SyncSender<DiagnosticWorkerMessage>,
    abandoned: Arc<AtomicBool>,
}

enum DiagnosticWorkerMessage {
    Started(DiagnosticKind),
    Item(DiagnosticResult),
}

/// One worker and a one-job queue bound the number of simultaneous WMI reads.
/// A synchronous WMI provider cannot be forcibly interrupted. Therefore a
/// deadline/cancellation abandons the caller-visible item immediately and the
/// worker exits that job at the next provider boundary. At most one subsequent
/// job can wait in the bounded queue; additional callers are rejected instead
/// of spawning more threads or issuing concurrent reads.
struct DiagnosticExecutor {
    sender: SyncSender<DiagnosticJob>,
    status: Arc<Mutex<DiagnosticStatusSnapshot>>,
    next_run_id: AtomicU64,
    run_lock: Mutex<()>,
    worker_start_error: Option<String>,
}

impl DiagnosticExecutor {
    fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let status = Arc::new(Mutex::new(DiagnosticStatusSnapshot::idle()));
        let worker_status = Arc::clone(&status);
        let worker_start_error = thread::Builder::new()
            .name("vantage-diagnostics".into())
            .spawn(move || diagnostic_worker(receiver, worker_status))
            .err()
            .map(|error| error.to_string());
        Self {
            sender,
            status,
            next_run_id: AtomicU64::new(0),
            run_lock: Mutex::new(()),
            worker_start_error,
        }
    }

    fn status_snapshot(&self) -> DiagnosticStatusSnapshot {
        lock(&self.status).clone()
    }

    fn run(
        &self,
        items: &[DiagnosticKind],
        options: DiagnosticRunOptions,
        transport: Arc<dyn WmiTransport>,
    ) -> Result<Vec<DiagnosticResult>> {
        if let Some(error) = &self.worker_start_error {
            return Ok(resource_results(
                items,
                DiagnosticReason::ExecutorUnavailable,
                format!("diagnostic worker could not start: {error}"),
            ));
        }
        if items.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(reason) = options.cancellation.reason() {
            let mut status = lock(&self.status);
            if !matches!(
                status.state,
                DiagnosticRunState::Running | DiagnosticRunState::Preparing
            ) {
                status.state = DiagnosticRunState::Cancelled;
                status.total_items = items.len();
                status.completed_items = 0;
                status.current_item = None;
                status.reason = Some(DiagnosticReason::Cancelled { reason });
            }
            return Ok(cancelled_results(items, 0, reason));
        }
        if options.per_item_deadline.is_zero() {
            let mut status = lock(&self.status);
            if !matches!(
                status.state,
                DiagnosticRunState::Running | DiagnosticRunState::Preparing
            ) {
                status.state = DiagnosticRunState::ResourceError;
                status.total_items = items.len();
                status.completed_items = 0;
                status.current_item = None;
                status.reason = Some(DiagnosticReason::DeadlineExceeded { deadline_ms: 0 });
            }
            return Ok(deadline_results(items, 0, options.per_item_deadline));
        }
        if Instant::now()
            .checked_add(options.per_item_deadline)
            .is_none()
        {
            return Ok(resource_results(
                items,
                DiagnosticReason::InvalidMeasurement {
                    name: "per_item_deadline".into(),
                },
                "per-item deadline cannot be represented by the monotonic clock".into(),
            ));
        }

        // Only one caller may own the status/run slot. Try-lock keeps a
        // second caller bounded even when a provider is blocked.
        let _run_guard = match self.run_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Ok(resource_results(
                    items,
                    DiagnosticReason::ExecutorBusy,
                    "another diagnostic run is still active".into(),
                ));
            }
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        };

        let run_id = self.next_run_id.fetch_add(1, Ordering::Relaxed) + 1;
        let previous_status = self.status_snapshot();
        *lock(&self.status) = DiagnosticStatusSnapshot {
            run_id,
            state: DiagnosticRunState::Preparing,
            current_item: None,
            completed_items: 0,
            total_items: items.len(),
            reason: None,
        };

        let (response, receiver) = mpsc::sync_channel(1);
        let abandoned = Arc::new(AtomicBool::new(false));
        let job = DiagnosticJob {
            run_id,
            items: items.to_vec(),
            options: options.clone(),
            transport,
            response,
            abandoned: Arc::clone(&abandoned),
        };
        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                *lock(&self.status) = previous_status;
                return Ok(resource_results(
                    items,
                    DiagnosticReason::ExecutorBusy,
                    "the bounded diagnostic worker is occupied".into(),
                ));
            }
            Err(TrySendError::Disconnected(_)) => {
                let reason = DiagnosticReason::ExecutorUnavailable;
                self.finish_status(
                    run_id,
                    DiagnosticRunState::ResourceError,
                    0,
                    Some(reason.clone()),
                );
                return Ok(resource_results(
                    items,
                    reason,
                    "the diagnostic worker is disconnected".into(),
                ));
            }
        }

        let mut results = Vec::with_capacity(items.len());
        for (index, kind) in items.iter().copied().enumerate() {
            let mut deadline = Instant::now().checked_add(options.per_item_deadline);
            loop {
                if let Some(reason) = options.cancellation.reason() {
                    abandoned.store(true, Ordering::Release);
                    self.finish_status(
                        run_id,
                        DiagnosticRunState::Cancelled,
                        results.len(),
                        Some(DiagnosticReason::Cancelled { reason }),
                    );
                    return Ok(cancelled_results_with_prefix(items, index, reason, results));
                }

                let wait = deadline
                    .map(|value| value.saturating_duration_since(Instant::now()))
                    .unwrap_or(CANCELLATION_POLL)
                    .min(CANCELLATION_POLL);
                if wait.is_zero() {
                    abandoned.store(true, Ordering::Release);
                    let reason = DiagnosticReason::DeadlineExceeded {
                        deadline_ms: duration_ms(options.per_item_deadline),
                    };
                    self.finish_status(
                        run_id,
                        DiagnosticRunState::ResourceError,
                        results.len(),
                        Some(reason.clone()),
                    );
                    return Ok(deadline_results_with_prefix(
                        items,
                        index,
                        options.per_item_deadline,
                        results,
                    ));
                }

                match receiver.recv_timeout(wait) {
                    Ok(DiagnosticWorkerMessage::Started(received_kind)) => {
                        if received_kind != kind {
                            abandoned.store(true, Ordering::Release);
                            let reason = DiagnosticReason::ExecutorUnavailable;
                            self.finish_status(
                                run_id,
                                DiagnosticRunState::ResourceError,
                                results.len(),
                                Some(reason.clone()),
                            );
                            return Ok(resource_results_with_prefix(
                                items,
                                index,
                                reason,
                                "diagnostic worker returned an out-of-order item".into(),
                                results,
                            ));
                        }
                        // Queueing time is bounded above by the initial
                        // deadline; the actual item gets its own full window.
                        deadline = Instant::now().checked_add(options.per_item_deadline);
                    }
                    Ok(DiagnosticWorkerMessage::Item(result)) => {
                        if result.kind != kind {
                            abandoned.store(true, Ordering::Release);
                            let reason = DiagnosticReason::ExecutorUnavailable;
                            self.finish_status(
                                run_id,
                                DiagnosticRunState::ResourceError,
                                results.len(),
                                Some(reason.clone()),
                            );
                            return Ok(resource_results_with_prefix(
                                items,
                                index,
                                reason,
                                "diagnostic worker returned the wrong item".into(),
                                results,
                            ));
                        }
                        if let Some(reason) = options.cancellation.reason() {
                            abandoned.store(true, Ordering::Release);
                            self.finish_status(
                                run_id,
                                DiagnosticRunState::Cancelled,
                                results.len(),
                                Some(DiagnosticReason::Cancelled { reason }),
                            );
                            return Ok(cancelled_results_with_prefix(
                                items, index, reason, results,
                            ));
                        }
                        results.push(result);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // The loop checks cancellation and the deadline on
                        // every iteration, including while WMI is blocked.
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        abandoned.store(true, Ordering::Release);
                        let reason = DiagnosticReason::ExecutorUnavailable;
                        self.finish_status(
                            run_id,
                            DiagnosticRunState::ResourceError,
                            results.len(),
                            Some(reason.clone()),
                        );
                        return Ok(resource_results_with_prefix(
                            items,
                            index,
                            reason,
                            "diagnostic worker disconnected during a read".into(),
                            results,
                        ));
                    }
                }
            }
        }

        self.finish_status(run_id, DiagnosticRunState::Completed, results.len(), None);
        Ok(results)
    }

    fn finish_status(
        &self,
        run_id: u64,
        state: DiagnosticRunState,
        completed_items: usize,
        reason: Option<DiagnosticReason>,
    ) {
        let mut status = lock(&self.status);
        if status.run_id == run_id {
            status.state = state;
            status.current_item = None;
            status.completed_items = completed_items;
            status.reason = reason;
        }
    }
}

fn diagnostic_worker(
    receiver: Receiver<DiagnosticJob>,
    status: Arc<Mutex<DiagnosticStatusSnapshot>>,
) {
    while let Ok(job) = receiver.recv() {
        let mut abandoned_job = false;
        for (index, kind) in job.items.iter().copied().enumerate() {
            if job.abandoned.load(Ordering::Acquire) {
                abandoned_job = true;
                break;
            }
            if let Some(reason) = job.options.cancellation.reason() {
                let result = cancelled_result(kind, reason);
                update_worker_status(&status, job.run_id, |snapshot| {
                    snapshot.state = DiagnosticRunState::Cancelled;
                    snapshot.current_item = None;
                    snapshot.completed_items = index;
                    snapshot.reason = Some(DiagnosticReason::Cancelled { reason });
                });
                let _ = job.response.send(DiagnosticWorkerMessage::Item(result));
                abandoned_job = true;
                break;
            }

            update_worker_status(&status, job.run_id, |snapshot| {
                snapshot.state = DiagnosticRunState::Running;
                snapshot.current_item = Some(kind);
                snapshot.completed_items = index;
            });
            if job
                .response
                .send(DiagnosticWorkerMessage::Started(kind))
                .is_err()
            {
                abandoned_job = true;
                break;
            }

            let result = evaluate_diagnostic(job.transport.as_ref(), kind);
            if job.abandoned.load(Ordering::Acquire) {
                abandoned_job = true;
                break;
            }
            if let Some(reason) = job.options.cancellation.reason() {
                let _ = job
                    .response
                    .send(DiagnosticWorkerMessage::Item(cancelled_result(
                        kind, reason,
                    )));
                abandoned_job = true;
                break;
            }
            if job
                .response
                .send(DiagnosticWorkerMessage::Item(result))
                .is_err()
            {
                abandoned_job = true;
                break;
            }
            update_worker_status(&status, job.run_id, |snapshot| {
                snapshot.current_item = None;
                snapshot.completed_items = index + 1;
            });
        }

        if !abandoned_job && !job.abandoned.load(Ordering::Acquire) {
            update_worker_status(&status, job.run_id, |snapshot| {
                snapshot.state = DiagnosticRunState::Completed;
                snapshot.current_item = None;
                snapshot.completed_items = job.items.len();
                snapshot.reason = None;
            });
        }
    }
}

fn update_worker_status<F>(status: &Arc<Mutex<DiagnosticStatusSnapshot>>, run_id: u64, update: F)
where
    F: FnOnce(&mut DiagnosticStatusSnapshot),
{
    let mut snapshot = lock(status);
    if snapshot.run_id == run_id {
        update(&mut snapshot);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Copy)]
struct DiagnosticQuery {
    namespace: &'static str,
    wql: &'static str,
    label: &'static str,
    class: &'static str,
}

impl DiagnosticQuery {
    fn source(self) -> String {
        format!("WMI {} {}", self.namespace, self.class)
    }
}

fn diagnostic_query(kind: DiagnosticKind) -> DiagnosticQuery {
    match kind {
        DiagnosticKind::Battery => DiagnosticQuery {
            namespace: ROOT_CIMV2,
            wql: "SELECT Status, BatteryStatus, EstimatedChargeRemaining FROM Win32_Battery",
            label: "battery",
            class: "Win32_Battery",
        },
        DiagnosticKind::Thermal => DiagnosticQuery {
            namespace: ROOT_WMI,
            wql: "SELECT CurrentTemperature, CriticalTripPoint FROM MSAcpi_ThermalZoneTemperature",
            label: "thermal-zone",
            class: "MSAcpi_ThermalZoneTemperature",
        },
        DiagnosticKind::Storage => DiagnosticQuery {
            namespace: ROOT_CIMV2,
            wql: "SELECT Model, Status, Size FROM Win32_DiskDrive",
            label: "storage-device",
            class: "Win32_DiskDrive",
        },
        DiagnosticKind::Memory => DiagnosticQuery {
            namespace: ROOT_CIMV2,
            wql: "SELECT Capacity, Status FROM Win32_PhysicalMemory",
            label: "memory-device",
            class: "Win32_PhysicalMemory",
        },
        DiagnosticKind::Firmware => DiagnosticQuery {
            namespace: ROOT_CIMV2,
            wql: "SELECT SMBIOSBIOSVersion, Status FROM Win32_BIOS",
            label: "firmware",
            class: "Win32_BIOS",
        },
        DiagnosticKind::Network => DiagnosticQuery {
            namespace: ROOT_CIMV2,
            wql: "SELECT Name, Status, NetEnabled FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE",
            label: "network-adapter",
            class: "Win32_NetworkAdapter",
        },
    }
}

fn evaluate_diagnostic(transport: &dyn WmiTransport, kind: DiagnosticKind) -> DiagnosticResult {
    let query = diagnostic_query(kind);
    match transport.query(query.namespace, query.wql) {
        Ok(objects) if objects.is_empty() => DiagnosticResult::new(
            kind,
            DiagnosticOutcome::Unavailable,
            format!(
                "no {} records were returned; inventory presence is not health",
                query.label
            ),
        )
        .with_source(query.source())
        .with_reason(DiagnosticReason::NoRecords),
        Ok(objects) => match kind {
            DiagnosticKind::Battery => evaluate_battery(&objects, query),
            DiagnosticKind::Thermal => evaluate_thermal(&objects, query),
            DiagnosticKind::Storage => evaluate_storage(&objects, query),
            DiagnosticKind::Memory => evaluate_memory(&objects, query),
            DiagnosticKind::Firmware => evaluate_firmware(&objects, query),
            DiagnosticKind::Network => evaluate_network(&objects, query),
        },
        Err(error) => provider_error_result(kind, query, error),
    }
}

fn provider_error_result(
    kind: DiagnosticKind,
    query: DiagnosticQuery,
    error: LctrlError,
) -> DiagnosticResult {
    let outcome = match &error {
        LctrlError::Unsupported { .. } | LctrlError::ChannelUnavailable { .. } => {
            DiagnosticOutcome::Unavailable
        }
        LctrlError::PermissionDenied { .. }
        | LctrlError::FirmwareRejected { .. }
        | LctrlError::InvalidArgument { .. }
        | LctrlError::VerifyMismatch { .. }
        | LctrlError::Io(_) => DiagnosticOutcome::ResourceError,
    };
    let message = error.to_string();
    DiagnosticResult::new(
        kind,
        outcome,
        format!("{} read failed: {message}", query.label),
    )
    .with_source(query.source())
    .with_reason(DiagnosticReason::ProviderError { message })
}

#[derive(Default)]
struct ItemEvaluation {
    outcome: Option<DiagnosticOutcome>,
    reason: Option<DiagnosticReason>,
    measurements: Vec<DiagnosticMeasurement>,
    threshold: Option<DiagnosticThreshold>,
    saw_reading: bool,
}

impl ItemEvaluation {
    fn merge(&mut self, outcome: DiagnosticOutcome, reason: Option<DiagnosticReason>) {
        let replace = self
            .outcome
            .is_none_or(|current| outcome_rank(outcome) > outcome_rank(current));
        if replace {
            self.outcome = Some(outcome);
            self.reason = reason;
        } else if self.reason.is_none() {
            self.reason = reason;
        }
    }

    fn reading(&mut self, measurement: DiagnosticMeasurement) {
        self.saw_reading = true;
        self.measurements.push(measurement);
    }

    fn finish(
        self,
        kind: DiagnosticKind,
        query: DiagnosticQuery,
        detail: String,
    ) -> DiagnosticResult {
        let mut result = DiagnosticResult::new(
            kind,
            self.outcome.unwrap_or(DiagnosticOutcome::Unavailable),
            detail,
        )
        .with_source(query.source())
        .with_measurements(self.measurements);
        if let Some(threshold) = self.threshold {
            result = result.with_threshold(threshold);
        }
        if let Some(reason) = self.reason {
            result = result.with_reason(reason);
        }
        result
    }
}

fn outcome_rank(outcome: DiagnosticOutcome) -> u8 {
    match outcome {
        DiagnosticOutcome::Passed => 0,
        DiagnosticOutcome::Skipped => 1,
        DiagnosticOutcome::Warning => 2,
        DiagnosticOutcome::Unavailable => 3,
        DiagnosticOutcome::ResourceError => 4,
        DiagnosticOutcome::Failed => 5,
        DiagnosticOutcome::Cancelled => 6,
    }
}

fn measurement_name(name: &str, index: usize, count: usize) -> String {
    if count == 1 {
        name.to_owned()
    } else {
        format!("{name}[{index}]")
    }
}

enum ReadValue<T> {
    Missing,
    Value(T),
    Invalid,
}

fn read_string(object: &WmiObject, name: &str) -> ReadValue<String> {
    match object.get(name) {
        None | Some(WmiValue::Empty | WmiValue::Null) => ReadValue::Missing,
        Some(WmiValue::String(value)) if !value.trim().is_empty() => {
            ReadValue::Value(value.clone())
        }
        Some(WmiValue::String(_)) => ReadValue::Missing,
        Some(_) => ReadValue::Invalid,
    }
}

fn read_bool(object: &WmiObject, name: &str) -> ReadValue<bool> {
    match object.get(name) {
        None | Some(WmiValue::Empty | WmiValue::Null) => ReadValue::Missing,
        Some(WmiValue::Bool(value)) => ReadValue::Value(*value),
        Some(_) => ReadValue::Invalid,
    }
}

fn read_u64(object: &WmiObject, name: &str) -> ReadValue<u64> {
    let value = match object.get(name) {
        None | Some(WmiValue::Empty | WmiValue::Null) => return ReadValue::Missing,
        Some(WmiValue::U8(value)) => u64::from(*value),
        Some(WmiValue::U16(value)) => u64::from(*value),
        Some(WmiValue::U32(value)) => u64::from(*value),
        Some(WmiValue::U64(value)) => *value,
        Some(WmiValue::I8(value)) if *value >= 0 => *value as u64,
        Some(WmiValue::I16(value)) if *value >= 0 => *value as u64,
        Some(WmiValue::I32(value)) if *value >= 0 => *value as u64,
        Some(WmiValue::I64(value)) if *value >= 0 => *value as u64,
        Some(_) => return ReadValue::Invalid,
    };
    ReadValue::Value(value)
}

struct StatusEvaluation {
    value: ReadValue<DiagnosticOutcome>,
    measurement: Option<DiagnosticMeasurement>,
    reason: Option<DiagnosticReason>,
}

fn evaluate_status(object: &WmiObject, index: usize, count: usize) -> StatusEvaluation {
    match read_string(object, "Status") {
        ReadValue::Missing => StatusEvaluation {
            value: ReadValue::Missing,
            measurement: None,
            reason: None,
        },
        ReadValue::Invalid => StatusEvaluation {
            value: ReadValue::Invalid,
            measurement: None,
            reason: Some(DiagnosticReason::InvalidMeasurement {
                name: "Status".into(),
            }),
        },
        ReadValue::Value(status) => {
            let outcome = status_outcome(&status);
            let reason = match outcome {
                DiagnosticOutcome::ResourceError => Some(DiagnosticReason::ProviderError {
                    message: format!("device reported Status={status}"),
                }),
                _ => None,
            };
            StatusEvaluation {
                value: ReadValue::Value(outcome),
                measurement: Some(DiagnosticMeasurement::new(
                    measurement_name("Status", index, count),
                    status,
                )),
                reason,
            }
        }
    }
}

fn status_outcome(status: &str) -> DiagnosticOutcome {
    // CIM_ManagedSystemElement.Status, inherited by the Win32 classes:
    // https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/cim-managedsystemelement
    let status = status.trim();
    if status.eq_ignore_ascii_case("OK") {
        DiagnosticOutcome::Passed
    } else if [
        "Error",
        "Non-Recoverable",
        "Critical",
        "Predictive Failure",
        "Pred Fail",
        "Install Error",
        "Supporting Entity in Error",
    ]
    .iter()
    .any(|value| status.eq_ignore_ascii_case(value))
    {
        DiagnosticOutcome::Failed
    } else if ["Lost Communication", "No Contact"]
        .iter()
        .any(|value| status.eq_ignore_ascii_case(value))
    {
        DiagnosticOutcome::ResourceError
    } else if status.eq_ignore_ascii_case("Not Available") {
        DiagnosticOutcome::Unavailable
    } else {
        // Unknown and transitional CIM states are not proof of health.
        DiagnosticOutcome::Warning
    }
}

fn merge_status(
    evaluation: &mut ItemEvaluation,
    object: &WmiObject,
    index: usize,
    count: usize,
) -> ReadValue<()> {
    let status = evaluate_status(object, index, count);
    if let Some(measurement) = status.measurement {
        evaluation.reading(measurement);
    }
    match status.value {
        ReadValue::Missing => ReadValue::Missing,
        ReadValue::Invalid => {
            evaluation.merge(DiagnosticOutcome::ResourceError, status.reason);
            ReadValue::Invalid
        }
        ReadValue::Value(outcome) => {
            evaluation.merge(outcome, status.reason);
            ReadValue::Value(())
        }
    }
}

fn evaluate_battery(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    // BatteryStatus enum and the 0..100 percentage domain are public Win32_Battery semantics:
    // https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-battery
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut battery_status_seen = false;
    let mut charge_seen = false;
    for (index, object) in objects.iter().enumerate() {
        // Status is useful when exposed, but BatteryStatus is the specific
        // battery state; a missing generic Status does not invalidate it.
        let _ = merge_status(&mut evaluation, object, index, count);
        match read_u64(object, "BatteryStatus") {
            ReadValue::Missing => evaluation.merge(
                DiagnosticOutcome::Warning,
                Some(DiagnosticReason::MissingMeasurement {
                    name: "BatteryStatus".into(),
                }),
            ),
            ReadValue::Invalid => evaluation.merge(
                DiagnosticOutcome::ResourceError,
                Some(DiagnosticReason::InvalidMeasurement {
                    name: "BatteryStatus".into(),
                }),
            ),
            ReadValue::Value(value) => {
                battery_status_seen = true;
                evaluation.reading(DiagnosticMeasurement::new(
                    measurement_name("BatteryStatus", index, count),
                    value,
                ));
                let outcome = match value {
                    1 | 2 | 3 | 6 | 7 | 11 => DiagnosticOutcome::Passed,
                    4 | 8 => DiagnosticOutcome::Warning,
                    5 | 9 => DiagnosticOutcome::Failed,
                    _ => DiagnosticOutcome::Warning,
                };
                evaluation.merge(
                    outcome,
                    (value > 11).then(|| DiagnosticReason::InvalidMeasurement {
                        name: "BatteryStatus".into(),
                    }),
                );
            }
        }
        match read_u64(object, "EstimatedChargeRemaining") {
            ReadValue::Missing => evaluation.merge(
                DiagnosticOutcome::Warning,
                Some(DiagnosticReason::MissingMeasurement {
                    name: "EstimatedChargeRemaining".into(),
                }),
            ),
            ReadValue::Invalid => evaluation.merge(
                DiagnosticOutcome::ResourceError,
                Some(DiagnosticReason::InvalidMeasurement {
                    name: "EstimatedChargeRemaining".into(),
                }),
            ),
            ReadValue::Value(value) if value <= 100 => {
                charge_seen = true;
                evaluation.reading(
                    DiagnosticMeasurement::new(
                        measurement_name("EstimatedChargeRemaining", index, count),
                        value,
                    )
                    .with_unit("percent"),
                );
                // No charge-percentage health threshold is inferred.
            }
            ReadValue::Value(_) => evaluation.merge(
                DiagnosticOutcome::ResourceError,
                Some(DiagnosticReason::InvalidMeasurement {
                    name: "EstimatedChargeRemaining".into(),
                }),
            ),
        }
    }
    if !battery_status_seen || !charge_seen || !evaluation.saw_reading {
        evaluation.merge(
            if evaluation.saw_reading {
                DiagnosticOutcome::Warning
            } else {
                DiagnosticOutcome::Unavailable
            },
            Some(DiagnosticReason::MissingMeasurement {
                name: "BatteryStatus/EstimatedChargeRemaining".into(),
            }),
        );
    }
    evaluation.finish(
        DiagnosticKind::Battery,
        query,
        "Win32_Battery Status, BatteryStatus and EstimatedChargeRemaining evaluated; charge percentage has no invented health threshold; proprietary deep battery tests are unavailable".into(),
    )
}

fn evaluate_thermal(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut current_seen = false;
    let mut threshold_seen = false;
    for (index, object) in objects.iter().enumerate() {
        let current = read_u64(object, "CurrentTemperature");
        let critical = read_u64(object, "CriticalTripPoint");
        let current_c = match current {
            ReadValue::Missing => {
                evaluation.merge(
                    DiagnosticOutcome::Warning,
                    Some(DiagnosticReason::MissingMeasurement {
                        name: "CurrentTemperature".into(),
                    }),
                );
                None
            }
            ReadValue::Invalid => {
                evaluation.merge(
                    DiagnosticOutcome::ResourceError,
                    Some(DiagnosticReason::InvalidMeasurement {
                        name: "CurrentTemperature".into(),
                    }),
                );
                None
            }
            ReadValue::Value(raw) => {
                current_seen = true;
                let value = decikelvin_to_celsius(raw);
                evaluation.reading(
                    DiagnosticMeasurement::new(
                        measurement_name("CurrentTemperature", index, count),
                        value,
                    )
                    .with_unit("degC"),
                );
                Some(value)
            }
        };
        let critical_c = match critical {
            ReadValue::Missing => {
                evaluation.merge(
                    DiagnosticOutcome::Warning,
                    Some(DiagnosticReason::NoNormativeThreshold),
                );
                None
            }
            ReadValue::Invalid => {
                evaluation.merge(
                    DiagnosticOutcome::ResourceError,
                    Some(DiagnosticReason::InvalidMeasurement {
                        name: "CriticalTripPoint".into(),
                    }),
                );
                None
            }
            ReadValue::Value(raw) => {
                threshold_seen = true;
                let value = decikelvin_to_celsius(raw);
                if evaluation.threshold.is_none()
                    || current_c.is_some_and(|current| current >= value)
                {
                    evaluation.threshold = Some(
                        DiagnosticThreshold::at_or_above(
                            measurement_name("CriticalTripPoint", index, count),
                            value,
                        )
                        .with_unit("degC"),
                    );
                }
                Some(value)
            }
        };
        if let (Some(current), Some(critical)) = (current_c, critical_c) {
            if current >= critical {
                evaluation.merge(
                    DiagnosticOutcome::Failed,
                    Some(DiagnosticReason::ThresholdExceeded),
                );
            } else {
                evaluation.merge(DiagnosticOutcome::Passed, None);
            }
        }
    }
    if !current_seen {
        evaluation.merge(
            if evaluation.saw_reading {
                DiagnosticOutcome::Warning
            } else {
                DiagnosticOutcome::Unavailable
            },
            Some(DiagnosticReason::MissingMeasurement {
                name: "CurrentTemperature".into(),
            }),
        );
    } else if !threshold_seen {
        evaluation.merge(
            DiagnosticOutcome::Warning,
            Some(DiagnosticReason::NoNormativeThreshold),
        );
    }
    evaluation.finish(
        DiagnosticKind::Thermal,
        query,
        if threshold_seen {
            "MSAcpi_ThermalZoneTemperature CurrentTemperature compared with the source CriticalTripPoint (tenths Kelvin converted to degC); no thermal stress test was run".into()
        } else {
            "MSAcpi_ThermalZoneTemperature CurrentTemperature sensor_readout only; CriticalTripPoint is unavailable, so no pass/fail threshold was invented".into()
        },
    )
}

fn decikelvin_to_celsius(raw: u64) -> f64 {
    // MSAcpi_ThermalZoneTemperature exposes ACPI temperatures in tenths Kelvin.
    // CriticalTripPoint is a source-provided critical limit, not a tool policy.
    raw as f64 / 10.0 - 273.15
}

fn evaluate_storage(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut status_seen = false;
    for (index, object) in objects.iter().enumerate() {
        if let ReadValue::Value(model) = read_string(object, "Model") {
            evaluation.reading(DiagnosticMeasurement::new(
                measurement_name("Model", index, count),
                model,
            ));
        }
        if let ReadValue::Value(size) = read_u64(object, "Size") {
            evaluation.reading(
                DiagnosticMeasurement::new(measurement_name("Size", index, count), size)
                    .with_unit("bytes"),
            );
        }
        if matches!(
            merge_status(&mut evaluation, object, index, count),
            ReadValue::Value(())
        ) {
            status_seen = true;
        }
    }
    if !status_seen {
        evaluation.merge(
            if evaluation.saw_reading {
                DiagnosticOutcome::Warning
            } else {
                DiagnosticOutcome::Unavailable
            },
            Some(DiagnosticReason::MissingMeasurement {
                name: "Status".into(),
            }),
        );
    } else if evaluation.outcome == Some(DiagnosticOutcome::Passed) {
        // A good shallow Status is not a SMART pass. Keep the overall item
        // warning while retaining the real Status=OK measurement.
        evaluation.merge(
            DiagnosticOutcome::Warning,
            Some(DiagnosticReason::DeepTestUnavailable {
                test: "SMART".into(),
            }),
        );
    }
    evaluation.finish(
        DiagnosticKind::Storage,
        query,
        "Win32_DiskDrive Status evaluated; SMART diagnostics are unavailable and were not claimed or run; inventory presence alone is not health".into(),
    )
}

fn evaluate_memory(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut status_seen = false;
    for (index, object) in objects.iter().enumerate() {
        if let ReadValue::Value(capacity) = read_u64(object, "Capacity") {
            evaluation.reading(
                DiagnosticMeasurement::new(measurement_name("Capacity", index, count), capacity)
                    .with_unit("bytes"),
            );
        }
        if matches!(
            merge_status(&mut evaluation, object, index, count),
            ReadValue::Value(())
        ) {
            status_seen = true;
        }
    }
    if !status_seen {
        evaluation.merge(
            if evaluation.saw_reading {
                DiagnosticOutcome::Warning
            } else {
                DiagnosticOutcome::Unavailable
            },
            Some(DiagnosticReason::MissingMeasurement {
                name: "Status".into(),
            }),
        );
    } else if evaluation.outcome == Some(DiagnosticOutcome::Passed) {
        evaluation.merge(
            DiagnosticOutcome::Warning,
            Some(DiagnosticReason::DeepTestUnavailable {
                test: "destructive memory stress".into(),
            }),
        );
    }
    evaluation.finish(
        DiagnosticKind::Memory,
        query,
        "Win32_PhysicalMemory Status evaluated; destructive memory stress is unavailable and was not run; capacity/inventory alone is not health".into(),
    )
}

fn evaluate_firmware(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut status_seen = false;
    for (index, object) in objects.iter().enumerate() {
        if let ReadValue::Value(version) = read_string(object, "SMBIOSBIOSVersion") {
            evaluation.reading(DiagnosticMeasurement::new(
                measurement_name("SMBIOSBIOSVersion", index, count),
                version,
            ));
        }
        match merge_status(&mut evaluation, object, index, count) {
            ReadValue::Value(()) => status_seen = true,
            ReadValue::Missing => evaluation.merge(
                DiagnosticOutcome::Warning,
                Some(DiagnosticReason::MissingMeasurement {
                    name: "Status".into(),
                }),
            ),
            ReadValue::Invalid => {}
        }
    }
    if !status_seen {
        evaluation.merge(
            if evaluation.saw_reading {
                DiagnosticOutcome::Warning
            } else {
                DiagnosticOutcome::Unavailable
            },
            Some(DiagnosticReason::MissingMeasurement {
                name: "Status".into(),
            }),
        );
    }
    evaluation.finish(
        DiagnosticKind::Firmware,
        query,
        "Win32_BIOS Status and SMBIOSBIOSVersion read; firmware identity alone is not health and firmware flashing/deep proprietary diagnostics were not run".into(),
    )
}

fn evaluate_network(objects: &[WmiObject], query: DiagnosticQuery) -> DiagnosticResult {
    let mut evaluation = ItemEvaluation::default();
    let count = objects.len();
    let mut enabled_seen = false;
    let mut disabled_seen = false;
    let mut enabled_status_missing = false;
    for (index, object) in objects.iter().enumerate() {
        if let ReadValue::Value(name) = read_string(object, "Name") {
            evaluation.reading(DiagnosticMeasurement::new(
                measurement_name("Name", index, count),
                name,
            ));
        }
        let status = evaluate_status(object, index, count);
        if let Some(measurement) = status.measurement {
            evaluation.reading(measurement);
        }
        let status_value = match status.value {
            ReadValue::Missing => None,
            ReadValue::Invalid => {
                evaluation.merge(DiagnosticOutcome::ResourceError, status.reason);
                None
            }
            ReadValue::Value(outcome) => {
                evaluation.merge(outcome, status.reason);
                Some(outcome)
            }
        };
        match read_bool(object, "NetEnabled") {
            ReadValue::Missing => {
                enabled_status_missing = true;
                evaluation.merge(
                    DiagnosticOutcome::Warning,
                    Some(DiagnosticReason::MissingMeasurement {
                        name: "NetEnabled".into(),
                    }),
                );
            }
            ReadValue::Invalid => {
                evaluation.merge(
                    DiagnosticOutcome::ResourceError,
                    Some(DiagnosticReason::InvalidMeasurement {
                        name: "NetEnabled".into(),
                    }),
                );
            }
            ReadValue::Value(enabled) => {
                evaluation.reading(DiagnosticMeasurement::new(
                    measurement_name("NetEnabled", index, count),
                    enabled,
                ));
                if enabled {
                    enabled_seen = true;
                    if status_value.is_none() {
                        enabled_status_missing = true;
                        evaluation.merge(
                            DiagnosticOutcome::Warning,
                            Some(DiagnosticReason::MissingMeasurement {
                                name: "Status".into(),
                            }),
                        );
                    }
                } else {
                    disabled_seen = true;
                    if matches!(
                        status_value,
                        Some(DiagnosticOutcome::Failed | DiagnosticOutcome::ResourceError)
                    ) {
                        // A reported failure is more important than the
                        // disabled/skipped classification.
                    } else {
                        evaluation
                            .merge(DiagnosticOutcome::Skipped, Some(DiagnosticReason::Disabled));
                    }
                }
            }
        }
    }
    if enabled_seen && evaluation.outcome == Some(DiagnosticOutcome::Skipped) {
        evaluation.outcome = Some(DiagnosticOutcome::Passed);
        evaluation.reason = None;
    }
    if !enabled_seen && disabled_seen && evaluation.outcome == Some(DiagnosticOutcome::Skipped) {
        // All physical adapters were explicitly disabled. This is not a
        // failed connectivity test and no attempt is made to enable them.
    } else if enabled_seen && enabled_status_missing {
        evaluation.merge(
            DiagnosticOutcome::Warning,
            Some(DiagnosticReason::MissingMeasurement {
                name: "Status".into(),
            }),
        );
    } else if !enabled_seen && !disabled_seen && !evaluation.saw_reading {
        evaluation.merge(
            DiagnosticOutcome::Unavailable,
            Some(DiagnosticReason::MissingMeasurement {
                name: "NetEnabled".into(),
            }),
        );
    }
    evaluation.finish(
        DiagnosticKind::Network,
        query,
        "Win32_NetworkAdapter NetEnabled and Status evaluated; disabled adapters are skipped, enabled state is not a connectivity guarantee, and no device/service changes were made".into(),
    )
}

fn cancelled_result(
    kind: DiagnosticKind,
    reason: DiagnosticCancellationReason,
) -> DiagnosticResult {
    DiagnosticResult::new(
        kind,
        DiagnosticOutcome::Cancelled,
        format!("diagnostic cancelled before completion ({reason:?})"),
    )
    .with_reason(DiagnosticReason::Cancelled { reason })
}

fn cancelled_results(
    items: &[DiagnosticKind],
    start: usize,
    reason: DiagnosticCancellationReason,
) -> Vec<DiagnosticResult> {
    items[start..]
        .iter()
        .copied()
        .map(|kind| cancelled_result(kind, reason))
        .collect()
}

fn cancelled_results_with_prefix(
    items: &[DiagnosticKind],
    index: usize,
    reason: DiagnosticCancellationReason,
    mut prefix: Vec<DiagnosticResult>,
) -> Vec<DiagnosticResult> {
    prefix.extend(cancelled_results(items, index, reason));
    prefix
}

fn deadline_result(kind: DiagnosticKind, deadline: Duration) -> DiagnosticResult {
    DiagnosticResult::new(
        kind,
        DiagnosticOutcome::ResourceError,
        format!(
            "diagnostic exceeded the per-item deadline of {} ms; the read was abandoned without a device/service mutation",
            duration_ms(deadline)
        ),
    )
    .with_source(diagnostic_query(kind).source())
    .with_reason(DiagnosticReason::DeadlineExceeded {
        deadline_ms: duration_ms(deadline),
    })
}

fn deadline_results(
    items: &[DiagnosticKind],
    start: usize,
    deadline: Duration,
) -> Vec<DiagnosticResult> {
    items[start..]
        .iter()
        .copied()
        .map(|kind| deadline_result(kind, deadline))
        .collect()
}

fn deadline_results_with_prefix(
    items: &[DiagnosticKind],
    index: usize,
    deadline: Duration,
    mut prefix: Vec<DiagnosticResult>,
) -> Vec<DiagnosticResult> {
    prefix.push(deadline_result(items[index], deadline));
    prefix.extend(items[index + 1..].iter().copied().map(|kind| {
        DiagnosticResult::new(
            kind,
            DiagnosticOutcome::Skipped,
            "not run because an earlier diagnostic item exceeded its deadline; serial execution was preserved",
        )
        .with_reason(DiagnosticReason::DeadlineExceeded {
            deadline_ms: duration_ms(deadline),
        })
    }));
    prefix
}

fn resource_results(
    items: &[DiagnosticKind],
    reason: DiagnosticReason,
    detail: String,
) -> Vec<DiagnosticResult> {
    items
        .iter()
        .copied()
        .map(|kind| {
            DiagnosticResult::new(kind, DiagnosticOutcome::ResourceError, detail.clone())
                .with_reason(reason.clone())
        })
        .collect()
}

fn resource_results_with_prefix(
    items: &[DiagnosticKind],
    index: usize,
    reason: DiagnosticReason,
    detail: String,
    mut prefix: Vec<DiagnosticResult>,
) -> Vec<DiagnosticResult> {
    prefix.extend(items[index..].iter().copied().map(|kind| {
        DiagnosticResult::new(kind, DiagnosticOutcome::ResourceError, detail.clone())
            .with_reason(reason.clone())
            .with_source(diagnostic_query(kind).source())
    }));
    prefix
}
