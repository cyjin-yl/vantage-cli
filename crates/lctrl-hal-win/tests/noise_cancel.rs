#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use lctrl_core::{ApplyMode, LctrlError, NoiseCancellationMode, Result};
use lctrl_hal::NoiseCancelControl;
use lctrl_hal_win::{ElevocRpc, WindowsNoiseCancel};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone)]
struct FakeRpc {
    mode: Arc<Mutex<i32>>,
    set_result: Arc<Mutex<i32>>,
    voice_id: Arc<Mutex<i32>>,
    set_calls: Arc<Mutex<Vec<i32>>>,
}

impl Default for FakeRpc {
    fn default() -> Self {
        Self {
            mode: Arc::new(Mutex::new(0)),
            set_result: Arc::new(Mutex::new(1)),
            voice_id: Arc::new(Mutex::new(0)),
            set_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ElevocRpc for FakeRpc {
    fn current_mode(&self) -> Result<i32> {
        Ok(*self.mode.lock())
    }

    fn set_effect_mode(&self, mode: u32) -> Result<()> {
        self.set_calls.lock().push(mode as i32);
        match *self.set_result.lock() {
            1 => {
                *self.mode.lock() = mode as i32;
                Ok(())
            }
            other => Err(map_set_result(other, mode)),
        }
    }

    fn voice_id_status(&self) -> Result<i32> {
        Ok(*self.voice_id.lock())
    }
}

fn map_set_result(result: i32, mode: u32) -> LctrlError {
    match result {
        -1 => LctrlError::ChannelUnavailable {
            channel: "Elevoc SetEffectMode RPC or default-endpoint failure".to_string(),
        },
        2 => LctrlError::Unsupported {
            feature: format!("noise-cancel mode {mode} is not supported by this endpoint"),
        },
        4 => LctrlError::InvalidArgument {
            detail: "voiceid mode requires an enrolled VoiceID profile".to_string(),
        },
        other => LctrlError::ChannelUnavailable {
            channel: format!("Elevoc SetEffectMode returned unexpected result {other}"),
        },
    }
}

#[test]
fn state_maps_sdk_mode_and_voice_id() {
    let rpc = FakeRpc::default();
    *rpc.mode.lock() = 1;
    *rpc.voice_id.lock() = 2;
    let control = WindowsNoiseCancel::new(rpc);
    let state = control.noise_cancel_state().expect("state reads");
    assert_eq!(state.mic_mode, NoiseCancellationMode::Shared);
    assert_eq!(state.voice_id_enrolled, Some(true));
}

#[test]
fn set_mode_commits_and_reads_back() {
    let rpc = FakeRpc::default();
    let control = WindowsNoiseCancel::new(rpc.clone());
    let report = control
        .set_noise_cancel_mode(NoiseCancellationMode::Single, ApplyMode::Commit)
        .expect("commit applies");
    assert_eq!(rpc.set_calls.lock().as_slice(), &[2]);
    assert_eq!(
        report.actual().expect("readback").mic_mode,
        NoiseCancellationMode::Single
    );
}

#[test]
fn dry_run_sends_no_sdk_writes() {
    let rpc = FakeRpc::default();
    let control = WindowsNoiseCancel::new(rpc.clone());
    let report = control
        .set_noise_cancel_mode(NoiseCancellationMode::Shared, ApplyMode::DryRun)
        .expect("dry run plans");
    assert!(rpc.set_calls.lock().is_empty());
    assert!(report.actual().is_none());
    assert_eq!(report.requested().mic_mode, NoiseCancellationMode::Shared);
}

#[test]
fn endpoint_rejection_is_unsupported() {
    let rpc = FakeRpc {
        set_result: Arc::new(Mutex::new(2)),
        ..FakeRpc::default()
    };
    let control = WindowsNoiseCancel::new(rpc);
    let error = control
        .set_noise_cancel_mode(NoiseCancellationMode::Spatial, ApplyMode::Commit)
        .expect_err("rejection surfaces");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
}

#[test]
fn voice_id_without_enrollment_is_invalid_argument() {
    let rpc = FakeRpc {
        set_result: Arc::new(Mutex::new(4)),
        ..FakeRpc::default()
    };
    let control = WindowsNoiseCancel::new(rpc);
    let error = control
        .set_noise_cancel_mode(NoiseCancellationMode::VoiceId, ApplyMode::Commit)
        .expect_err("gate surfaces");
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
}

#[test]
fn unknown_wire_mode_is_error() {
    let rpc = FakeRpc::default();
    *rpc.mode.lock() = 9;
    let control = WindowsNoiseCancel::new(rpc);
    let state = control.noise_cancel_state().expect_err("unknown mode");
    drop(state);
}
