//! Read-only USB port inventory: controllers, hubs, ports and the power
//! capability the platform actually reports for a port.
//!
//! Every field here is copied from a live channel. A field stays `None` when no
//! channel on the running platform returns it — in particular per-port power
//! delivery, which the Windows channel on the 21VG cannot read (see
//! `docs/evidence/re-usb-ports-20261105.md`).

use serde::Serialize;

/// Read-only port tree of every USB controller that exposes ports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbPortInventory {
    pub controllers: Vec<UsbController>,
    pub power_delivery: PowerDeliveryReport,
}

/// One USB host controller (xHCI) or USB4 host router.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbController {
    /// PnP instance id, e.g. `PCI\VEN_8086&DEV_E47D...`.
    pub instance_id: String,
    /// Device description as the platform reports it.
    pub description: String,
    /// Driver service name, when the platform reports one.
    pub driver: Option<String>,
    pub kind: UsbControllerKind,
    /// Hubs whose ports this controller exposes. Empty when the platform
    /// offers no user-mode port enumeration for it.
    pub hubs: Vec<UsbHub>,
    /// Why this controller's port list is empty or incomplete: the measured
    /// channel error for a hub that could not be read, or the documented reason
    /// the platform exposes no port enumeration for this controller at all.
    /// `None` when every hub under it answered.
    pub ports_unavailable: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsbControllerKind {
    /// USB 3.x eXtensible Host Controller exposing root-hub ports.
    Xhci,
    /// USB4 host router; its port state lives in the USB4 device-router stack.
    Usb4HostRouter,
    /// Any other parent the hub stack reports (for example a USB4 device
    /// router under which a dock's USB 3 hub enumerates).
    Other,
}

impl UsbControllerKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Xhci => "xhci",
            Self::Usb4HostRouter => "usb4-host-router",
            Self::Other => "other",
        }
    }
}

/// One hub under a controller, with the ports its descriptor declares.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbHub {
    /// PnP instance id, e.g. `USB\ROOT_HUB30\4&196F3462&0&0`.
    pub instance_id: String,
    /// `bNumberOfPorts` from the hub descriptor.
    pub port_count: u8,
    /// `HubIsBusPowered` from the node-information union.
    pub bus_powered: bool,
    pub ports: Vec<UsbPort>,
}

/// One physical hub port.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbPort {
    /// Hub-relative port number (`ConnectionIndex`).
    pub number: u8,
    pub connection: UsbPortConnection,
    /// Negotiated speed, present only for a connected port. An out-of-range
    /// driver value is preserved as [`UsbPortSpeed::Unknown`] instead of being
    /// mapped onto the nearest known grade.
    pub speed: Option<UsbPortSpeed>,
    /// Device identity read from the port, when a device is attached.
    pub device: Option<UsbPortDevice>,
    /// Port-level power-delivery report. `None` when no channel reports one;
    /// see [`PowerDeliveryReport::per_port_unavailable`].
    pub power_delivery: Option<UsbPortPowerDelivery>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsbPortConnection {
    /// `NoDeviceConnected`.
    Empty,
    /// `DeviceConnected`: the device enumerated and is addressable.
    Connected,
    /// Any other `USB_CONNECTION_STATUS` value, preserved raw. Failed
    /// enumeration and over-current states land here rather than being
    /// reported as an empty port.
    Other(u8),
}

impl UsbPortConnection {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Empty,
            1 => Self::Connected,
            other => Self::Other(other as u8),
        }
    }

    #[must_use]
    pub const fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }
}

/// Device as described by its own USB descriptors plus the PnP devnode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbPortDevice {
    /// PnP instance id of the device attached to the port, when it could be
    /// resolved exactly (hub port → child devnode).
    pub instance_id: Option<String>,
    /// Bus-reported device description, when the device supplies one.
    pub description: Option<String>,
    /// `idVendor` from the device descriptor.
    pub vendor_id: u16,
    /// `idProduct` from the device descriptor.
    pub product_id: u16,
    /// `bcdUSB` rendered as `<major>.<minor><sub-minor>`, e.g. `2.00`.
    pub usb_revision: String,
    /// `DeviceIsHub`: the device on this port is itself a hub, which is why the
    /// same instance id appears again as a hub further down the tree.
    pub is_hub: bool,
    /// `bMaxPower` of the active configuration, in mA, converted with the unit
    /// implied by `bcdUSB` (2 mA below USB 3.0, 8 mA at USB 3.0 and above).
    pub max_power_ma: Option<u32>,
}

