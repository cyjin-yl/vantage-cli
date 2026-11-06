//! Read-only USB port inventory.

use lctrl_core::{Result, UsbPortInventory};

/// Read-only inventory of USB controllers, hubs and ports.
///
/// The trait has no write method on purpose: `usb ports` reports what the
/// platform returns and never changes a port state. Per-port power delivery is
/// part of the inventory only when a channel really reports it.
pub trait UsbPortInventoryControl: Send + Sync {
    fn usb_port_inventory(&self) -> Result<UsbPortInventory>;
}
