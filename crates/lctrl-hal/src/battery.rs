use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryElectricalObservation, BatteryTelemetry, ChangeReport,
    ChargeMode, ChargeModeActual, LctrlError, OsBatterySnapshot, Result, SystemPowerStatus,
};

pub trait BatteryElectricalSource: Send + Sync {
    fn read_electrical(&self) -> Result<BatteryElectricalObservation>;
}

pub trait BatteryControl: Send + Sync {
    /// One backend-specific battery reading. Never merge unbound sources.
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry>;
    /// A distinct, internally identity-bound OS battery observation.
    fn os_battery_snapshot(&self) -> Result<OsBatterySnapshot> {
        Err(LctrlError::Unsupported {
            feature: "battery.os-observation".into(),
        })
    }
    /// A distinct native battery-class electrical observation.
    fn battery_electrical(&self) -> Result<BatteryElectricalObservation> {
        Err(LctrlError::Unsupported {
            feature: "battery.electrical-observation".into(),
        })
    }
    /// Aggregate system power is not a per-pack observation.
    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        Err(LctrlError::Unsupported {
            feature: "battery.system-power".into(),
        })
    }
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

    fn os_battery_snapshot(&self) -> Result<OsBatterySnapshot> {
        (**self).os_battery_snapshot()
    }

    fn battery_electrical(&self) -> Result<BatteryElectricalObservation> {
        (**self).battery_electrical()
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        (**self).system_power_status()
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
