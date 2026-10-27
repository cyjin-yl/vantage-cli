use lctrl_core::{
    ApplyMode, ChangeReport, ProcessInventory, ProcessPolicy, ProcessSnapshot, ProcessState,
    ProcessTarget, Result,
};

/// Per-process inventory and mutation contract. Implementations must preserve
/// the target's PID + creation-time identity guard through every mutation.
pub trait ProcessControl: Send + Sync {
    /// Return at most `limit` processes, ordered by sampled interval CPU time.
    fn top_processes(&self, limit: usize) -> Result<ProcessInventory>;

    fn process_snapshot(&self, target: &ProcessTarget) -> Result<ProcessSnapshot>;

    fn boost_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>>;

    fn throttle_process(
        &self,
        target: &ProcessTarget,
        policy: ProcessPolicy,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>>;

    /// Restore the exact priority, affinity and working-set quota state from a
    /// prior snapshot of the same named process instance.
    fn restore_process(
        &self,
        target: &ProcessTarget,
        snapshot: &ProcessSnapshot,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>>;
}
