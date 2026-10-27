use lctrl_core::{
    ApplyMode, ChangeReport, FanDescriptor, FanId, FanMode, FanTable, PerformanceMode,
    PerformanceState, Result, SensorId,
};

pub trait PerformanceControl: Send + Sync {
    fn performance_state(&self) -> Result<PerformanceState>;
    fn set_performance_mode(
        &self,
        mode: PerformanceMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerLimitKind {
    Pl1,
    Pl2,
    Tau,
}

pub trait TuningControl: Send + Sync {
    fn epp(&self) -> Result<u8>;
    fn set_epp(&self, value: u8, apply: ApplyMode) -> Result<ChangeReport<u8>>;
    fn power_limit(&self, kind: PowerLimitKind) -> Result<u64>;
    fn set_power_limit(
        &self,
        kind: PowerLimitKind,
        value: u64,
        apply: ApplyMode,
    ) -> Result<ChangeReport<u64>>;
}

pub trait FanControl: Send + Sync {
    fn fan_mode(&self) -> Result<FanMode>;
    fn set_fan_mode(&self, mode: FanMode, apply: ApplyMode) -> Result<ChangeReport<FanMode>>;
    fn fans(&self) -> Result<Vec<FanDescriptor>>;
    fn fan_table(&self, fan: FanId, sensor: SensorId) -> Result<FanTable>;
}

pub trait TemperatureControl: Send + Sync {
    fn temperature_sensors(&self) -> Result<Vec<lctrl_core::TemperatureSensor>>;
    fn temperature(&self, id: &str) -> Result<lctrl_core::TemperatureSensor>;
}
