use lctrl_core::{
    ApplyMode, ChangeReport, FanDescriptor, FanId, FanTable, PerformanceMode, PerformanceState,
    Result, SensorId,
};

pub trait PerformanceControl: Send + Sync {
    fn performance_state(&self) -> Result<PerformanceState>;
    fn set_performance_mode(
        &self,
        mode: PerformanceMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>>;
}

pub trait FanControl: Send + Sync {
    fn fans(&self) -> Result<Vec<FanDescriptor>>;
    fn fan_table(&self, fan: FanId, sensor: SensorId) -> Result<FanTable>;
}
