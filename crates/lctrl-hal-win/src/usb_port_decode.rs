//! Pure decoding of the buffers the Windows USB hub driver returns.
//!
//! These helpers have no platform types, so the Linux CI job that builds this
//! crate can exercise them; `usb ports` feeds them the live buffers. The
//! offsets are the packed `usbioctl.h` layouts, cross-checked against the byte
//! counts the driver actually returned on the target (see
//! `docs/evidence/re-usb-ports-20261105.md` §4):
//!
//! * `USB_NODE_INFORMATION` → 76 bytes for both root hubs
//!   (`4 + sizeof(USB_HUB_INFORMATION)`).
//! * `USB_NODE_CONNECTION_INFORMATION_EX` → 35 bytes for an empty port and
//!   `35 + 11 · NumberOfOpenPipes` for an occupied one.
//! * `USB_DESCRIPTOR_REQUEST` + configuration descriptor → `12 + wTotalLength`.
//!
//! A buffer that does not match its documented shape decodes to `None`; no
//! field is guessed from a neighbouring offset.

use lctrl_core::{UsbPortConnection, UsbPortSpeed};

/// Bytes a valid `USB_NODE_INFORMATION` response must contain.
pub const HUB_NODE_INFORMATION_BYTES: usize = 76;
/// Fixed part of `USB_NODE_CONNECTION_INFORMATION_EX`, before `PipeList`.
pub const CONNECTION_INFORMATION_EX_FIXED_BYTES: usize = 35;
/// `USB_DESCRIPTOR_REQUEST` header (`ULONG` + packed 8-byte setup packet).
pub const DESCRIPTOR_REQUEST_HEADER_BYTES: usize = 12;

const HUB_NODE_TYPE_USB_HUB: u32 = 0;
const HUB_DESCRIPTOR_LENGTH: u8 = 9;
const HUB_DESCRIPTOR_TYPE: u8 = 0x29;
const DEVICE_DESCRIPTOR_LENGTH: u8 = 18;
const DEVICE_DESCRIPTOR_TYPE: u8 = 1;
const CONFIG_DESCRIPTOR_LENGTH: u8 = 9;
const CONFIG_DESCRIPTOR_TYPE: u8 = 2;

const HUB_DESCRIPTOR_OFFSET: usize = 4;
const HUB_BUS_POWERED_OFFSET: usize = 75;
const DEVICE_DESCRIPTOR_OFFSET: usize = 4;
const PORT_SPEED_OFFSET: usize = 23;
const PORT_DEVICE_IS_HUB_OFFSET: usize = 24;
const PORT_ADDRESS_OFFSET: usize = 25;
const PORT_CONNECTION_STATUS_OFFSET: usize = 31;
const CONFIG_DESCRIPTOR_OFFSET: usize = DESCRIPTOR_REQUEST_HEADER_BYTES;

/// Facts the hub descriptor and node union carry for the whole hub.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HubNodeFacts {
    /// `bNumberOfPorts`.
    pub port_count: u8,
    /// `HubIsBusPowered`.
    pub bus_powered: bool,
    /// `wHubCharacteristics` (USB 2.0 §11.11.2.1 power-switching and
    /// over-current bits).
    pub characteristics: u16,
}

/// Facts for one port from `USB_NODE_CONNECTION_INFORMATION_EX`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortConnectionFacts {
    pub connection: UsbPortConnection,
    /// `DeviceAddress` as the hub reports it (0 when no device).
    pub address: u16,
    /// Negotiated speed, only for a connected port.
    pub speed: Option<UsbPortSpeed>,
    /// Device descriptor fields, only for a connected port.
    pub descriptor: Option<PortDeviceDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortDeviceDescriptor {
    pub bcd_usb: u16,
    /// `DeviceIsHub`: the device on this port is itself a hub.
    pub device_is_hub: bool,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// The active configuration's power declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceConfigurationFacts {
    /// `bmAttributes`.
    pub attributes: u8,
    /// `bMaxPower` before unit conversion.
    pub max_power_raw: u8,
}

