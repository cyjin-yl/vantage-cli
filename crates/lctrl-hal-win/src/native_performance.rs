use std::ffi::c_void;

use lctrl_core::{LctrlError, Result};
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RegGetValueW,
};

use crate::{PerformanceRegistryReader, map_win_error};

const POWER_SLIDER_KEY: &str =
    r"SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider";

#[derive(Clone, Copy, Debug, Default)]
pub struct NativePerformanceRegistry;

impl PerformanceRegistryReader for NativePerformanceRegistry {
    fn read_dword(&self, value: &str) -> Result<u32> {
        let key = wide(POWER_SLIDER_KEY);
        let value = wide(value);
        let mut data = 0u32;
        let mut bytes = u32::try_from(std::mem::size_of_val(&data)).expect("u32 size fits");
        let mut kind: REG_VALUE_TYPE = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                &mut kind,
                (&mut data as *mut u32).cast::<c_void>(),
                &mut bytes,
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("PowerSlider registry value {value:?}"),
            ));
        }
        if bytes != 4 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "PowerSlider registry value {value:?} has {bytes} bytes; expected 4"
                ),
            });
        }
        Ok(data)
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
