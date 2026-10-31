use std::sync::Mutex;

use lctrl_core::{LctrlError, Result};
use lctrl_hal_win::{
    IOCTL_MSR_READ, IOCTL_MSR_WRITE, IOCTL_NOTIFY_PLATFORM, MsrSelector, PLATFORM_ID_21VG,
    WinMsrDev,
};

#[derive(Default)]
struct FakeIoctl {
    calls: Mutex<Vec<(u32, Vec<u8>)>>,
    outputs: Mutex<Vec<Vec<u8>>>,
}

impl FakeIoctl {
    fn with_output(self, output: Vec<u8>) -> Self {
        self.outputs.lock().unwrap().push(output);
        self
    }
}

impl lctrl_hal_win::IoctlTransport for FakeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> Result<Vec<u8>> {
        self.calls
            .lock()
            .unwrap()
            .push((code, input.to_vec()));
        let mut outputs = self.outputs.lock().unwrap();
        assert!(!outputs.is_empty(), "test must queue an output for every call");
        let output = outputs.remove(0);
        assert!(
            output.len() <= output_len,
            "queued output exceeds requested length"
        );
        Ok(output)
    }
}

#[test]
fn read_uses_documented_ioctl_and_reassembles_u64() {
    let fake = FakeIoctl::default().with_output(vec![0x01, 0x00, 0xFF, 0x00, 0x19, 0x00, 0x00, 0x00]);
    let client = WinMsrDev::new(&fake);

    let value = client.read_selector(MsrSelector::HWP_REQUEST).unwrap();

    assert_eq!(value, (0x19 << 32) | 0x00FF_0001);
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].0, IOCTL_MSR_READ);
    assert_eq!(calls[0].1, 4u32.to_le_bytes());
}

#[test]
fn write_uses_documented_layout_and_rejects_nonzero_status() {
    // The vendor wrapper returns the DeviceIoControl BOOL; the driver may
    // leave the output buffer untouched (0 bytes = success).
    let ok = FakeIoctl::default().with_output(Vec::new());
    let client = WinMsrDev::new(&ok);
    client
        .write_selector(MsrSelector::ENERGY_PERF_BIAS, 0x1_0000_0007)
        .unwrap();
    let calls = ok.calls.lock().unwrap();
    assert_eq!(calls[0].0, IOCTL_MSR_WRITE);
    let mut expected = 2u32.to_le_bytes().to_vec();
    expected.extend_from_slice(&7u32.to_le_bytes());
    expected.extend_from_slice(&1u32.to_le_bytes());
    assert_eq!(calls[0].1, expected);

    let rejected = FakeIoctl::default().with_output(87u32.to_le_bytes().to_vec());
    let client = WinMsrDev::new(&rejected);
    let error = client
        .write_selector(MsrSelector::PLATFORM_POWER_LIMIT, 0)
        .unwrap_err();
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
}

#[test]
fn notify_platform_info_sends_id_and_cookie() {
    let fake = FakeIoctl::default().with_output(Vec::new());
    let client = WinMsrDev::new(&fake);

    client.notify_platform_info(PLATFORM_ID_21VG, 0).unwrap();

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls[0].0, IOCTL_NOTIFY_PLATFORM);
    let mut expected = PLATFORM_ID_21VG.to_le_bytes().to_vec();
    expected.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(calls[0].1, expected);
}

#[test]
fn probe_runs_handshake_then_safe_selector_read() {
    let fake = FakeIoctl::default()
        .with_output(Vec::new())
        .with_output(16u64.to_le_bytes().to_vec());
    let client = WinMsrDev::new(&fake);

    client.probe().unwrap();

    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, IOCTL_NOTIFY_PLATFORM);
    assert_eq!(calls[1].0, IOCTL_MSR_READ);
    assert_eq!(calls[1].1, 0u32.to_le_bytes());
}

#[test]
fn probe_surfaces_rejection_when_platform_is_not_whitelisted() {
    let fake = FakeIoctl::default().with_output(87u32.to_le_bytes().to_vec());
    let client = WinMsrDev::new(&fake);

    let error = client.probe().unwrap_err();
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
}

#[test]
fn out_of_range_selector_is_rejected_without_a_transport_call() {
    let fake = FakeIoctl::default();
    let client = WinMsrDev::new(&fake);

    assert!(client.read_selector(MsrSelector::new(6)).is_err());
    assert!(client.write_selector(MsrSelector::new(9), 0).is_err());
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[test]
fn selector_table_matches_re_documented_msr_addresses() {
    assert_eq!(MsrSelector::THERM_STATUS.msr_address(), 0x19B);
    assert_eq!(MsrSelector::MISC_ENABLE.msr_address(), 0x19C);
    assert_eq!(MsrSelector::ENERGY_PERF_BIAS.msr_address(), 0x1B0);
    assert_eq!(MsrSelector::HWP_REQUEST_PKG.msr_address(), 0x772);
    assert_eq!(MsrSelector::HWP_REQUEST.msr_address(), 0x774);
    assert_eq!(MsrSelector::PLATFORM_POWER_LIMIT.msr_address(), 0x65C);
    assert_eq!(MsrSelector::all().len() as u32, MsrSelector::COUNT);
}
