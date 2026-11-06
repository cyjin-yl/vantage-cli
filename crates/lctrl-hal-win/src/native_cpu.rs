//! Registry transport for the firmware-published processor block.
//!
//! The firmware publishes one subkey per logical processor under
//! `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\`; subkey `0` carries the
//! package identity. Values read here are identity strings plus the
//! firmware-declared approximate frequency — none of them is a live clock
//! (`docs/evidence/re-cpu-20261105.md` §3).

use std::ffi::c_void;

use lctrl_core::{LctrlError, Result};
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};

use crate::cpu::CpuRegistryReader;
use crate::{map_win_error, wide::wide};

/// `HKLM` subkey holding the first logical processor's firmware block.
const PROCESSOR_KEY: &[u16] = &wide!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");

/// Upper bound for a `REG_SZ` identity value. The longest value the firmware
/// publishes is `ProcessorNameString` (well under 64 UTF-16 units); the bound
/// keeps an oversized value an error instead of silently truncating it.
const MAX_STRING_BYTES: u32 = 512;

/// Native reader for `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeCpuRegistry;

impl CpuRegistryReader for NativeCpuRegistry {
    fn read_string(&self, value: &str) -> Result<String> {
        let name = wide_value(value);
        let mut buffer = [0u16; (MAX_STRING_BYTES / 2) as usize];
        let mut bytes = MAX_STRING_BYTES;
        let mut kind: REG_VALUE_TYPE = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PROCESSOR_KEY.as_ptr(),
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
                &format!("CentralProcessor registry value {value:?}"),
            ));
        }
        if bytes > MAX_STRING_BYTES {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "CentralProcessor registry value {value:?} reported {bytes} bytes; the reader \
                     holds {MAX_STRING_BYTES}"
                ),
            });
        }
        // `bytes` includes the terminating NUL; the decode needs the units only.
        let units = buffer.len().min(bytes as usize / 2);
        let end = buffer[..units]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units);
        String::from_utf16(&buffer[..end]).map_err(|error| LctrlError::ChannelUnavailable {
            channel: format!("CentralProcessor registry value {value:?} is not UTF-16: {error}"),
        })
    }

    fn read_dword(&self, value: &str) -> Result<u32> {
        let name = wide_value(value);
        let mut data = 0u32;
        let mut bytes = u32::try_from(std::mem::size_of_val(&data)).expect("u32 size fits");
        let mut kind: REG_VALUE_TYPE = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PROCESSOR_KEY.as_ptr(),
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
                &format!("CentralProcessor registry value {value:?}"),
            ));
        }
        if bytes != 4 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "CentralProcessor registry value {value:?} has {bytes} bytes; expected 4"
                ),
            });
        }
        Ok(data)
    }
}

/// Widen a runtime value name; the `wide!` macro only takes literals.
fn wide_value(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
