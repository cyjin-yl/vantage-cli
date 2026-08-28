use lctrl_core::{DiagnosticKind, DiagnosticResult, Result, UpdateCapability};

pub trait DiagnosticsControl: Send + Sync {
    fn diagnostic_items(&self) -> Result<Vec<DiagnosticKind>>;
    fn run_diagnostics(&self, items: &[DiagnosticKind]) -> Result<Vec<DiagnosticResult>>;
}

pub trait UpdateControl: Send + Sync {
    fn update_capability(&self) -> Result<UpdateCapability>;
}
