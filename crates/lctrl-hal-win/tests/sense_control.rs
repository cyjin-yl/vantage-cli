use lctrl_core::{ApplyMode, LctrlError, Result, SenseDistance, SenseState, SenseWait};
use lctrl_hal::{SenseControl, SenseFeature};
use lctrl_hal_win::{HpdSettingsStore, SensePipe, WindowsSenseControl};
use parking_lot::Mutex;
use std::sync::Arc;

/// Pipe fake that applies accepted writes to the shared state, mirroring the
/// real service persisting to the HPD store.
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
            match code {
                0x0009_0101 => state.lock_on_leave = Some(arg != 0),
                0x0011_0101 => state.leave_distance = SenseDistance::from_raw(arg),
                0x000A_0101 => state.leave_wait = SenseWait::from_raw(arg),
                0x0007_0101 => state.wake_on_approach = Some(arg != 0),
                0x0008_0101 => state.approach_distance = SenseDistance::from_raw(arg),
                0x000D_0101 => state.pause_video = Some(arg == 1),
                0x000D_0201 => state.privacy_alert = Some(arg == 1),
                // Attention tracking has no observed persistence key.
                _ => {}
            }
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
    result: i32,
) -> (
    WindowsSenseControl<FakePipe, FakeStore>,
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
    let pipe = FakePipe::new(state.clone(), result);
    let store = FakeStore {
        state,
        reads: Arc::new(Mutex::new(0)),
    };
    let control = WindowsSenseControl::new(pipe.clone(), store.clone());
    (control, pipe, store)
}

#[test]
fn lock_on_leave_uses_legacy_bool_encoding() {
    let (control, pipe, store) = fixture(1);
    let report = control
        .set_sense_feature(SenseFeature::LockOnLeave(true), ApplyMode::Commit)
        .expect("commit applies");
    assert_eq!(pipe.calls.lock().as_slice(), &[(0x0009_0101, 1)]);
    assert_eq!(report.actual().expect("readback").lock_on_leave, Some(true));
    assert!(*store.reads.lock() > 1, "readback polled the store");
}

#[test]
fn pause_video_uses_one_two_encoding() {
    let (control, pipe, _) = fixture(1);
    control
        .set_sense_feature(SenseFeature::PauseVideo(true), ApplyMode::Commit)
        .expect("commit applies");
    control
        .set_sense_feature(SenseFeature::PauseVideo(false), ApplyMode::Commit)
        .expect("commit applies");
    assert_eq!(
        pipe.calls.lock().as_slice(),
        &[(0x000D_0101, 1), (0x000D_0101, 2)]
    );
}

#[test]
fn distance_and_wait_map_to_enum_values() {
    let (control, pipe, _) = fixture(1);
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
    assert_eq!(
        pipe.calls.lock().as_slice(),
        &[(0x0011_0101, 2), (0x000A_0101, 2), (0x0008_0101, 1)]
    );
}

#[test]
fn dry_run_sends_no_pipe_writes() {
    let (control, pipe, _) = fixture(1);
    let report = control
        .set_sense_feature(SenseFeature::WakeOnApproach(true), ApplyMode::DryRun)
        .expect("dry run plans");
    assert!(pipe.calls.lock().is_empty());
    assert!(report.actual().is_none());
    assert_eq!(report.requested().wake_on_approach, Some(true));
}

#[test]
fn rejected_pipe_result_is_firmware_rejected() {
    let (control, _, _) = fixture(0);
    let error = control
        .set_sense_feature(SenseFeature::LockOnLeave(true), ApplyMode::Commit)
        .expect_err("rejection surfaces");
    assert!(matches!(error, LctrlError::FirmwareRejected { .. }));
}

#[test]
fn pipe_exception_is_channel_unavailable() {
    let (control, _, _) = fixture(-1);
    let error = control
        .set_sense_feature(SenseFeature::LockOnLeave(true), ApplyMode::Commit)
        .expect_err("exception surfaces");
    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
}
