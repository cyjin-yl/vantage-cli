//! Combining the vendor battery channel with the portable OS channel.
//!
//! The vendor channel (EnergyDrv) carries more identity detail but only
//! exists where the Lenovo energy driver is installed. The OS channel works
//! everywhere but reports fewer fields. This composition prefers the vendor
//! reading and fills only the fields it left unknown, so a machine without
//! the driver still reports battery and AC state.

use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode, ChargeModeActual, Result,
};
use lctrl_hal::BatteryControl;

use crate::OsBatteryChannel;

/// A battery controller that prefers the vendor channel and falls back to the
/// operating system channel.
pub struct WindowsBatteryFallback<V, O> {
    vendor: V,
    os: O,
}

impl<V, O> WindowsBatteryFallback<V, O> {
    /// Combine a vendor battery controller with an OS battery channel.
    pub const fn new(vendor: V, os: O) -> Self {
        Self { vendor, os }
    }

    /// The vendor battery controller.
    pub const fn vendor(&self) -> &V {
        &self.vendor
    }

    /// The operating system battery channel.
    pub const fn os(&self) -> &O {
        &self.os
    }
}

impl<V, O> BatteryControl for WindowsBatteryFallback<V, O>
where
    V: BatteryControl,
    O: OsBatteryChannel,
{
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        let vendor = self.vendor.battery_telemetry(index);
        let os = self.os.snapshot();
        match (vendor, os) {
            (Ok(mut telemetry), Ok(snapshot)) => {
                let os_telemetry = BatteryTelemetry::from_os_snapshot(&snapshot)?;
                telemetry.fill_missing_from(&os_telemetry);
                // The operating system reports the actual charging and
                // discharging flags, while the vendor structure only
                // carries an inferred status word. Prefer the explicit
                // observation over an unfilled field.
                if os_telemetry.charge_status.is_some() {
                    telemetry.charge_status = os_telemetry.charge_status;
                }
                Ok(telemetry)
            }
            (Ok(telemetry), Err(_)) => Ok(telemetry),
            (Err(error), Ok(snapshot)) => {
                BatteryTelemetry::from_os_snapshot(&snapshot).map_err(|_| {
                    // The vendor error is the more specific failure: it names
                    // the channel that could not be read.
                    error
                })
            }
            (Err(error), Err(_)) => Err(error),
        }
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        match self.vendor.adapter_info() {
            Ok(mut info) => {
                // The vendor adapter channel only reports the AC line state
                // when a detailed charger response is available. The
                // operating system always knows the line state, so it fills
                // the gap without overriding a vendor observation.
                if info.ac_connected.is_none() {
                    if let Ok(status) = self.os.system_power_status() {
                        info.ac_connected = status.ac_line_status;
                    }
                }
                Ok(info)
            }
            Err(error) => match self.os.system_power_status() {
                Ok(status) => Ok(AdapterInfo::from_system_power_status(&status)),
                Err(_) => Err(error),
            },
        }
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        self.vendor.charge_mode()
    }

    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        self.vendor.set_charge_mode(mode, apply)
    }
}
