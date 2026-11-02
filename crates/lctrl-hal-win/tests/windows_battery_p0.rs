use parking_lot::Mutex;

use lctrl_core::{
    AdapterAuthentication, AdapterStatus, ApplyMode, ChargeMode, ChargeModeActual, LctrlError,
};
use lctrl_hal::BatteryControl;
use lctrl_hal_win::{
    ChargeModeReader, GbmdChargeModeReader, IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD,
    IoctlTransport, WindowsBatteryP0,
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
    states: Mutex<Vec<ChargeModeActual>>,
}

impl FakeReader {
    fn new(states: impl IntoIterator<Item = ChargeModeActual>) -> Self {
        let mut states: Vec<_> = states.into_iter().collect();
        states.reverse();
        Self {
            states: Mutex::new(states),
        }
    }
}

impl ChargeModeReader for FakeReader {
    fn read_charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
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
    assert_eq!(info.is_underpowered(), Some(true));
    assert_eq!(p0.ioctl().calls.lock()[1].code, IOCTL_GAPD);
}

#[test]
fn adapter_sentinel_ids_are_reported_as_disconnected_not_65535() {
    let ioctl = FakeIoctl::new([
        0x0100_8004_u32.to_le_bytes().to_vec(),
        vec![0xff, 0xff, 0xff, 0xff, 100, 0, 0, 0, 0, 0],
    ]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([]));

    let info = p0.adapter_info().unwrap();

    assert_eq!(info.ac_connected, Some(false));
    assert_eq!(info.status, Some(AdapterStatus::Disconnected));
    assert_eq!(info.detail.unwrap().pid, None);
    assert_eq!(info.detail.unwrap().vid, None);
}

#[test]
fn dry_run_reads_but_never_writes() {
    let ioctl = FakeIoctl::new([telemetry(5000)]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([ChargeModeActual::Normal]));

    let report = p0
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::DryRun)
        .unwrap();
    assert_eq!(report.previous(), &ChargeMode::Normal);
    assert_eq!(report.requested(), &ChargeMode::Conservation);
    assert_eq!(report.actual(), None);
    assert!(p0.ioctl().calls.lock().is_empty());
}

#[test]
fn conservation_transition_and_restore_follow_device_subtype() {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    struct Device {
        state: Arc<AtomicU32>,
        subtype2: bool,
        pending: Mutex<Option<bool>>,
    }

    impl IoctlTransport for Device {
        fn call(&self, code: u32, input: &[u8], _: usize) -> lctrl_core::Result<Vec<u8>> {
            assert_eq!(code, IOCTL_GBMD);
            match input {
                [0xff] => return Ok(self.state.load(Ordering::SeqCst).to_le_bytes().to_vec()),
                [0x08] => {
                    self.state.fetch_and(!4, Ordering::SeqCst);
                }
                [0x07] => {
                    self.state.fetch_or(4, Ordering::SeqCst);
                }
                [0x03] => {
                    if self.subtype2 {
                        *self.pending.lock() = Some(true);
                    } else {
                        self.state.fetch_or(0x20, Ordering::SeqCst);
                    }
                }
                [0x05] => {
                    if self.subtype2 {
                        *self.pending.lock() = Some(false);
                    } else {
                        self.state.fetch_and(!0x20, Ordering::SeqCst);
                    }
                }
                [0x0d] if self.subtype2 && self.pending.lock().take() == Some(true) => {
                    self.state.fetch_or(0x20, Ordering::SeqCst);
                }
                [0x0f] if self.subtype2 && self.pending.lock().take() == Some(false) => {
                    self.state.fetch_and(!0x20, Ordering::SeqCst);
                }
                _ => return Err(std::io::Error::other("wrong selector for device subtype").into()),
            }
            Ok(Vec::new())
        }
    }

    struct Reader(Arc<AtomicU32>);
    impl ChargeModeReader for Reader {
        fn read_charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
            Ok(lctrl_core::decode_gbmd_charge_mode(
                self.0.load(Ordering::SeqCst),
            ))
        }
    }

    // Bit23 is deliberately independent of the subtype selection bit14.
    for (subtype2, bit23) in [(false, false), (false, true), (true, false), (true, true)] {
        let capability = (u32::from(subtype2) << 14) | (u32::from(bit23) << 23);
        let state = Arc::new(AtomicU32::new(capability | 4));
        let p0 = WindowsBatteryP0::new(
            Device {
                state: Arc::clone(&state),
                subtype2,
                pending: Mutex::new(None),
            },
            Reader(Arc::clone(&state)),
        );
        let report = p0
            .set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit)
            .unwrap();
        assert_eq!(report.previous(), &ChargeMode::Rapid);
        assert_eq!(report.actual(), Some(&ChargeMode::Conservation));
        assert_eq!(state.load(Ordering::SeqCst), capability | 0x20);
        let report = p0
            .set_charge_mode(ChargeMode::Normal, ApplyMode::Commit)
            .unwrap();
        assert_eq!(report.actual(), Some(&ChargeMode::Normal));
        assert_eq!(state.load(Ordering::SeqCst), capability);
    }
}

