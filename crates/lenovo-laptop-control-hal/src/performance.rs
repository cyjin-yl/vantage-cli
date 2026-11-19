use lenovo_laptop_control_core::{
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

    /// Live tachometer readings, one `(FanId, rpm)` per firmware-visible fan.
    ///
    /// On ThinkBook 21VG this is `LENOVO_OTHER_METHOD.GetFeatureValue` SIDS
    /// `0x04030001` (ACPI `WMAE` → `ECCC(0x64)` → `ECRT`), verified live
    /// 2026-11-19 (docs/evidence/re-fan-wmae-channel-20261119.md).
    fn fan_speeds(&self) -> Result<Vec<(FanId, u32)>> {
        Err(lenovo_laptop_control_core::LenovoLaptopControlError::Unsupported {
            feature: "perf.fan.rpm".into(),
        })
    }

    /// Direct fan-duty override, `duty_pct` = 0..=100.
    ///
    /// The override latches until the next write or reboot; the firmware
    /// holds an ~1800 RPM floor at the low end, so a low write cannot stall
    /// the fan. `previous`/`actual` are estimated from live RPM against the
    /// inventory bounds — the duty register is write-only
    /// (docs/evidence/re-fan-wmae-channel-20261119.md §5).
    fn set_fan_duty(&self, duty_pct: u8, apply: ApplyMode) -> Result<ChangeReport<u8>> {
        let _ = (duty_pct, apply);
        Err(lenovo_laptop_control_core::LenovoLaptopControlError::Unsupported {
            feature: "perf.fan.duty.write".into(),
        })
    }
}

pub trait TemperatureControl: Send + Sync {
    fn temperature_sensors(&self) -> Result<Vec<lenovo_laptop_control_core::TemperatureSensor>>;
    fn temperature(&self, id: &str) -> Result<lenovo_laptop_control_core::TemperatureSensor>;
}

/// Read-only battery direct-discharge current-limit (DBDC) table.
///
/// Writes stay gated: `LENOVO_OTHER_METHOD.SetFeatureValue` needs a recovered
/// FeatureId↔name mapping (docs/interfaces.md §10.3), and the tier-write
/// semantics are not verified on the target.
pub trait DbdcControl: Send + Sync {
    fn dbdc_profile(&self) -> Result<lenovo_laptop_control_core::DbdcProfile>;
}

/// Read-only GPU overclock status (`LENOVO_GAMEZONE_DATA.IsSupportGpuOC` plus
/// the `LENOVO_GAMEZONE_GPU_OC_DATA` parameter block).
///
/// There is no write counterpart: the vendor OC writers are NVAPI
/// `NvAPI_GPU_SetPstates20` and Intel's OC SDK, neither of which is a Lenovo
/// WMI verb, and no reversible write with readback is documented on this
/// interface (`evidence/re-gpu-oc-20261105.md`).
pub trait GpuOcControl: Send + Sync {
    fn gpu_oc_status(&self) -> Result<lenovo_laptop_control_core::GpuOcStatus>;
}
