use std::ffi::c_void;

use lctrl_core::{LctrlError, Result};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_DWORD, REG_VALUE_TYPE, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};

use crate::{PerformanceRegistryReader, map_win_error};

const POWER_SLIDER_KEY: &str =
    r"SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider";

#[derive(Clone, Copy, Debug, Default)]
pub struct NativePerformanceRegistry;

impl PerformanceRegistryReader for NativePerformanceRegistry {
    fn read_dword(&self, value: &str) -> Result<u32> {
        // The value *name* is kept separately from the NUL-terminated wide
        // buffer. Shadowing it with the buffer made every error message print
        // the UTF-16 code units (for example `[67, 85, 82, 82, 69, 78, 84]`)
        // instead of the name the caller asked for.
        let key = wide(POWER_SLIDER_KEY);
        let name = wide(value);
        let mut data = 0u32;
        let mut bytes = u32::try_from(std::mem::size_of_val(&data)).expect("u32 size fits");
        let mut kind: REG_VALUE_TYPE = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
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

    fn write_dword(&self, value: &str, data: u32) -> Result<()> {
        let key = wide(POWER_SLIDER_KEY);
        let name = wide(value);
        let mut handle: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                0,
                std::ptr::null_mut(),
                0,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut handle,
                std::ptr::null_mut(),
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("open PowerSlider registry key for {value:?}"),
            ));
        }
        let bytes = data.to_ne_bytes();
        let status = unsafe {
            RegSetValueExW(
                handle,
                name.as_ptr(),
                0,
                REG_DWORD,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        unsafe { RegCloseKey(handle) };
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("write PowerSlider registry value {value:?}"),
            ));
        }
        Ok(())
    }

    fn read_string(&self, value: &str) -> Result<String> {
        // The dispatcher reads the curve tables into a 520-byte buffer
        // (`movl $0x208` before `RegQueryValueExW`) and expects `REG_SZ`.
        // Matching that bound means an oversized value surfaces as
        // `ERROR_MORE_DATA` instead of being cut down to a shorter table.
        const MAX_BYTES: u32 = 520;
        const MAX_UNITS: usize = (MAX_BYTES / 2) as usize;
        let key = wide(POWER_SLIDER_KEY);
        let name = wide(value);
        let mut buffer = [0u16; MAX_UNITS];
        let mut bytes = MAX_BYTES;
        let mut kind: REG_VALUE_TYPE = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                &mut kind,
                buffer.as_mut_ptr().cast::<c_void>(),
                &mut bytes,
            )
        };
        if status != 0 {
            return Err(map_win_error(
                status,
                &format!("PowerSlider registry value {value:?}"),
            ));
        }
        if bytes > MAX_BYTES {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "PowerSlider registry value {value:?} reported {bytes} bytes; buffer holds \
                     {MAX_BYTES}"
                ),
            });
        }
        // `bytes` counts the terminating NUL; the decode needs the units only.
        let units = buffer.len().min(bytes as usize / 2);
        let end = buffer[..units]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units);
        Ok(String::from_utf16_lossy(&buffer[..end]))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
