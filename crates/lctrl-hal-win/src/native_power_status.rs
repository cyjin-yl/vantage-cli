//! The operating system's aggregate power status on Windows.
//!
//! `GetSystemPowerStatus` is a documented Windows API. It is the only
//! portable source for "is external power present", so it backs the adapter
//! view on machines without the vendor energy driver.

use lctrl_core::{Result, SystemPowerStatus};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::SystemPowerStatusReader;

/// Reads the aggregate power status through `GetSystemPowerStatus`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSystemPowerStatus;

impl SystemPowerStatusReader for NativeSystemPowerStatus {
    fn read(&self) -> Result<SystemPowerStatus> {
        let mut status = SYSTEM_POWER_STATUS {
            ACLineStatus: 0,
            BatteryFlag: 0,
            BatteryLifePercent: 0,
            SystemStatusFlag: 0,
            BatteryLifeTime: 0,
            BatteryFullLifeTime: 0,
        };
        let ok = unsafe { GetSystemPowerStatus(&mut status) };
        if ok == 0 {
            return Err(lctrl_core::LctrlError::ChannelUnavailable {
                channel: "GetSystemPowerStatus".into(),
            });
        }

        Ok(SystemPowerStatus {
            ac_line_status: match status.ACLineStatus {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            },
            battery_flag: Some(status.BatteryFlag),
            // 255 means "percentage unknown" on this interface.
            battery_life_percent: (status.BatteryLifePercent != u8::MAX)
                .then_some(status.BatteryLifePercent),
            battery_life_time: (status.BatteryLifeTime != u32::MAX)
                .then_some(status.BatteryLifeTime),
            battery_full_life_time: (status.BatteryFullLifeTime != u32::MAX)
                .then_some(status.BatteryFullLifeTime),
        })
    }
}
