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
/// `WBEM_E_NOT_SUPPORTED` — the firmware does not implement the method on this
/// SKU. Measured on the target (2026-11-06) together with the provider's own
/// "Not supported" text; `LENOVO_LIGHTING_DATA`/`_METHOD`, `LENOVO_FAN_METHOD`
/// and `LENOVO_PANEL_METHOD` all answer it.
///
/// `0x80041001` is `WBEM_E_FAILED` (generic failure, unrelated to absence) and
/// MUST stay outside the `Unsupported` arm — classifying it as absence would
/// turn a real provider failure into a silently "unsupported" feature.
const WBEM_E_NOT_SUPPORTED: i32 = 0x8004_100C_u32 as i32;
const WBEM_E_ACCESS_DENIED: i32 = 0x8004_1003_u32 as i32;
const WBEM_E_INVALID_PARAMETER: i32 = 0x8004_1008_u32 as i32;
const WBEM_E_INVALID_OBJECT: i32 = 0x8004_100f_u32 as i32;
const WBEM_E_INVALID_CLASS: i32 = 0x8004_1010_u32 as i32;
/// `WBEM_E_INVALID_METHOD_PARAMETERS` — an ACPI-WMI method invoked without the
/// instance it belongs to (proven live 2026-11-06 on
/// `LENOVO_LIGHTING_METHOD.Get_Lighting_Current_Status` and
/// `LENOVO_GAMEZONE_DATA.GetKeyboardLight`). Never a firmware verdict on its
/// own, but it is the fixed answer of an unimplemented family when the caller
/// has only the class object.
const WBEM_E_INVALID_METHOD_PARAMETERS: i32 = 0x8004_102F_u32 as i32;

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
///
/// `WBEM_E_NOT_SUPPORTED` (`0x8004100C`) is the code this firmware actually
/// returns for a method it does not implement; `WBEM_E_INVALID_OBJECT` is its
/// per-method rejection on a live instance. Neither may be reported as an I/O
/// error, because `WindowsKeyboardSources::channel_absent` and `active_instance`
/// key on `Unsupported` to fall back to a working channel.
#[must_use]
pub fn map_wmi_hresult(hresult: i32, context: &str) -> LctrlError {
    match hresult {
        WBEM_E_NOT_SUPPORTED
        | WBEM_E_NOT_FOUND
        | WBEM_E_INVALID_PARAMETER
        | WBEM_E_INVALID_METHOD_PARAMETERS
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
