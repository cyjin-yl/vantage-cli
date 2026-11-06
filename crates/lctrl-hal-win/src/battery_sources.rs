//! Independent vendor, WMI, native-electrical, and aggregate power sources.
//!
//! No source is merged with another: vendor index and operating-system
//! battery identities are not known to refer to the same physical pack.

use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryElectricalObservation, BatteryFieldProvenance, BatteryTelemetry,
    BatteryWearReport, ChangeReport, ChargeMode, ChargeModeActual, OsBatterySnapshot, Result,
    SystemPowerStatus,
};
use lctrl_hal::{BatteryControl, BatteryElectricalSource};

use crate::OsBatteryChannel;

/// Capacity/cycle facts come from the root\WMI battery class chain, which is
/// the only channel on the target that exposes absolute design capacity.
const WMI_CAPACITY_CHANNEL: &str = "root_wmi_battery_classes";
const WMI_CAPACITY_FIELDS: BatteryFieldProvenance = BatteryFieldProvenance {
    design_capacity: "root\\WMI:BatteryStaticData.DesignedCapacity",
    full_charge_capacity: "root\\WMI:BatteryFullChargedCapacity.FullChargedCapacity",
    cycle_count: "root\\WMI:BatteryCycleCount.CycleCount",
};

pub struct WindowsBatterySources<V, O, E> {
    vendor: V,
    os: O,
    electrical: E,
}

impl<V, O, E> WindowsBatterySources<V, O, E> {
    #[must_use]
    pub const fn new(vendor: V, os: O, electrical: E) -> Self {
        Self {
            vendor,
            os,
            electrical,
        }
    }

    #[must_use]
    pub const fn vendor(&self) -> &V {
        &self.vendor
    }

    #[must_use]
    pub const fn os(&self) -> &O {
        &self.os
    }

    #[must_use]
    pub const fn electrical(&self) -> &E {
        &self.electrical
    }
}

impl<V, O, E> BatteryControl for WindowsBatterySources<V, O, E>
where
    V: BatteryControl,
    O: OsBatteryChannel,
    E: BatteryElectricalSource,
{
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        self.vendor.battery_telemetry(index)
    }

    fn os_battery_snapshot(&self) -> Result<OsBatterySnapshot> {
        self.os.snapshot()
    }

    fn battery_electrical(&self) -> Result<BatteryElectricalObservation> {
        self.electrical.read_electrical()
    }

    /// Wear facts are derived only from the identity-matched OS capacity
    /// observation; the vendor device response is never merged into it.
    fn battery_wear(&self) -> Result<BatteryWearReport> {
        let snapshot = self.os.snapshot()?;
        Ok(BatteryWearReport::from_capacities(
            WMI_CAPACITY_CHANNEL,
            WMI_CAPACITY_FIELDS,
            snapshot.design_capacity_mwh,
            snapshot.full_charge_capacity_mwh,
            snapshot.cycle_count,
            snapshot.source_issues,
        ))
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        self.os.system_power_status()
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        self.vendor.adapter_info()
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        self.vendor.charge_mode()
    }

    fn night_charge(&self) -> Result<lctrl_core::NightChargeState> {
        self.vendor.night_charge()
    }

    fn set_night_charge(&self, enabled: bool, apply: ApplyMode) -> Result<ChangeReport<bool>> {
        self.vendor.set_night_charge(enabled, apply)
    }

    fn temporary_charge(&self) -> Result<lctrl_core::TemporaryChargeState> {
        self.vendor.temporary_charge()
    }

    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        self.vendor.set_charge_mode(mode, apply)
    }
}
