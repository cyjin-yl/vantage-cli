use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode, ChargeModeActual, Result,
};

pub trait BatteryControl: Send + Sync {
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry>;
    fn adapter_info(&self) -> Result<AdapterInfo>;
    fn charge_mode(&self) -> Result<ChargeModeActual>;
    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>>;
}
