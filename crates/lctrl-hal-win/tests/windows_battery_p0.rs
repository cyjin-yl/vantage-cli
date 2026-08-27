use parking_lot::Mutex;

use lctrl_core::{AdapterAuthentication, ApplyMode, ChargeMode, ChargeModeActual, LctrlError};
use lctrl_hal::BatteryControl;
use lctrl_hal_win::{
    ChargeModeReader, IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IoctlTransport,
    WindowsBatteryP0,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Call {
    code: u32,
    input: Vec<u8>,
    output_len: usize,
}

struct FakeIoctl {
    replies: Mutex<Vec<Vec<u8>>>,
    calls: Mutex<Vec<Call>>,
}

impl FakeIoctl {
    fn new(replies: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut replies: Vec<_> = replies.into_iter().collect();
        replies.reverse();
        Self {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl IoctlTransport for FakeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> lctrl_core::Result<Vec<u8>> {
        self.calls.lock().push(Call {
            code,
            input: input.into(),
            output_len,
        });
        Ok(self.replies.lock().pop().unwrap_or_default())
    }
}

struct FakeReader {
    states: Mutex<Vec<u32>>,
}

impl FakeReader {
    fn new(states: impl IntoIterator<Item = u32>) -> Self {
        let mut states: Vec<_> = states.into_iter().collect();
        states.reverse();
        Self {
            states: Mutex::new(states),
        }
    }
}

impl ChargeModeReader for FakeReader {
    fn read_charge_mode_raw(&self) -> lctrl_core::Result<u32> {
        self.states
            .lock()
            .pop()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "test charge mode reader exhausted".into(),
            })
    }
}

fn telemetry(design_capacity_raw: u16) -> Vec<u8> {
    let mut data = vec![0; 83];
    data[0..2].copy_from_slice(&design_capacity_raw.to_le_bytes());
    data[18..20].copy_from_slice(&u16::MAX.to_le_bytes());
    data[20..22].copy_from_slice(&u16::MAX.to_le_bytes());
    data
}

#[test]
fn adapter_gates_gapd_on_gbmd_detail_bit() {
    let ioctl = FakeIoctl::new([0x0086_0004_u32.to_le_bytes().to_vec()]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([]));

    let info = p0.adapter_info().unwrap();
    assert_eq!(info.authentication, AdapterAuthentication::Inbox);
    assert!(!info.has_detail);
    assert!(info.detail.is_none());
    assert_eq!(
        &*p0.ioctl().calls.lock(),
        &[Call {
            code: IOCTL_GBMD,
            input: vec![0xff],
            output_len: 4,
        }]
    );
}

#[test]
fn adapter_queries_gapd_only_when_gbmd_advertises_it() {
    let ioctl = FakeIoctl::new([
        0x0100_8004_u32.to_le_bytes().to_vec(),
        vec![0x34, 0x12, 0x78, 0x56, 100, 0, 65, 0, 0, 0],
    ]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([]));

    let info = p0.adapter_info().unwrap();
    assert_eq!(info.authentication, AdapterAuthentication::Lenovo);
    assert!(info.has_detail);
    assert!(info.is_underpowered());
    assert_eq!(p0.ioctl().calls.lock()[1].code, IOCTL_GAPD);
}

#[test]
fn dry_run_reads_but_never_writes() {
    let ioctl = FakeIoctl::new([telemetry(5000)]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([0]));

    let report = p0
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::DryRun)
        .unwrap();
    assert_eq!(report.previous(), &ChargeMode::Normal);
    assert_eq!(report.requested(), &ChargeMode::Conservation);
    assert_eq!(report.actual(), None);
    assert!(p0.ioctl().calls.lock().is_empty());
}

#[test]
fn target_conservation_uses_only_safe_gen2_commands_and_reads_back() {
    let ioctl = FakeIoctl::new([0_u32.to_le_bytes().to_vec(), 0_u32.to_le_bytes().to_vec()]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([0, 1]));

    let report = p0
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&ChargeMode::Conservation));
    let calls = p0.ioctl().calls.lock();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].code, IOCTL_GBMD);
    assert_eq!(calls[0].input, vec![0x08]);
    assert_eq!(calls[1].input, vec![0x0d]);
    assert!(calls.iter().all(|call| call.input != vec![0x03]));
}

#[test]
fn target_rapid_rejects_39wh_before_any_write() {
    let ioctl = FakeIoctl::new([telemetry(3900)]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([0]));

    assert!(matches!(
        p0.set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
    assert_eq!(p0.ioctl().calls.lock()[0].code, IOCTL_BATTERY_DETAIL);
    assert_eq!(p0.ioctl().calls.lock().len(), 1);
}

#[test]
fn readback_mismatch_is_not_reported_as_success() {
    let ioctl = FakeIoctl::new([
        telemetry(5000),
        0_u32.to_le_bytes().to_vec(),
        0_u32.to_le_bytes().to_vec(),
    ]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([0, 0]));

    assert!(matches!(
        p0.set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit),
        Err(LctrlError::VerifyMismatch { .. })
    ));
}

#[test]
fn conflict_or_unknown_readback_blocks_write() {
    for raw in [3, 9, u32::MAX] {
        let ioctl = FakeIoctl::new([]);
        let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([raw]));
        assert!(
            p0.set_charge_mode(ChargeMode::Normal, ApplyMode::Commit)
                .is_err()
        );
        assert!(p0.ioctl().calls.lock().is_empty());
    }
}

#[test]
fn telemetry_uses_energy_driver_battery_detail() {
    let ioctl = FakeIoctl::new([telemetry(5000)]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([]));

    assert_eq!(
        p0.battery_telemetry(0).unwrap().design_capacity_mwh,
        Some(50_000)
    );
    assert_eq!(
        &*p0.ioctl().calls.lock(),
        &[Call {
            code: IOCTL_BATTERY_DETAIL,
            input: 0_u32.to_le_bytes().to_vec(),
            output_len: 83,
        }]
    );
}

#[test]
fn current_charge_mode_preserves_conflict() {
    let p0 = WindowsBatteryP0::new(FakeIoctl::new([]), FakeReader::new([3]));
    assert_eq!(p0.charge_mode().unwrap(), ChargeModeActual::Conflict);
}

#[test]
fn unverified_readback_source_blocks_all_charge_mode_writes() {
    let p0 = WindowsBatteryP0::new(
        FakeIoctl::new([]),
        lctrl_hal_win::UnverifiedChargeModeReader,
    );

    assert!(matches!(
        p0.set_charge_mode(ChargeMode::Normal, ApplyMode::Commit),
        Err(LctrlError::Unsupported { feature }) if feature == "battery.charge-mode.readback"
    ));
    assert!(p0.ioctl().calls.lock().is_empty());
}
