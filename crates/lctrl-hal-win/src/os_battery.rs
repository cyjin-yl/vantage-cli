//! Battery readings from the operating system's own battery interfaces.
//!
//! The vendor EnergyDrv channel is richer, but it only exists on machines
//! that shipped the Lenovo energy driver. This module reads the portable
//! Windows battery class through WMI plus the aggregate system power status,
//! so battery and AC state remain observable on any Windows machine.
//!
//! Everything here is a public Windows interface: the `root\WMI` battery
//! classes and `GetSystemPowerStatus`. No vendor binary is inspected.

use lctrl_core::{LctrlError, OsBatterySnapshot, Result, SystemPowerStatus};

use crate::{WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};

const BATTERY_STATUS: &str = "BatteryStatus";
const BATTERY_FULL_CHARGED_CAPACITY: &str = "BatteryFullChargedCapacity";
const BATTERY_CYCLE_COUNT: &str = "BatteryCycleCount";
const BATTERY_RUNTIME: &str = "BatteryRuntime";
const BATTERY_STATIC_DATA: &str = "BatteryStaticData";

/// A source of the operating system's aggregate power status.
pub trait SystemPowerStatusReader: Send + Sync {
    fn read(&self) -> Result<SystemPowerStatus>;
}

/// Reads battery state through the public Windows battery classes.
pub trait OsBatteryChannel: Send + Sync {
    /// The battery reading, or a channel error when no battery is exposed.
    fn snapshot(&self) -> Result<OsBatterySnapshot>;

    /// The aggregate system power status.
    fn system_power_status(&self) -> Result<SystemPowerStatus>;
}

/// The Windows implementation of the public battery channel.
pub struct WindowsOsBattery<W, P> {
    wmi: W,
    power: P,
}

impl<W, P> WindowsOsBattery<W, P> {
    /// Combine a WMI transport with a system power status reader.
    pub const fn new(wmi: W, power: P) -> Self {
        Self { wmi, power }
    }

    /// The WMI transport used for battery class queries.
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    /// The system power status reader.
    pub const fn power(&self) -> &P {
        &self.power
    }
}

impl<W, P> OsBatteryChannel for WindowsOsBattery<W, P>
where
    W: WmiTransport,
    P: SystemPowerStatusReader,
{
    fn snapshot(&self) -> Result<OsBatterySnapshot> {
        let status = self
            .wmi
            .query(ROOT_WMI, &format!("SELECT * FROM {BATTERY_STATUS}"))?
            .into_iter()
            .next()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("{ROOT_WMI} class {BATTERY_STATUS} exposed no instance"),
            })?;

        let power = self.power.read().ok();
        let mut snapshot = OsBatterySnapshot {
            remaining_capacity_mwh: u32_field(&status, "RemainingCapacity"),
            full_charge_capacity_mwh: None,
            design_capacity_mwh: None,
            voltage_mv: u32_field(&status, "Voltage").and_then(|value| u16::try_from(value).ok()),
            charge_rate_ma: i32_field(&status, "ChargeRate"),
            discharge_rate_ma: i32_field(&status, "DischargeRate"),
            remaining_percent: power.and_then(|status| status.battery_life_percent()),
            cycle_count: None,
            temperature_deci_kelvin: None,
            estimated_runtime_min: None,
            power_online: bool_field(&status, "PowerOnline"),
            charging: bool_field(&status, "Charging"),
            discharging: bool_field(&status, "Discharging"),
            critical: bool_field(&status, "Critical"),
        };

        if let Some(capacity) = self
            .wmi
            .query(
                ROOT_WMI,
                &format!("SELECT * FROM {BATTERY_FULL_CHARGED_CAPACITY}"),
            )?
            .into_iter()
            .next()
            .and_then(|object| u32_field(&object, "FullChargedCapacity"))
        {
            snapshot.full_charge_capacity_mwh = Some(capacity);
        }

        if let Some(cycles) = self
            .wmi
            .query(ROOT_WMI, &format!("SELECT * FROM {BATTERY_CYCLE_COUNT}"))?
            .into_iter()
            .next()
            .and_then(|object| u32_field(&object, "CycleCount"))
            .and_then(|value| u16::try_from(value).ok())
        {
            snapshot.cycle_count = Some(cycles);
        }

        if let Some(runtime) = self
            .wmi
            .query(ROOT_WMI, &format!("SELECT * FROM {BATTERY_RUNTIME}"))?
            .into_iter()
            .next()
            .and_then(|object| u32_field(&object, "EstimatedRuntime"))
            .filter(|seconds| *seconds != u32::MAX)
            .and_then(|seconds| u16::try_from(seconds / 60).ok())
        {
            snapshot.estimated_runtime_min = Some(runtime);
        }

        if let Some(design) = self
            .wmi
            .query(ROOT_WMI, &format!("SELECT * FROM {BATTERY_STATIC_DATA}"))?
            .into_iter()
            .next()
            .and_then(|object| u32_field(&object, "DesignedCapacity"))
        {
            snapshot.design_capacity_mwh = Some(design);
        }

        Ok(snapshot)
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        self.power.read()
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    match object.get(name) {
        Some(WmiValue::U32(value)) => Some(*value),
        Some(WmiValue::U16(value)) => Some(u32::from(*value)),
        Some(WmiValue::I32(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::I16(value)) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn i32_field(object: &WmiObject, name: &str) -> Option<i32> {
    match object.get(name) {
        Some(WmiValue::I32(value)) => Some(*value),
        Some(WmiValue::I16(value)) => Some(i32::from(*value)),
        Some(WmiValue::U32(value)) => i32::try_from(*value).ok(),
        Some(WmiValue::U16(value)) => Some(i32::from(*value)),
        _ => None,
    }
}

fn bool_field(object: &WmiObject, name: &str) -> Option<bool> {
    match object.get(name) {
        Some(WmiValue::Bool(value)) => Some(*value),
        _ => None,
    }
}
