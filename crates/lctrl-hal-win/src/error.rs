use std::io;

use lctrl_core::LctrlError;

const ERROR_INVALID_FUNCTION: u32 = 1;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NOT_READY: u32 = 21;
const ERROR_HANDLE_EOF: u32 = 38;
const ERROR_NOT_SUPPORTED: u32 = 50;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_OPERATION_ABORTED: u32 = 995;
const ERROR_IO_DEVICE: u32 = 1117;

const WBEM_E_NOT_FOUND: i32 = 0x8004_1002_u32 as i32;
const WBEM_E_ACCESS_DENIED: i32 = 0x8004_1003_u32 as i32;
const WBEM_E_INVALID_PARAMETER: i32 = 0x8004_1008_u32 as i32;
const WBEM_E_INVALID_OBJECT: i32 = 0x8004_100f_u32 as i32;
const WBEM_E_INVALID_CLASS: i32 = 0x8004_1010_u32 as i32;

/// Converts a Win32 `GetLastError` value from an EnergyDrv or WMI channel
/// into the cross-platform error contract.
#[must_use]
pub fn map_win_error(gle: u32, context: &str) -> LctrlError {
    match gle {
        ERROR_ACCESS_DENIED => LctrlError::PermissionDenied {
            need: "administrator".into(),
        },
        ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED | ERROR_INVALID_PARAMETER => {
            LctrlError::Unsupported {
                feature: context.into(),
            }
        }
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_NOT_READY | ERROR_HANDLE_EOF => {
            LctrlError::ChannelUnavailable {
                channel: context.into(),
            }
        }
        ERROR_IO_DEVICE => LctrlError::FirmwareRejected {
            detail: format!("{context}: I/O device error"),
        },
        ERROR_OPERATION_ABORTED => LctrlError::Io(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("{context}: Windows operation was aborted"),
        )),
        _ => LctrlError::Io(io::Error::other(format!(
            "{context}: Win32 error {gle} (0x{gle:08x})"
        ))),
    }
}

/// Converts WMI HRESULTs into stable user-facing errors.
///
/// `WBEM_E_INVALID_PARAMETER` is deliberately classified as unsupported:
/// on the target firmware it is the observable result of the unimplemented
/// GAMEZONE method family, which must be negatively capability-cached rather
/// than retried through another invocation shape.
#[must_use]
pub fn map_wmi_hresult(hresult: i32, context: &str) -> LctrlError {
    match hresult {
        WBEM_E_NOT_FOUND
        | WBEM_E_INVALID_PARAMETER
        | WBEM_E_INVALID_OBJECT
        | WBEM_E_INVALID_CLASS => LctrlError::Unsupported {
            feature: context.into(),
        },
        WBEM_E_ACCESS_DENIED => LctrlError::PermissionDenied {
            need: "administrator".into(),
        },
        _ => {
            let raw = hresult as u32;
            LctrlError::Io(io::Error::other(format!(
                "{context}: WMI HRESULT 0x{raw:08X}"
            )))
        }
    }
}
