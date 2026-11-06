//! Decoder tests for the USB hub buffers `usb ports` reads.
//!
//! Every byte sequence below was captured from the 21VG target on 2026-11-05
//! (`docs/evidence/re-usb-ports-20261105.md` §4): the Goodix fingerprint sensor,
//! the integrated camera and the wireless receiver on root hub
//! `USB\ROOT_HUB30\4&196F3462&0&0`, the two hub descriptors, and an all-zero
//! buffer for a port with no device. The tests therefore pin the packed
//! `usbioctl.h` offsets to real driver output instead of to a guess.

use lctrl_core::{UsbPortConnection, UsbPortSpeed};
use lctrl_hal_win::{
    decode_config_descriptor, decode_hub_node_information, decode_port_connection_information,
    format_usb_revision, max_power_ma,
};

fn bytes(hex: &str) -> Vec<u8> {
    hex.split_whitespace()
        .map(|token| u8::from_str_radix(token, 16).expect("hex byte"))
        .collect()
}

/// `IOCTL_USB_GET_NODE_INFORMATION`, 5-port root hub `4&32F967F1`.
const HUB_5_PORTS: &str =
    "00 00 00 00 09 29 05 00 00 02 00 00 ff 00";

/// `IOCTL_USB_GET_NODE_INFORMATION`, 12-port root hub `4&196F3462`.
const HUB_12_PORTS: &str =
    "00 00 00 00 09 29 0c 00 00 02 00 00 00 ff ff";

/// Port 4: Goodix `27c6:659a`, full speed, address 1, two open pipes.
const PORT_4_GOODIX: &str = "04 00 00 00 12 01 00 02 ef 00 00 40 c6 27 9a 65 00 01 01 02 03 \
     01 01 01 00 01 00 02 00 00 00 01 00 00 00 07 05 83 02 40 00 00 00 00 00 00 07 05";

/// Port 5: integrated camera `174f:182b`, high speed, address 2, three pipes.
const PORT_5_CAMERA: &str = "05 00 00 00 12 01 01 02 ef 02 01 40 4f 17 2b 18 11 10 03 01 02 \
     01 01 02 00 02 00 02 00 00 00 01 00 00 00 07 05 83 03 20 00 06 00 00 00 00 07 05";

/// Port 8: Logitech receiver `046d:c542`, full speed, address 3, one pipe.
const PORT_8_RECEIVER: &str = "08 00 00 00 12 01 10 01 00 00 00 08 6d 04 42 c5 08 03 01 02 00 \
     01 01 01 00 03 00 01 00 00 00 01 00 00 00 07 05 82 03 08 00 04 00 00 00 00 00 00";

/// Goodix active configuration: bus powered, remote wakeup, `bMaxPower` 50.
const GOODIX_CONFIG: &str =
    "04 00 00 00 80 06 00 02 00 00 ff 00 09 02 20 00 01 01 03 a0 32";

/// Camera active configuration: bus powered, `bMaxPower` 250.
const CAMERA_CONFIG: &str =
    "05 00 00 00 80 06 00 02 00 00 ff 00 09 02 9d 04 05 01 04 80 fa";

/// Receiver active configuration: remote wakeup, `bMaxPower` 25.
const RECEIVER_CONFIG: &str =
    "08 00 00 00 80 06 00 02 00 00 ff 00 09 02 22 00 01 01 00 a0 19";

#[test]
fn hub_descriptors_yield_the_reported_port_counts() {
    let five = decode_hub_node_information(&bytes(HUB_5_PORTS)).expect("5-port hub decodes");
    assert_eq!(five.port_count, 5);
    assert!(!five.bus_powered, "root hubs are self-powered");
    assert_eq!(five.characteristics, 0x0000);

    let twelve = decode_hub_node_information(&bytes(HUB_12_PORTS)).expect("12-port hub decodes");
    assert_eq!(twelve.port_count, 12);
    assert!(!twelve.bus_powered);
    assert_eq!(twelve.characteristics, 0x0000);
}

#[test]
fn goodix_port_decodes_descriptor_and_full_speed() {
    let port = decode_port_connection_information(&bytes(PORT_4_GOODIX)).expect("port decodes");
    assert_eq!(port.connection, UsbPortConnection::Connected);
    assert_eq!(port.address, 1);
    assert_eq!(port.speed, Some(UsbPortSpeed::Full));
    let descriptor = port.descriptor.expect("connected port has a descriptor");
    assert_eq!(descriptor.vendor_id, 0x27c6);
    assert_eq!(descriptor.product_id, 0x659a);
    assert_eq!(descriptor.bcd_usb, 0x0200);
    assert!(!descriptor.device_is_hub);
}

