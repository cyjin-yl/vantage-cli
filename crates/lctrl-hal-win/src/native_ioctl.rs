use std::{ffi::c_void, ptr};

use lctrl_core::{LctrlError, Result};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::IO::DeviceIoControl,
};

use crate::{
    IOCTL_BATTERY_CONFIG, IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET,
    IOCTL_NIGHT_CHARGE, IOCTL_TEMP_CHARGE, IOCTL_USB_CHARGER, IoctlTransport, map_win_error,
};

const ENERGY_DRIVER_PATH: &str = r"\\.\EnergyDrv";

/// Bound `DeviceIoControl` transport for a device interface path. Unlike
/// [`NativeIoctl`] it performs no request validation and always opens with
/// read+write access; contract validation lives in the caller (see `msr`).
#[derive(Clone, Copy, Debug)]
pub struct DeviceIoTransport {
    path: &'static str,
    context: &'static str,
}

impl DeviceIoTransport {
    #[must_use]
    pub const fn new(path: &'static str, context: &'static str) -> Self {
        Self { path, context }
    }

    /// Device path this transport opens.
    #[must_use]
    pub const fn path(&self) -> &'static str {
        self.path
    }
}

impl IoctlTransport for DeviceIoTransport {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> Result<Vec<u8>> {
        let handle = EnergyDriverHandle::open_path(self.path, self.context, true)?;
        handle.call_context(code, input, output_len, self.context)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeIoctl;

impl IoctlTransport for NativeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> Result<Vec<u8>> {
        let write = validate_request(code, input)?;
        let handle = EnergyDriverHandle::open(write)?;
        handle.call(code, input, output_len)
    }
}

fn validate_request(code: u32, input: &[u8]) -> Result<bool> {
    match code {
        IOCTL_GBMD => match input {
            [0xff] => Ok(false),
            [0x0d | 0x0f | 0x07 | 0x08] => Ok(true),
            [other] => Err(LctrlError::Unsupported {
                feature: format!("EnergyDrv GBMD subcommand 0x{other:02x}"),
            }),
            _ => Err(LctrlError::InvalidArgument {
                detail: "EnergyDrv GBMD requires exactly one input byte".into(),
            }),
        },
        IOCTL_NIGHT_CHARGE => match input {
            [0x13, 0, 0, 0] | [0x11, 0, 0, 0] => Ok(false),
            [0x12, 0, 0, 0] | [0x12, 0, 0, 0x80] => Ok(true),
            _ => Err(LctrlError::Unsupported {
                feature: format!("EnergyDrv night-charge selector {input:?}"),
            }),
        },
        IOCTL_TEMP_CHARGE => match input {
            [0x13, 0, 0, 0] | [0x11, 0, 0, 0] => Ok(false),
            _ => Err(LctrlError::Unsupported {
                feature: format!("EnergyDrv temporary-charge selector {input:?}"),
            }),
        },
        IOCTL_GENERIC_GET | IOCTL_BATTERY_CONFIG | IOCTL_BATTERY_DETAIL | IOCTL_GAPD => Ok(false),
        IOCTL_USB_CHARGER => match input {
            // RE-proven contract (docs/interfaces.md §11): only 0x02 is a
            // query; every other dispatched byte reaches the EC setter.
            [0x02] => Ok(false),
            [0x0a | 0x0b | 0x12 | 0x13] => Ok(true),
            [other] => Err(LctrlError::Unsupported {
                feature: format!("EnergyDrv USB charger selector 0x{other:02x}"),
            }),
            _ => Err(LctrlError::InvalidArgument {
                detail: "EnergyDrv USB charger requires exactly one input byte".into(),
            }),
        },
        _ => Err(LctrlError::Unsupported {
            feature: format!("EnergyDrv IOCTL 0x{code:08x}"),
        }),
    }
}

struct EnergyDriverHandle(HANDLE);

impl EnergyDriverHandle {
    fn open(write: bool) -> Result<Self> {
        Self::open_path(ENERGY_DRIVER_PATH, "EnergyDrv", write)
    }

    fn open_path(path: &str, context: &str, write: bool) -> Result<Self> {
        let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let access = if write {
            GENERIC_READ | GENERIC_WRITE
        } else {
            GENERIC_READ
        };
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            return Err(map_win_error(error, &format!("{context} open")));
        }
        Ok(Self(handle))
    }

    fn call(&self, code: u32, input: &[u8], output_len: usize) -> Result<Vec<u8>> {
        self.call_context(code, input, output_len, "EnergyDrv")
    }

    fn call_context(
        &self,
        code: u32,
        input: &[u8],
        output_len: usize,
        context: &str,
    ) -> Result<Vec<u8>> {
        let input_len = u32::try_from(input.len()).map_err(|_| LctrlError::InvalidArgument {
            detail: format!("{context} input exceeds u32 buffer length"),
        })?;
        let requested_output =
            u32::try_from(output_len).map_err(|_| LctrlError::InvalidArgument {
                detail: format!("{context} output exceeds u32 buffer length"),
            })?;
        let input_ptr = if input.is_empty() {
            ptr::null()
        } else {
            input.as_ptr().cast::<c_void>()
        };
        let mut output = vec![0; output_len];
        let output_ptr = if output.is_empty() {
            ptr::null_mut()
        } else {
            output.as_mut_ptr().cast::<c_void>()
        };
        let mut returned = 0u32;
        let success = unsafe {
            DeviceIoControl(
                self.0,
                code,
                input_ptr,
                input_len,
                output_ptr,
                requested_output,
                &mut returned,
                ptr::null_mut(),
            )
        };
        if success == 0 {
            let error = unsafe { GetLastError() };
            return Err(map_win_error(
                error,
                &format!("{context} IOCTL 0x{code:08x}"),
            ));
        }
        let returned = usize::try_from(returned).map_err(|_| {
            LctrlError::Io(std::io::Error::other(format!(
                "{context} byte count does not fit usize"
            )))
        })?;
        if returned > output.len() {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "{context} IOCTL 0x{code:08x} returned {returned} bytes into {}-byte buffer",
                    output.len()
                ),
            });
        }
        output.truncate(returned);
        Ok(output)
    }
}

impl Drop for EnergyDriverHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
