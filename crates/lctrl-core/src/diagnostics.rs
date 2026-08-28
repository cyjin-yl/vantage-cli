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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOutcome {
    Passed,
    Warning,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiagnosticResult {
    pub kind: DiagnosticKind,
    pub outcome: DiagnosticOutcome,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCapability {
    TrustedLocalInfOnly,
    Unavailable { reason: String },
}
