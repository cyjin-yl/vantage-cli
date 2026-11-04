#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use lctrl_core::{ApplyMode, DolbyProfile, LctrlError, Result};
use lctrl_hal::AudioControl;
use lctrl_hal_win::{DolbyRpc, WindowsDolbyAudio};
use parking_lot::Mutex;

#[derive(Default)]
struct FakeRpc {
    enabled: Mutex<bool>,
    profile: Mutex<u32>,
    sku: Mutex<u32>,
    spatial: Mutex<bool>,
    writes: Mutex<Vec<String>>,
    /// When set, the RPC write exports report rejection.
    reject_writes: bool,
    /// When true, GetActiveProfile returns an out-of-range index.
    bad_profile: bool,
}

impl DolbyRpc for FakeRpc {
    fn dolby_enabled(&self) -> Result<bool> {
        Ok(*self.enabled.lock())
    }

    fn set_dolby_enabled(&self, enabled: bool) -> Result<()> {
        if self.reject_writes {
            return Err(LctrlError::ChannelUnavailable {
                channel: "rejected".into(),
            });
        }
        self.writes.lock().push(format!("enabled={enabled}"));
        *self.enabled.lock() = enabled;
        Ok(())
    }

    fn active_profile(&self) -> Result<u32> {
        Ok(if self.bad_profile {
            42
        } else {
            *self.profile.lock()
        })
    }

    fn set_active_profile(&self, index: u32) -> Result<()> {
        if self.reject_writes {
            return Err(LctrlError::ChannelUnavailable {
                channel: "rejected".into(),
            });
        }
        self.writes.lock().push(format!("profile={index}"));
        *self.profile.lock() = index;
        Ok(())
    }

    fn sku(&self) -> Result<u32> {
        Ok(*self.sku.lock())
    }

    fn spatial_supported(&self) -> Result<bool> {
        Ok(*self.spatial.lock())
    }
}

#[test]
fn state_maps_rpc_fields() {
    let rpc = FakeRpc::default();
    *rpc.enabled.lock() = true;
    *rpc.profile.lock() = 5;
    *rpc.sku.lock() = 1;
    *rpc.spatial.lock() = true;
    let audio = WindowsDolbyAudio::new(rpc);
    let state = audio.dolby_state().expect("state reads");
    assert!(state.enabled);
    assert_eq!(state.active_profile, DolbyProfile::Dynamic);
    assert_eq!(state.sku, Some(1));
    assert_eq!(state.spatial_supported, Some(true));
}

#[test]
fn off_disables_without_profile_write() {
    let rpc = FakeRpc::default();
    *rpc.enabled.lock() = true;
    *rpc.profile.lock() = 2;
    let audio = WindowsDolbyAudio::new(rpc);
    let report = audio
        .set_dolby_profile(DolbyProfile::Off, ApplyMode::Commit)
        .expect("off applies");
    // `Off` only clears `enabled` — the device keeps its last preset, so the
    // committed readback reports `active_profile` as the prior `Game`, not the
    // requested `Off`. Assert the fields the write is allowed to change, not a
    // full `requested == actual` that the Off path is designed to violate.
    let actual = report.actual().expect("readback");
    assert_eq!(actual.enabled, report.requested().enabled);
    assert!(!actual.enabled);
    assert_eq!(
        audio.dolby_state().expect("state").active_profile,
        DolbyProfile::Game
    );
}

#[test]
fn profile_enables_then_selects_index() {
    let rpc = FakeRpc::default();
    let audio = WindowsDolbyAudio::new(rpc);
    let report = audio
        .set_dolby_profile(DolbyProfile::Movie, ApplyMode::Commit)
        .expect("profile applies");
    assert!(report.actual().expect("readback").enabled);
    assert_eq!(
        report.actual().expect("readback").active_profile,
        DolbyProfile::Movie
    );
}

#[test]
fn dry_run_sends_no_writes() {
    let rpc = FakeRpc::default();
    *rpc.enabled.lock() = false;
    let audio = WindowsDolbyAudio::new(rpc);
    let report = audio
        .set_dolby_profile(DolbyProfile::Music, ApplyMode::DryRun)
        .expect("dry run plans");
    assert!(report.actual().is_none());
    assert!(report.requested().enabled);
    assert_eq!(report.requested().active_profile, DolbyProfile::Music);
    assert!(audio.rpc().writes.lock().is_empty());
}

#[test]
fn rejected_write_surfaces_error() {
    let rpc = FakeRpc {
        reject_writes: true,
        ..FakeRpc::default()
    };
    let audio = WindowsDolbyAudio::new(rpc);
    assert!(
        audio
            .set_dolby_profile(DolbyProfile::Dynamic, ApplyMode::Commit)
            .is_err()
    );
}

#[test]
fn out_of_range_profile_index_is_error() {
    let rpc = FakeRpc {
        bad_profile: true,
        ..FakeRpc::default()
    };
    let audio = WindowsDolbyAudio::new(rpc);
    assert!(audio.dolby_state().is_err());
}
