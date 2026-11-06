use lctrl_core::{
    ApplyMode, ChangeReport, FanDescriptor, FanId, FanMode, FanTable, ItsPolicy,
    PerformanceMode, PerformanceState, Result, SensorId,
};

pub trait PerformanceControl: Send + Sync {
    fn performance_state(&self) -> Result<PerformanceState>;
    fn set_performance_mode(
        &self,
        mode: PerformanceMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PerformanceMode>>;
}

/// Read-only decode of the dispatcher's `Policy_*` `PowerSlider` values.
///
/// Nothing on this surface writes: the values it reports are the vendor's own
/// published policy, decoded where the dispatcher's binary proves the packing
/// and left raw where it does not.
pub trait ItsPolicyControl: Send + Sync {
    fn its_policy(&self) -> Result<ItsPolicy>;
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

/// Read-only battery direct-discharge current-limit (DBDC) table.
///
/// Writes stay gated: `LENOVO_OTHER_METHOD.SetFeatureValue` needs a recovered
/// FeatureId↔name mapping (docs/interfaces.md §10.3), and the tier-write
/// semantics are not verified on the target.
pub trait DbdcControl: Send + Sync {
    fn dbdc_profile(&self) -> Result<lctrl_core::DbdcProfile>;
}

/// Read-only GPU overclock status (`LENOVO_GAMEZONE_DATA.IsSupportGpuOC` plus
/// the `LENOVO_GAMEZONE_GPU_OC_DATA` parameter block).
///
/// There is no write counterpart: the vendor OC writers are NVAPI
/// `NvAPI_GPU_SetPstates20` and Intel's OC SDK, neither of which is a Lenovo
/// WMI verb, and no reversible write with readback is documented on this
/// interface (`evidence/re-gpu-oc-20261105.md`).
pub trait GpuOcControl: Send + Sync {
    fn gpu_oc_status(&self) -> Result<lctrl_core::GpuOcStatus>;
}
