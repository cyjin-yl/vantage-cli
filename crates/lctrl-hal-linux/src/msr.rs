//! Native model-specific register channel via `/dev/cpu/N/msr`.
//!
//! Spec source: `docs/interfaces.md` §3.1 (selector→MSR map recovered from
//! the vendor driver) and Intel SDM. The Linux channel addresses the same
//! registers directly by MSR address; no vendor component is involved.
//!
//! Safety contract: callers doing read-modify-write must preserve reserved
//! bits and never write lock bits whole-word (`MISC_ENABLE` bit 38 covers
//! Turbo). This module performs exactly one 64-bit `pread`/`pwrite` per call.

use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
};

use lctrl_core::{LctrlError, Result};

const MSR_LEN: u64 = 8;

/// Model-specific register address (Intel SDM).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MsrAddress(u32);

impl MsrAddress {
    /// `IA32_THERM_STATUS` — status register, writes only for confirmed bits.
    pub const THERM_STATUS: Self = Self(0x19B);
    /// `IA32_MISC_ENABLE` — bit 38 disables Turbo; never write whole-word.
    pub const MISC_ENABLE: Self = Self(0x19C);
    /// `IA32_ENERGY_PERF_BIAS` — bits 3:0 hold EPP, safe read-modify-write.
    pub const ENERGY_PERF_BIAS: Self = Self(0x1B0);
    /// `MSR_HWP_REQUEST_PKG` — package-level HWP request.
    pub const HWP_REQUEST_PKG: Self = Self(0x772);
    /// `MSR_HWP_REQUEST` — per-core HWP request.
    pub const HWP_REQUEST: Self = Self(0x774);
    /// `MSR_PLATFORM_POWER_LIMIT` — platform PL1/PL2 window.
    pub const PLATFORM_POWER_LIMIT: Self = Self(0x65C);

    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Raw MSR read/write transport. One 64-bit access per call, no caching.
pub trait MsrTransport: Send + Sync {
    fn read(&self, address: MsrAddress) -> Result<u64>;
    fn write(&self, address: MsrAddress, value: u64) -> Result<()>;
}

/// `/dev/cpu/N/msr`-backed transport.
///
/// In tests the path may point at a regular file created inside a temporary
/// filesystem tree; `pread`/`pwrite` behave identically there.
pub struct DevMsr {
    path: PathBuf,
    cpu: u32,
}

impl DevMsr {
    /// Bind to one CPU's MSR node.
    #[must_use]
    pub fn new(path: PathBuf, cpu: u32) -> Self {
        Self { path, cpu }
    }

    /// CPU index this transport is bound to.
    #[must_use]
    pub const fn cpu(&self) -> u32 {
        self.cpu
    }

    /// Read-only availability probe: open the node for reading. Side-effect
    /// free with respect to hardware.
    pub fn probe(&self) -> Result<()> {
        self.open(false).map(|_| ())
    }

    fn open(&self, writable: bool) -> Result<File> {
        OpenOptions::new()
            .read(true)
            .write(writable)
            .open(&self.path)
            .map_err(|error| map_msr_error(&error, &self.path, self.cpu))
    }
}

impl MsrTransport for DevMsr {
    fn read(&self, address: MsrAddress) -> Result<u64> {
        let file = self.open(false)?;
        let mut buffer = [0_u8; MSR_LEN as usize];
        file.read_exact_at(&mut buffer, u64::from(address.raw()))
            .map_err(|error| map_msr_error(&error, &self.path, self.cpu))?;
        Ok(u64::from_ne_bytes(buffer))
    }

    fn write(&self, address: MsrAddress, value: u64) -> Result<()> {
        let file = self.open(true)?;
        file.write_all_at(&value.to_ne_bytes(), u64::from(address.raw()))
            .map_err(|error| map_msr_error(&error, &self.path, self.cpu))?;
        Ok(())
    }
}

fn map_msr_error(error: &io::Error, path: &Path, cpu: u32) -> LctrlError {
    match error.kind() {
        io::ErrorKind::NotFound => LctrlError::Unsupported {
            feature: format!("msr.cpu{cpu}"),
        },
        io::ErrorKind::PermissionDenied => LctrlError::PermissionDenied {
            need: format!("root or msr-group access to {}", path.display()),
        },
        _ => LctrlError::Io(io::Error::other(format!(
            "{} (cpu {cpu}): {error}",
            path.display()
        ))),
    }
}
