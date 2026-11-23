//! The machine's present device hardware IDs, read from sysfs.
//!
//! Only the standard sysfs device trees are used: PCI (`vendor`/`device`)
//! and USB (`idVendor`/`idProduct`). The IDs are rendered in the Windows PnP
//! spelling (`PCI\VEN_8086&DEV_1234`, `USB\VID_1D6B&PID_0002`) so a catalog
//! package's declared binding can be compared against them directly.

use std::path::{Path, PathBuf};

use lenovo_laptop_control_core::Result;
use lenovo_laptop_control_hal::HardwareInventoryControl;

const PCI_DEVICES: &str = "/sys/bus/pci/devices";
const USB_DEVICES: &str = "/sys/bus/usb/devices";

/// Reads the host's device tree directly; the roots are injectable so the
/// mapping is testable without a real machine.
#[derive(Clone, Debug)]
pub struct LinuxHardwareInventory {
    pci_root: PathBuf,
    usb_root: PathBuf,
}

impl Default for LinuxHardwareInventory {
    fn default() -> Self {
        Self {
            pci_root: PathBuf::from(PCI_DEVICES),
            usb_root: PathBuf::from(USB_DEVICES),
        }
    }
}

impl LinuxHardwareInventory {
    #[must_use]
    pub fn with_roots(pci_root: PathBuf, usb_root: PathBuf) -> Self {
        Self { pci_root, usb_root }
    }

    fn ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        ids.extend(pci_ids(&self.pci_root)?);
        ids.extend(usb_ids(&self.usb_root)?);
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

impl HardwareInventoryControl for LinuxHardwareInventory {
    fn present_hardware_ids(&self) -> Result<Vec<String>> {
        self.ids()
    }
}

fn pci_ids(root: &Path) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(ids);
    };
    for entry in entries.flatten() {
        let vendor = read_hex(&entry.path().join("vendor"));
        let device = read_hex(&entry.path().join("device"));
        if let (Some(vendor), Some(device)) = (vendor, device) {
            ids.push(format!("PCI\\VEN_{vendor}&DEV_{device}"));
        }
    }
    Ok(ids)
}

fn usb_ids(root: &Path) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Ok(ids);
    };
    for entry in entries.flatten() {
        let vendor = read_hex(&entry.path().join("idVendor"));
        let product = read_hex(&entry.path().join("idProduct"));
        if let (Some(vendor), Some(product)) = (vendor, product) {
            ids.push(format!("USB\\VID_{vendor}&PID_{product}"));
        }
    }
    Ok(ids)
}

/// A sysfs `0x8086`-style value, uppercase and without the prefix.
fn read_hex(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let digits = text
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(digits.to_ascii_uppercase())
}