#[test]
fn camera_port_decodes_high_speed() {
    let port = decode_port_connection_information(&bytes(PORT_5_CAMERA)).expect("port decodes");
    assert_eq!(port.connection, UsbPortConnection::Connected);
    assert_eq!(port.address, 2);
    assert_eq!(port.speed, Some(UsbPortSpeed::High));
    let descriptor = port.descriptor.expect("descriptor");
    assert_eq!(descriptor.vendor_id, 0x174f);
    assert_eq!(descriptor.product_id, 0x182b);
    assert_eq!(descriptor.bcd_usb, 0x0201);
}

#[test]
fn receiver_port_decodes_usb_1_1_full_speed() {
    let port = decode_port_connection_information(&bytes(PORT_8_RECEIVER)).expect("port decodes");
    assert_eq!(port.connection, UsbPortConnection::Connected);
    assert_eq!(port.address, 3);
    assert_eq!(port.speed, Some(UsbPortSpeed::Full));
    let descriptor = port.descriptor.expect("descriptor");
    assert_eq!(descriptor.vendor_id, 0x046d);
    assert_eq!(descriptor.product_id, 0xc542);
    assert_eq!(format_usb_revision(descriptor.bcd_usb), "1.10");
}

#[test]
fn empty_port_is_empty_without_speed_or_descriptor() {
    let port = decode_port_connection_information(&[0u8; 35]).expect("empty port decodes");
    assert_eq!(port.connection, UsbPortConnection::Empty);
    assert_eq!(port.speed, None);
    assert!(port.descriptor.is_none(), "no invented device on an empty port");
}

#[test]
fn out_of_range_speed_keeps_the_raw_driver_value() {
    let mut buffer = bytes(PORT_4_GOODIX);
    buffer[23] = 9;
    let port = decode_port_connection_information(&buffer).expect("port decodes");
    assert_eq!(port.speed, Some(UsbPortSpeed::Unknown(9)));
    assert_eq!(
        port.speed.expect("speed").to_string(),
        "unknown (raw 9)",
        "an unknown grade prints raw instead of the nearest known grade"
    );
}

#[test]
fn truncated_or_mislabelled_buffers_are_rejected() {
    assert!(decode_port_connection_information(&[0u8; 34]).is_none());
    assert!(decode_hub_node_information(&[0u8; 75]).is_none());

    let mut wrong_descriptor = bytes(PORT_4_GOODIX);
    wrong_descriptor[5] = 0x02;
    assert!(decode_port_connection_information(&wrong_descriptor).is_none());

    let mut wrong_hub_type = bytes(HUB_5_PORTS);
    wrong_hub_type[5] = 0x2a;
    assert!(decode_hub_node_information(&wrong_hub_type).is_none());
}

#[test]
fn configuration_descriptors_convert_bmax_power_in_the_right_unit() {
    let goodix = decode_config_descriptor(&bytes(GOODIX_CONFIG)).expect("config decodes");
    assert_eq!(goodix.attributes, 0xa0);
    assert_eq!(max_power_ma(goodix.max_power_raw, 0x0200), 100);

    let camera = decode_config_descriptor(&bytes(CAMERA_CONFIG)).expect("config decodes");
    assert_eq!(camera.attributes, 0x80);
    assert_eq!(max_power_ma(camera.max_power_raw, 0x0201), 500);

    let receiver = decode_config_descriptor(&bytes(RECEIVER_CONFIG)).expect("config decodes");
    assert_eq!(receiver.attributes, 0xa0);
    assert_eq!(max_power_ma(receiver.max_power_raw, 0x0110), 50);

    // USB 3.2 §9.6.3: 8 mA units from bcdUSB 3.0 upwards.
    assert_eq!(max_power_ma(0x19, 0x0300), 200);
}

#[test]
fn usb_revision_renders_the_bcd_field() {
    assert_eq!(format_usb_revision(0x0110), "1.10");
    assert_eq!(format_usb_revision(0x0200), "2.00");
    assert_eq!(format_usb_revision(0x0201), "2.01");
    assert_eq!(format_usb_revision(0x0320), "3.20");
}
