use lctrl_core::{
    DiagnosticKind, DiagnosticResult, DiagnosticRunOptions, DiagnosticStatusSnapshot, Result,
    UpdateCapability,
};

pub trait DiagnosticsControl: Send + Sync {
    fn diagnostic_items(&self) -> Result<Vec<DiagnosticKind>>;

    /// Run each requested item with cooperative cancellation and a finite
    /// per-item deadline. Item-level failures are represented in the returned
    /// results rather than being promoted to a whole-run error.
    fn run_diagnostics(
        &self,
        items: &[DiagnosticKind],
        options: DiagnosticRunOptions,
    ) -> Result<Vec<DiagnosticResult>>;

    /// Return the latest executor state without starting a device operation.
    /// Backends without an executor retain the idle default; the Windows
    /// backend overrides this with its bounded serial worker snapshot.
    fn diagnostic_status(&self) -> DiagnosticStatusSnapshot {
        DiagnosticStatusSnapshot::idle()
    }
}

pub trait UpdateControl: Send + Sync {
    fn update_capability(&self) -> Result<UpdateCapability>;

    /// Report what the machine's synced update catalog offers, after the
    /// device's own applicability filter. Read-only. Backends without a
    /// readable catalog stay honest.
    fn check_updates(&self) -> Result<lctrl_core::UpdateCheckReport> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "update.check".into(),
        })
    }
}
