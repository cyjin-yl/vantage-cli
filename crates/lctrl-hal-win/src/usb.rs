//! Runtime USB charging control through the RE-proven `0x831020e8` contract
//! (`PowerBattery.dll` `CUSBCharger` / `CUSBBatteryCharger`; docs/interfaces.md
//! §11). The vendor retries the status query up to 10 times after a write; the
//! same poll bounds verification here.

use std::time::Duration;

use lctrl_core::{ApplyMode, ChangeReport, Result, UsbChargingState};
use lctrl_hal::{UsbControl, poll_readback};

use crate::codec::UsbChargerSelector;
use crate::{EnergyDriver, IoctlTransport};

#[derive(Debug)]
pub struct WindowsUsbPower<T> {
    ioctl: T,
}

impl<T> WindowsUsbPower<T> {
    #[must_use]
    pub const fn new(ioctl: T) -> Self {
        Self { ioctl }
    }

    #[must_use]
    pub const fn ioctl(&self) -> &T {
        &self.ioctl
    }
}

impl<T> WindowsUsbPower<T>
where
    T: IoctlTransport,
{
    fn read_state(&self) -> Result<UsbChargingState> {
        Ok(UsbChargingState::from_flags(
            EnergyDriver::new(&self.ioctl).usb_charger_flags()?,
        ))
    }
}

impl<T> UsbControl for WindowsUsbPower<T>
where
    T: IoctlTransport,
{
    fn usb_charging(&self) -> Result<UsbChargingState> {
        self.read_state()
    }

    fn set_usb_ac_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<UsbChargingState>> {
        let previous = self.read_state()?;
        if apply == ApplyMode::DryRun {
            let mut requested = previous;
            requested.ac_enabled = enabled;
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let selector = if enabled {
            UsbChargerSelector::AC_ON
        } else {
            UsbChargerSelector::AC_OFF
        };
        EnergyDriver::new(&self.ioctl).write_usb_charger(selector)?;
        let mut requested = previous;
        requested.ac_enabled = enabled;
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }

    fn set_usb_battery_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<UsbChargingState>> {
        let previous = self.read_state()?;
        if apply == ApplyMode::DryRun {
            let mut requested = previous;
            requested.battery_enabled = enabled;
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let selector = if enabled {
            UsbChargerSelector::BATTERY_ON
        } else {
            UsbChargerSelector::BATTERY_OFF
        };
        EnergyDriver::new(&self.ioctl).write_usb_charger(selector)?;
        let mut requested = previous;
        requested.battery_enabled = enabled;
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}