/// Negotiated link speed as the hub driver reports it (`USB_DEVICE_SPEED`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsbPortSpeed {
    Low,
    Full,
    High,
    SuperSpeed,
    /// A driver value outside the documented grades, preserved raw.
    Unknown(u8),
}

impl UsbPortSpeed {
    /// Decode `USB_NODE_CONNECTION_INFORMATION_EX.Speed`. The enum is
    /// `UsbLowSpeed = 0`, `UsbFullSpeed = 1`, `UsbHighSpeed = 2`,
    /// `UsbSuperSpeed = 3`; every other value is kept as reported.
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Low,
            1 => Self::Full,
            2 => Self::High,
            3 => Self::SuperSpeed,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Full => 1,
            Self::High => 2,
            Self::SuperSpeed => 3,
            Self::Unknown(raw) => raw,
        }
    }
}

impl std::fmt::Display for UsbPortSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The signalling rates are the USB 2.0/3.x speed-grade rates, not a
        // measurement of this link.
        match self {
            Self::Low => f.write_str("low (1.5 Mb/s)"),
            Self::Full => f.write_str("full (12 Mb/s)"),
            Self::High => f.write_str("high (480 Mb/s)"),
            Self::SuperSpeed => f.write_str("super-speed (5 Gb/s)"),
            Self::Unknown(raw) => write!(f, "unknown (raw {raw})"),
        }
    }
}

/// A port's power contract, when a channel reports one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UsbPortPowerDelivery {
    pub protocol: UsbPowerProtocol,
    pub voltage_mv: Option<u32>,
    pub current_ma: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsbPowerProtocol {
    /// USB Battery Charging 1.2 charging port behaviour.
    Bc12,
    /// USB Power Delivery contract.
    UsbPd,
}

/// Machine-level power-delivery observation, independent of any single port.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PowerDeliveryReport {
    /// The platform's UCSI power-delivery controller, when the platform has
    /// one. Presence is a PnP fact; it does not imply readable connector state.
    pub pd_controller: Option<UsbPdController>,
    /// USB4 host-router instance ids. USB4 power delivery is negotiated by the
    /// router, not by an xHCI port.
    pub usb4_host_routers: Vec<String>,
    /// Measured reason no port reports a power contract. `None` when at least
    /// one port does.
    pub per_port_unavailable: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsbPdController {
    pub instance_id: String,
    pub driver: Option<String>,
    /// Firmware version reported by the platform, when it reports one.
    pub firmware_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{UsbPortConnection, UsbPortSpeed};

    #[test]
    fn speed_grades_decode_and_preserve_unknown_raw_values() {
        assert_eq!(UsbPortSpeed::from_raw(0), UsbPortSpeed::Low);
        assert_eq!(UsbPortSpeed::from_raw(1), UsbPortSpeed::Full);
        assert_eq!(UsbPortSpeed::from_raw(2), UsbPortSpeed::High);
        assert_eq!(UsbPortSpeed::from_raw(3), UsbPortSpeed::SuperSpeed);
        assert_eq!(UsbPortSpeed::from_raw(7), UsbPortSpeed::Unknown(7));
        assert_eq!(UsbPortSpeed::from_raw(7).raw(), 7);
        assert_eq!(
            UsbPortSpeed::from_raw(7).to_string(),
            "unknown (raw 7)",
            "an out-of-range grade is printed raw, never coerced"
        );
    }

    #[test]
    fn connection_states_keep_failure_codes_distinct_from_empty_ports() {
        assert_eq!(UsbPortConnection::from_raw(0), UsbPortConnection::Empty);
        assert_eq!(
            UsbPortConnection::from_raw(1),
            UsbPortConnection::Connected
        );
        assert_eq!(
            UsbPortConnection::from_raw(2),
            UsbPortConnection::Other(2),
            "failed enumeration is not an empty port"
        );
        assert!(!UsbPortConnection::from_raw(2).is_connected());
        assert!(UsbPortConnection::Connected.is_connected());
    }
}
