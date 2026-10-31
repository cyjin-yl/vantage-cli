//! MSR channel through the vendor-signed `LnvMSRIO` driver (`\\.\WinMsrDev`).
//!
//! Spec source: `docs/RE-interfaces.md` §3–§4 (IOCTL contract recovered from
//! `WinMSRIO.dll`). The device ACL admits `LocalSystem` only, so a normal
//! process receives `ERROR_ACCESS_DENIED` on open; run the caller as a
//! SYSTEM service or accept the honest `PermissionDenied`.
//!
//! Per §3.2 the driver zeroes its selector table unless
//! `NotifyPlatformInfo` whitelists the platform ID, so [`WinMsrDev::probe`]
//! performs the full documented handshake before any read.
//!
//! Write gating: raw selector writes are exposed for the acceptance-gated
//! tuning path; see `set_selector`. No field-level semantics are assumed
//! here — callers must read-modify-write against the machine baseline
//! (RE-interfaces.md §3.3) and never write lock bits.

use lctrl_core::{LctrlError, Result};

use crate::IoctlTransport;
#[cfg(windows)]
use crate::native_ioctl::DeviceIoTransport;

/// Native transport bound to [`WIN_MSR_DEVICE_PATH`]. Opening requires
/// `LocalSystem`; anything else fails with `ERROR_ACCESS_DENIED`.
#[cfg(windows)]
pub const WIN_MSR_TRANSPORT: DeviceIoTransport =
    DeviceIoTransport::new(WIN_MSR_DEVICE_PATH, "WinMsrDev");

/// Device interface opened by the vendor user-mode library.
pub const WIN_MSR_DEVICE_PATH: &str = r"\\.\WinMsrDev";
/// `RdmsrCollection`: in = selector u32, out = 8-byte `{low, high}`.
pub const IOCTL_MSR_READ: u32 = 0x9C40_21CC;
/// `WrmsrCollection`: in = 12-byte `{selector, low, high}`, out = status u32.
pub const IOCTL_MSR_WRITE: u32 = 0x9C40_21D0;
/// `NotifyPlatformInfo`: in = 8-byte `{platform_id, cookie}`, out = status u32.
pub const IOCTL_NOTIFY_PLATFORM: u32 = 0x9C40_21D4;

/// Platform IDs whose selector table the driver enables (RE §3.2, verified
/// by patching the driver whitelist as SYSTEM — read-only).
pub const PLATFORM_WHITELIST: [u32; 3] = [0x33, 0x35, 0x36];
/// Platform ID observed on the 21VG reference machine.
pub const PLATFORM_ID_21VG: u32 = 0x36;

/// Driver selector index mapped to its MSR (RE §3.1, hardcoded in the
/// driver). The `u32` payload is the selector, not the MSR address.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MsrSelector(u32);

impl MsrSelector {
    pub const THERM_STATUS: Self = Self(0);
    pub const MISC_ENABLE: Self = Self(1);
    pub const ENERGY_PERF_BIAS: Self = Self(2);
    pub const HWP_REQUEST_PKG: Self = Self(3);
    pub const HWP_REQUEST: Self = Self(4);
    pub const PLATFORM_POWER_LIMIT: Self = Self(5);

    /// Selector count exposed by the driver; table has exactly 6 entries.
    pub const COUNT: u32 = 6;

    #[must_use]
    pub const fn new(selector: u32) -> Self {
        Self(selector)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// MSR address this selector reads/writes (RE §3.1).
    #[must_use]
    pub const fn msr_address(self) -> u32 {
        match self {
            Self::THERM_STATUS => 0x19B,
            Self::MISC_ENABLE => 0x19C,
            Self::ENERGY_PERF_BIAS => 0x1B0,
            Self::HWP_REQUEST_PKG => 0x772,
            Self::HWP_REQUEST => 0x774,
            Self::PLATFORM_POWER_LIMIT => 0x65C,
            Self(_) => 0,
        }
    }

    /// All documented selectors in table order.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        [
            Self::THERM_STATUS,
            Self::MISC_ENABLE,
            Self::ENERGY_PERF_BIAS,
            Self::HWP_REQUEST_PKG,
            Self::HWP_REQUEST,
            Self::PLATFORM_POWER_LIMIT,
        ]
    }
}

