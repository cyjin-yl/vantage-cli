use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    Battery,
    Thermal,
    Storage,
    Memory,
    Firmware,
    Network,
}

/// The result of the checks that were actually performed.
///
/// `Passed` never means that an unimplemented vendor/deep test passed.  A
/// shallow readout without a normative health decision is `Warning` instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOutcome {
    Passed,
    Failed,
    Skipped,
    Warning,
    Unavailable,
    Cancelled,
    ResourceError,
}

/// A measured value retains its type instead of being flattened into detail
/// text.  Floating point values are used only for finite unit conversions.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum DiagnosticValue {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Boolean(bool),
    Text(String),
}

impl From<bool> for DiagnosticValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for DiagnosticValue {
    fn from(value: i64) -> Self {
        Self::Signed(value)
    }
}

impl From<u64> for DiagnosticValue {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}

impl From<u32> for DiagnosticValue {
    fn from(value: u32) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

impl From<u16> for DiagnosticValue {
    fn from(value: u16) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

impl From<u8> for DiagnosticValue {
    fn from(value: u8) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

impl From<f64> for DiagnosticValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for DiagnosticValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for DiagnosticValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiagnosticMeasurement {
    pub name: String,
    pub value: DiagnosticValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl DiagnosticMeasurement {
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<DiagnosticValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            unit: None,
        }
    }

    #[must_use]
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticThresholdComparison {
    AtOrAbove,
}

/// A threshold is present only when the source defines the comparison.  In
/// particular, charge percentage and temperature readouts do not acquire
/// invented "healthy" ranges.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiagnosticThreshold {
    pub name: String,
    pub value: DiagnosticValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub comparison: DiagnosticThresholdComparison,
}

impl DiagnosticThreshold {
    #[must_use]
    pub fn at_or_above(name: impl Into<String>, value: impl Into<DiagnosticValue>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            unit: None,
            comparison: DiagnosticThresholdComparison::AtOrAbove,
        }
    }

    #[must_use]
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCancellationReason {
    Requested,
    DeadlineExceeded,
    ExecutorStopped,
}

/// Structured machine-readable context for an outcome.  The human-readable
/// `DiagnosticResult::detail` remains suitable for CLI text output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticReason {
    Cancelled {
        reason: DiagnosticCancellationReason,
    },
    DeadlineExceeded {
        deadline_ms: u64,
    },
    ProviderError {
        message: String,
    },
    NoRecords,
    MissingMeasurement {
        name: String,
    },
    InvalidMeasurement {
        name: String,
    },
    NoNormativeThreshold,
    DeepTestUnavailable {
        test: String,
    },
    Disabled,
    ThresholdExceeded,
    ExecutorBusy,
    ExecutorUnavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiagnosticResult {
    pub kind: DiagnosticKind,
    pub outcome: DiagnosticOutcome,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measurements: Vec<DiagnosticMeasurement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<DiagnosticThreshold>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DiagnosticReason>,
}

impl DiagnosticResult {
    #[must_use]
    pub fn new(
        kind: DiagnosticKind,
        outcome: DiagnosticOutcome,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            outcome,
            detail: detail.into(),
            measurements: Vec::new(),
            source: None,
            threshold: None,
            reason: None,
        }
    }

    #[must_use]
    pub fn with_measurement(mut self, measurement: DiagnosticMeasurement) -> Self {
        self.measurements.push(measurement);
        self
    }

    #[must_use]
    pub fn with_measurements(mut self, measurements: Vec<DiagnosticMeasurement>) -> Self {
        self.measurements = measurements;
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    #[must_use]
    pub fn with_threshold(mut self, threshold: DiagnosticThreshold) -> Self {
        self.threshold = Some(threshold);
        self
    }

    #[must_use]
    pub fn with_reason(mut self, reason: DiagnosticReason) -> Self {
        self.reason = Some(reason);
        self
    }

    #[must_use]
    pub fn measurement(&self, name: &str) -> Option<&DiagnosticMeasurement> {
        self.measurements
            .iter()
            .find(|measurement| measurement.name == name)
    }
}

/// A cloneable cooperative cancellation handle.  Cancellation never issues a
/// device or service mutation; providers observe it at execution boundaries.
#[derive(Clone, Debug)]
pub struct DiagnosticCancellationToken {
    state: Arc<AtomicU8>,
}

impl Default for DiagnosticCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticCancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(0)),
        }
    }

    pub fn cancel(&self) {
        self.cancel_with_reason(DiagnosticCancellationReason::Requested);
    }

    pub fn cancel_with_reason(&self, reason: DiagnosticCancellationReason) {
        let _ = self.state.compare_exchange(
            0,
            cancellation_code(reason),
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) != 0
    }

    #[must_use]
    pub fn reason(&self) -> Option<DiagnosticCancellationReason> {
        cancellation_reason(self.state.load(Ordering::Acquire))
    }
}

fn cancellation_code(reason: DiagnosticCancellationReason) -> u8 {
    match reason {
        DiagnosticCancellationReason::Requested => 1,
        DiagnosticCancellationReason::DeadlineExceeded => 2,
        DiagnosticCancellationReason::ExecutorStopped => 3,
    }
}

fn cancellation_reason(code: u8) -> Option<DiagnosticCancellationReason> {
    match code {
        1 => Some(DiagnosticCancellationReason::Requested),
        2 => Some(DiagnosticCancellationReason::DeadlineExceeded),
        3 => Some(DiagnosticCancellationReason::ExecutorStopped),
        _ => None,
    }
}

pub const DEFAULT_DIAGNOSTIC_ITEM_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct DiagnosticRunOptions {
    pub cancellation: DiagnosticCancellationToken,
    pub per_item_deadline: Duration,
}

impl Default for DiagnosticRunOptions {
    fn default() -> Self {
        Self {
            cancellation: DiagnosticCancellationToken::new(),
            per_item_deadline: DEFAULT_DIAGNOSTIC_ITEM_DEADLINE,
        }
    }
}

impl DiagnosticRunOptions {
    #[must_use]
    pub fn new(
        cancellation: DiagnosticCancellationToken,
        per_item_deadline: Duration,
    ) -> Self {
        Self {
            cancellation,
            per_item_deadline,
        }
    }

    #[must_use]
    pub fn with_cancellation(mut self, cancellation: DiagnosticCancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    #[must_use]
    pub fn with_per_item_deadline(mut self, per_item_deadline: Duration) -> Self {
        self.per_item_deadline = per_item_deadline;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticRunState {
    Idle,
    Preparing,
    Running,
    Completed,
    Cancelled,
    ResourceError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiagnosticStatusSnapshot {
    pub run_id: u64,
    pub state: DiagnosticRunState,
    pub current_item: Option<DiagnosticKind>,
    pub completed_items: usize,
    pub total_items: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DiagnosticReason>,
}

impl Default for DiagnosticStatusSnapshot {
    fn default() -> Self {
        Self {
            run_id: 0,
            state: DiagnosticRunState::Idle,
            current_item: None,
            completed_items: 0,
            total_items: 0,
            reason: None,
        }
    }
}

impl DiagnosticStatusSnapshot {
    #[must_use]
    pub fn idle() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            DiagnosticRunState::Completed
                | DiagnosticRunState::Cancelled
                | DiagnosticRunState::ResourceError
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCapability {
    TrustedLocalInfOnly,
    Unavailable { reason: String },
}
