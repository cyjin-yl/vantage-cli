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

impl<T: BatteryControl + ?Sized> BatteryControl for &T {
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        (**self).battery_telemetry(index)
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        (**self).adapter_info()
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        (**self).charge_mode()
    }

    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        (**self).set_charge_mode(mode, apply)
    }
}