/// `WinMsrDev` client over any [`IoctlTransport`].
pub struct WinMsrDev<'a, T: ?Sized> {
    transport: &'a T,
}

impl<'a, T> WinMsrDev<'a, T>
where
    T: IoctlTransport + ?Sized,
{
    #[must_use]
    pub const fn new(transport: &'a T) -> Self {
        Self { transport }
    }

    /// Documented handshake: enable the selector table for `platform_id`.
    /// A nonzero driver status is a firmware/platform rejection.
    pub fn notify_platform_info(&self, platform_id: u32, cookie: u32) -> Result<()> {
        let mut input = [0_u8; 8];
        input[..4].copy_from_slice(&platform_id.to_le_bytes());
        input[4..].copy_from_slice(&cookie.to_le_bytes());
        let output = self.transport.call(IOCTL_NOTIFY_PLATFORM, &input, 4)?;
        require_zero_status(&output, "NotifyPlatformInfo")
    }

    /// Read one selector. The driver reassembles `(high << 32) | low`.
    pub fn read_selector(&self, selector: MsrSelector) -> Result<u64> {
        if selector.raw() >= MsrSelector::COUNT {
            return Err(LctrlError::InvalidArgument {
                detail: format!("MSR selector {} is outside 0..6", selector.raw()),
            });
        }
        let output = self
            .transport
            .call(IOCTL_MSR_READ, &selector.raw().to_le_bytes(), 8)?;
        if output.len() != 8 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("WinMsrDev read returned {} bytes, expected 8", output.len()),
            });
        }
        let low = u32::from_le_bytes(output[..4].try_into().expect("len checked"));
        let high = u32::from_le_bytes(output[4..].try_into().expect("len checked"));
        Ok((u64::from(high) << 32) | u64::from(low))
    }

    /// Write one selector (raw 64-bit value, acceptance-gated at call sites).
    pub fn write_selector(&self, selector: MsrSelector, value: u64) -> Result<()> {
        if selector.raw() >= MsrSelector::COUNT {
            return Err(LctrlError::InvalidArgument {
                detail: format!("MSR selector {} is outside 0..6", selector.raw()),
            });
        }
        let mut input = [0_u8; 12];
        input[..4].copy_from_slice(&selector.raw().to_le_bytes());
        input[4..8].copy_from_slice(&(value as u32).to_le_bytes());
        input[8..].copy_from_slice(&((value >> 32) as u32).to_le_bytes());
        let output = self.transport.call(IOCTL_MSR_WRITE, &input, 4)?;
        require_zero_status(&output, "WrmsrCollection")
    }

    /// Availability probe: run the documented handshake for the 21VG
    /// platform ID, then read the always-present thermal status selector.
    /// Any failure (missing driver, non-SYSTEM caller, wrong platform)
    /// propagates as the honest error.
    pub fn probe(&self) -> Result<()> {
        self.notify_platform_info(PLATFORM_ID_21VG, 0)?;
        self.read_selector(MsrSelector::THERM_STATUS).map(|_| ())
    }
}

fn require_zero_status(output: &[u8], method: &str) -> Result<()> {
    // The vendor wrappers (RE-interfaces.md §4.2–4.3) return the
    // DeviceIoControl BOOL; the driver may leave the output buffer untouched
    // (bytes returned = 0). Treat an empty output as success, a 4-byte zero
    // status as success, and a nonzero status as rejection.
    if output.is_empty() {
        return Ok(());
    }
    if output.len() != 4 {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "WinMsrDev {method} returned {} bytes, expected 4",
                output.len()
            ),
        });
    }
    let status = u32::from_le_bytes(output[..4].try_into().expect("len checked"));
    if status == 0 {
        return Ok(());
    }
    Err(LctrlError::FirmwareRejected {
        detail: format!("WinMsrDev {method} returned driver status {status}"),
    })
}
