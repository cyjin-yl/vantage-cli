use parking_lot::Mutex;

use lctrl_core::LctrlError;
use lctrl_hal_win::{
    EnergyDriver, GbmdCommand, IOCTL_BATTERY_CONFIG, IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD,
    IOCTL_GENERIC_GET, IOCTL_GENERIC_SET, IoctlTransport,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Call {
    code: u32,
    input: Vec<u8>,
    output_len: usize,
}

#[derive(Default)]
struct FakeIoctl {
    calls: Mutex<Vec<Call>>,
    replies: Mutex<Vec<Vec<u8>>>,
}

impl FakeIoctl {
    fn with_replies(replies: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut replies: Vec<_> = replies.into_iter().collect();
        replies.reverse();
        Self {
            calls: Mutex::new(Vec::new()),
            replies: Mutex::new(replies),
        }
    }
}

impl IoctlTransport for FakeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> lctrl_core::Result<Vec<u8>> {
        self.calls.lock().push(Call {
            code,
            input: input.to_vec(),
            output_len,
        });
        Ok(self.replies.lock().pop().unwrap_or_default())
    }
}

#[test]
fn gbmd_status_uses_one_byte_query_and_four_byte_reply() {
    let transport = FakeIoctl::with_replies([[0x04, 0x00, 0x86, 0x00].to_vec()]);
    let driver = EnergyDriver::new(&transport);

    assert_eq!(driver.gbmd_status().unwrap(), 0x0086_0004);
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_GBMD,
            input: vec![0xff],
            output_len: 4,
        }]
    );
}

#[test]
fn gbmd_write_requires_zero_firmware_status() {
    let transport = FakeIoctl::with_replies([0u32.to_le_bytes().to_vec()]);
    let driver = EnergyDriver::new(&transport);

    driver.write_gbmd(GbmdCommand::RAPID_ON).unwrap();
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_GBMD,
            input: vec![0x07],
            output_len: 4,
        }]
    );

    let rejected = FakeIoctl::with_replies([9u32.to_le_bytes().to_vec()]);
    assert!(matches!(
        EnergyDriver::new(&rejected).write_gbmd(GbmdCommand::RAPID_ON),
        Err(LctrlError::FirmwareRejected { .. })
    ));
}

#[test]
fn generic_get_uses_literal_get_ioctl() {
    let transport = FakeIoctl::with_replies([16u32.to_le_bytes().to_vec()]);
    let driver = EnergyDriver::new(&transport);

    assert_eq!(driver.generic_get(14).unwrap(), 16);
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_GENERIC_GET,
            input: 14u32.to_le_bytes().to_vec(),
            output_len: 4,
        }]
    );
}

#[test]
fn generic_set_uses_twelve_byte_payload_and_no_output() {
    let transport = FakeIoctl::default();
    let driver = EnergyDriver::new(&transport);

    driver.generic_set(6, 1, 0x1234_5678).unwrap();
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_GENERIC_SET,
            input: vec![6, 0, 0, 0, 1, 0, 0, 0, 0x78, 0x56, 0x34, 0x12,],
            output_len: 0,
        }]
    );
}

#[test]
fn battery_detail_uses_index_and_exact_83_byte_reply() {
    let transport = FakeIoctl::with_replies([vec![0; 83]]);
    let driver = EnergyDriver::new(&transport);

    assert_eq!(driver.battery_detail(1).unwrap().as_bytes(), &[0; 83]);
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_BATTERY_DETAIL,
            input: 1u32.to_le_bytes().to_vec(),
            output_len: 83,
        }]
    );
}

#[test]
fn adapter_detail_uses_four_zero_bytes_and_ten_byte_reply() {
    let transport = FakeIoctl::with_replies([vec![0; 10]]);
    let driver = EnergyDriver::new(&transport);

    driver.adapter_detail().unwrap();
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_GAPD,
            input: vec![0; 4],
            output_len: 10,
        }]
    );
}

#[test]
fn battery_config_preserves_exact_twenty_byte_reply() {
    let reply: Vec<u8> = (0..20).collect();
    let transport = FakeIoctl::with_replies([reply.clone()]);
    let driver = EnergyDriver::new(&transport);

    assert_eq!(driver.battery_config().unwrap(), reply.as_slice());
    assert_eq!(
        &*transport.calls.lock(),
        &[Call {
            code: IOCTL_BATTERY_CONFIG,
            input: vec![0; 4],
            output_len: 20,
        }]
    );
}

#[test]
fn malformed_fixed_reply_is_rejected() {
    let transport = FakeIoctl::with_replies([vec![0; 19]]);

    assert!(matches!(
        EnergyDriver::new(&transport).battery_config(),
        Err(LctrlError::FirmwareRejected { .. })
    ));
}
