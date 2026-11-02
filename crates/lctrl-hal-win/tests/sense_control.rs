#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use lctrl_core::{ApplyMode, LctrlError, Result, SenseDistance, SenseState, SenseWait};
use lctrl_hal::{SenseControl, SenseFeature};
use lctrl_hal_win::{HpdRpc, HpdSettingsStore, SensePipe, WindowsSenseControl};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone)]
struct FakeRpc {
    calls: Arc<Mutex<Vec<(u32, u32)>>>,
    state: Arc<Mutex<SenseState>>,
    result: i64,
}

impl HpdRpc for FakeRpc {
    fn read_state(&self) -> Result<SenseState> {
        Ok(*self.state.lock())
    }

    fn set_feature(&self, feature: SenseFeature) -> Result<()> {
        let (op, arg) = feature.rpc_command().expect("recovered RPC feature");
        self.calls.lock().push((op, arg));
        if self.result != 0 {
            return Err(LctrlError::FirmwareRejected {
                detail: format!("scripted HPD Win32 status {}", self.result),
            });
        }
        let mut state = self.state.lock();
        match feature {
            SenseFeature::LockOnLeave(value) => state.lock_on_leave = Some(value),
            SenseFeature::LeaveDistance(value) => state.leave_distance = Some(value),
            SenseFeature::LeaveWait(value) => state.leave_wait = Some(value),
            SenseFeature::WakeOnApproach(value) => state.wake_on_approach = Some(value),
            SenseFeature::ApproachDistance(value) => state.approach_distance = Some(value),
            SenseFeature::PauseVideo(value) => state.pause_video = Some(value),
            _ => panic!("legacy feature must not reach the HPD RPC"),
        }
        Ok(())
    }
}

/// The separate legacy pipe is retained only for privacy/attention settings.
#[derive(Clone)]
struct FakePipe {
    calls: Arc<Mutex<Vec<(u32, u32)>>>,
    state: Arc<Mutex<SenseState>>,
    result: i32,
}

impl FakePipe {
    fn new(state: Arc<Mutex<SenseState>>, result: i32) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            state,
            result,
        }
    }
}

impl SensePipe for FakePipe {
    fn call(&self, code: u32, arg: u32) -> Result<i32> {
        self.calls.lock().push((code, arg));
        if self.result == 1 {
            let mut state = self.state.lock();
            if code == 0x000D_0201 {
                state.privacy_alert = Some(arg == 1);
            }
            // Attention tracking has no observed persistence key.
        }
        Ok(self.result)
    }
}

#[derive(Clone)]
struct FakeStore {
    state: Arc<Mutex<SenseState>>,
    reads: Arc<Mutex<u32>>,
}

impl HpdSettingsStore for FakeStore {
    fn read_state(&self) -> Result<SenseState> {
        *self.reads.lock() += 1;
        Ok(*self.state.lock())
    }
}

fn fixture(
    rpc_result: i64,
    pipe_result: i32,
) -> (
    WindowsSenseControl<FakeRpc, FakePipe, FakeStore>,
    FakeRpc,
    FakePipe,
    FakeStore,
) {
    let state = Arc::new(Mutex::new(SenseState {
        lock_on_leave: Some(false),
        leave_distance: Some(SenseDistance::Near),
        leave_wait: Some(SenseWait::Short),
        wake_on_approach: Some(false),
        approach_distance: Some(SenseDistance::Near),
        pause_video: Some(false),
        privacy_alert: Some(false),
        attention_tracking: None,
        capability: Some(1),
        sensor_type: Some(2),
        facial_feature_registered: Some(true),
    }));
    let rpc = FakeRpc {
        calls: Arc::new(Mutex::new(Vec::new())),
        state: state.clone(),
        result: rpc_result,
    };
    let pipe = FakePipe::new(state.clone(), pipe_result);
    let store = FakeStore {
        state,
        reads: Arc::new(Mutex::new(0)),
    };
    let control = WindowsSenseControl::new(rpc.clone(), pipe.clone(), store.clone());
    (control, rpc, pipe, store)
}