#[test]
fn target_rapid_rejects_39wh_before_any_write() {
    let ioctl = FakeIoctl::new([telemetry(3900)]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([ChargeModeActual::Normal]));

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
        0_u32.to_le_bytes().to_vec(),
        0_u32.to_le_bytes().to_vec(),
    ]);
    let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([ChargeModeActual::Normal; 12]));

    assert!(matches!(
        p0.set_charge_mode(ChargeMode::Rapid, ApplyMode::Commit),
        Err(LctrlError::VerifyMismatch { .. })
    ));
}

#[test]
fn conflict_or_unknown_readback_blocks_write() {
    for state in [
        ChargeModeActual::Conflict,
        ChargeModeActual::Unknown(9),
        ChargeModeActual::Unknown(u32::MAX),
    ] {
        let ioctl = FakeIoctl::new([]);
        let p0 = WindowsBatteryP0::new(ioctl, FakeReader::new([state]));
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
    let p0 = WindowsBatteryP0::new(
        FakeIoctl::new([]),
        FakeReader::new([ChargeModeActual::Conflict]),
    );
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

#[test]
fn first_write_failure_after_effect_restores_prior_mode() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    struct AppliedThenFailed {
        state: Arc<AtomicU32>,
        failed: AtomicBool,
    }

    impl IoctlTransport for AppliedThenFailed {
        fn call(&self, code: u32, input: &[u8], _: usize) -> lctrl_core::Result<Vec<u8>> {
            assert_eq!(code, IOCTL_GBMD);
            match input {
                [0xff] => return Ok(0_u32.to_le_bytes().to_vec()),
                [0x08] => self.state.fetch_and(!2, Ordering::SeqCst),
                [0x05] => self.state.fetch_and(!1, Ordering::SeqCst),
                [0x07] => self.state.fetch_or(2, Ordering::SeqCst),
                _ => panic!("unexpected charging command"),
            };
            if !self.failed.swap(true, Ordering::SeqCst) {
                return Err(std::io::Error::other("completion lost after device effect").into());
            }
            Ok(0_u32.to_le_bytes().to_vec())
        }
    }

    struct Reader(Arc<AtomicU32>);

    impl ChargeModeReader for Reader {
        fn read_charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
            Ok(match self.0.load(Ordering::SeqCst) {
                1 => ChargeModeActual::Conservation,
                2 => ChargeModeActual::Rapid,
                _ => ChargeModeActual::Normal,
            })
        }
    }

    let state = Arc::new(AtomicU32::new(2));
    let p0 = WindowsBatteryP0::new(
        AppliedThenFailed {
            state: Arc::clone(&state),
            failed: AtomicBool::new(false),
        },
        Reader(Arc::clone(&state)),
    );

    assert!(matches!(
        p0.set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit),
        Err(LctrlError::Io(_))
    ));
    assert_eq!(state.load(Ordering::SeqCst), 2);
    assert_eq!(p0.charge_mode().unwrap(), ChargeModeActual::Rapid);
}

#[test]
fn failed_recovery_keeps_both_errors_and_does_not_claim_restoration() {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    struct FailingIoctl(Arc<AtomicU32>);

    impl IoctlTransport for FailingIoctl {
        fn call(&self, code: u32, input: &[u8], _: usize) -> lctrl_core::Result<Vec<u8>> {
            assert_eq!(code, IOCTL_GBMD);
            match input {
                [0xff] => Ok(0_u32.to_le_bytes().to_vec()),
                [0x08] => {
                    self.0.fetch_and(!2, Ordering::SeqCst);
                    Err(std::io::Error::other("first dispatch lost").into())
                }
                [0x05] => Err(std::io::Error::other("restore unavailable").into()),
                _ => panic!("unexpected charging command"),
            }
        }
    }

    struct Reader(Arc<AtomicU32>);

    impl ChargeModeReader for Reader {
        fn read_charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
            Ok(match self.0.load(Ordering::SeqCst) {
                1 => ChargeModeActual::Conservation,
                2 => ChargeModeActual::Rapid,
                _ => ChargeModeActual::Normal,
            })
        }
    }

    let state = Arc::new(AtomicU32::new(2));
    let p0 = WindowsBatteryP0::new(FailingIoctl(Arc::clone(&state)), Reader(Arc::clone(&state)));
    let error = p0
        .set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit)
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("first dispatch lost"));
    assert!(message.contains("restore unavailable"));
    assert_eq!(state.load(Ordering::SeqCst), 0);
    assert_eq!(p0.charge_mode().unwrap(), ChargeModeActual::Normal);
}

#[test]
fn gbmd_reader_decodes_the_vendor_status_bitmask() {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    struct StatusOnly(Arc<AtomicU32>);

    impl IoctlTransport for StatusOnly {
        fn call(&self, code: u32, input: &[u8], _: usize) -> lctrl_core::Result<Vec<u8>> {
            assert_eq!(code, IOCTL_GBMD);
            assert_eq!(input, [0xff]);
            Ok(self.0.load(Ordering::SeqCst).to_le_bytes().to_vec())
        }
    }

    for (raw, expected) in [
        (0u32, ChargeModeActual::Normal),
        (1 << 5, ChargeModeActual::Conservation),
        (0x0086_0004, ChargeModeActual::Rapid),
    ] {
        let reader = GbmdChargeModeReader::new(StatusOnly(Arc::new(AtomicU32::new(raw))));
        assert_eq!(reader.read_charge_mode().unwrap(), expected, "raw {raw:#x}");
    }
}