#[must_use]
pub fn decode_hub_node_information(buffer: &[u8]) -> Option<HubNodeFacts> {
    if buffer.len() < HUB_NODE_INFORMATION_BYTES {
        return None;
    }
    if read_u32(buffer, 0)? != HUB_NODE_TYPE_USB_HUB {
        return None;
    }
    if buffer[HUB_DESCRIPTOR_OFFSET] != HUB_DESCRIPTOR_LENGTH
        || buffer[HUB_DESCRIPTOR_OFFSET + 1] != HUB_DESCRIPTOR_TYPE
    {
        return None;
    }
    let port_count = buffer[HUB_DESCRIPTOR_OFFSET + 2];
    if port_count == 0 {
        return None;
    }
    Some(HubNodeFacts {
        port_count,
        bus_powered: buffer[HUB_BUS_POWERED_OFFSET] != 0,
        characteristics: u16::from_le_bytes([
            buffer[HUB_DESCRIPTOR_OFFSET + 3],
            buffer[HUB_DESCRIPTOR_OFFSET + 4],
        ]),
    })
}

#[must_use]
pub fn decode_port_connection_information(buffer: &[u8]) -> Option<PortConnectionFacts> {
    if buffer.len() < CONNECTION_INFORMATION_EX_FIXED_BYTES {
        return None;
    }
    let connection = UsbPortConnection::from_raw(read_u32(buffer, PORT_CONNECTION_STATUS_OFFSET)?);
    let address = u16::from_le_bytes([
        buffer[PORT_ADDRESS_OFFSET],
        buffer[PORT_ADDRESS_OFFSET + 1],
    ]);
    if !connection.is_connected() {
        // The driver leaves the descriptor area zeroed for every non-connected
        // state; reporting a speed or a vendor id there would be invention.
        return Some(PortConnectionFacts {
            connection,
            address,
            speed: None,
            descriptor: None,
        });
    }
    if buffer[DEVICE_DESCRIPTOR_OFFSET] != DEVICE_DESCRIPTOR_LENGTH
        || buffer[DEVICE_DESCRIPTOR_OFFSET + 1] != DEVICE_DESCRIPTOR_TYPE
    {
        return None;
    }
    let descriptor = PortDeviceDescriptor {
        bcd_usb: read_u16(buffer, DEVICE_DESCRIPTOR_OFFSET + 2)?,
        device_is_hub: buffer[PORT_DEVICE_IS_HUB_OFFSET] != 0,
        vendor_id: read_u16(buffer, DEVICE_DESCRIPTOR_OFFSET + 8)?,
        product_id: read_u16(buffer, DEVICE_DESCRIPTOR_OFFSET + 10)?,
    };
    Some(PortConnectionFacts {
        connection,
        address,
        speed: Some(UsbPortSpeed::from_raw(buffer[PORT_SPEED_OFFSET])),
        descriptor: Some(descriptor),
    })
}

#[must_use]
pub fn decode_config_descriptor(buffer: &[u8]) -> Option<DeviceConfigurationFacts> {
    let descriptor = buffer.get(CONFIG_DESCRIPTOR_OFFSET..)?;
    if descriptor.len() < usize::from(CONFIG_DESCRIPTOR_LENGTH)
        || descriptor[0] != CONFIG_DESCRIPTOR_LENGTH
        || descriptor[1] != CONFIG_DESCRIPTOR_TYPE
    {
        return None;
    }
    Some(DeviceConfigurationFacts {
        attributes: descriptor[7],
        max_power_raw: descriptor[8],
    })
}

/// `bMaxPower` in milliamps. USB 3.2 §9.6.3 changes the unit from 2 mA to 8 mA
/// for devices whose `bcdUSB` is 3.0 or higher.
#[must_use]
pub const fn max_power_ma(raw: u8, bcd_usb: u16) -> u32 {
    let unit = if bcd_usb >= 0x0300 { 8 } else { 2 };
    raw as u32 * unit
}

/// Render `bcdUSB` the way the USB-IF writes it (`0x0201` → `2.01`).
#[must_use]
pub fn format_usb_revision(bcd_usb: u16) -> String {
    format!("{}.{:02}", bcd_usb >> 8, bcd_usb & 0xff)
}

fn read_u16(buffer: &[u8], offset: usize) -> Option<u16> {
    let bytes = buffer.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(buffer: &[u8], offset: usize) -> Option<u32> {
    let bytes = buffer.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