#[test]
fn lock_on_leave_uses_rpc_op_eleven() {
    let (control, rpc, pipe, store) = fixture(0, 1);
    let report = control
        .set_sense_feature(SenseFeature::LockOnLeave(true), ApplyMode::Commit)
        .expect("commit applies");
    assert_eq!(rpc.calls.lock().as_slice(), &[(11, 1)]);
    assert!(pipe.calls.lock().is_empty());
    assert_eq!(report.actual().expect("readback").lock_on_leave, Some(true));
    assert!(*store.reads.lock() > 1, "readback polled the store");
}

#[test]
fn pause_video_uses_rpc_zero_one_not_pipe_one_two() {
    let (control, rpc, pipe, _) = fixture(0, 1);
    control
        .set_sense_feature(SenseFeature::PauseVideo(true), ApplyMode::Commit)
        .expect("commit applies");
    control
        .set_sense_feature(SenseFeature::PauseVideo(false), ApplyMode::Commit)
        .expect("commit applies");
    assert_eq!(rpc.calls.lock().as_slice(), &[(16, 1), (16, 0)]);
    assert!(pipe.calls.lock().is_empty());
}

#[test]
fn distance_and_wait_map_to_enum_values() {
    let (control, rpc, pipe, _) = fixture(0, 1);
    control
        .set_sense_feature(
            SenseFeature::LeaveDistance(SenseDistance::Far),
            ApplyMode::Commit,
        )
        .expect("commit applies");
    control
        .set_sense_feature(SenseFeature::LeaveWait(SenseWait::Long), ApplyMode::Commit)
        .expect("commit applies");
    control
        .set_sense_feature(
            SenseFeature::ApproachDistance(SenseDistance::Middle),
            ApplyMode::Commit,
        )
        .expect("commit applies");
    assert_eq!(rpc.calls.lock().as_slice(), &[(14, 2), (12, 2), (9, 1)]);
    assert!(pipe.calls.lock().is_empty());
}

#[test]
fn dry_run_sends_no_rpc_or_pipe_writes() {
    let (control, rpc, pipe, _) = fixture(0, 1);
    let report = control
        .set_sense_feature(SenseFeature::WakeOnApproach(true), ApplyMode::DryRun)
        .expect("dry run plans");
    assert!(pipe.calls.lock().is_empty());
    assert!(rpc.calls.lock().is_empty());
    assert!(report.actual().is_none());
    assert_eq!(report.requested().wake_on_approach, Some(true));
}

#[test]
fn rejected_pipe_result_is_firmware_rejected() {
    let (control, _, _, _) = fixture(0, 0);
    let error = control
        .set_sense_feature(SenseFeature::PrivacyAlert(true), ApplyMode::Commit)
        .expect_err("rejection surfaces");
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
}

#[test]
fn pipe_exception_is_channel_unavailable() {
    let (control, _, _, _) = fixture(0, -1);
    let error = control
        .set_sense_feature(SenseFeature::PrivacyAlert(true), ApplyMode::Commit)
        .expect_err("exception surfaces");
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
}

#[test]
fn nonzero_rpc_status_does_not_fall_back_to_pipe() {
    let (control, rpc, pipe, _) = fixture(5, 1);
    let error = control
        .set_sense_feature(SenseFeature::WakeOnApproach(true), ApplyMode::Commit)
        .expect_err("server failure surfaces");
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
    assert_eq!(rpc.calls.lock().as_slice(), &[(8, 1)]);
    assert!(pipe.calls.lock().is_empty());
}

#[test]
fn unadvertised_rpc_feature_is_rejected_before_write() {
    let (control, rpc, pipe, store) = fixture(0, 1);
    store.state.lock().pause_video = None;
    let error = control
        .set_sense_feature(SenseFeature::PauseVideo(true), ApplyMode::Commit)
        .expect_err("missing capability is not a writable false value");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert!(rpc.calls.lock().is_empty());
    assert!(pipe.calls.lock().is_empty());
}
