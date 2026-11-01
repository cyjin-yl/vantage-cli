#![cfg(windows)]
// The IGCL panel channel only exists on Windows.
use lctrl_core::{ApplyMode, EyeCareLevel, LctrlError, Result};
use lctrl_hal::PanelFeaturesControl;
use lctrl_hal_win::{IntelCtlApi, WindowsPanelFeatures, classify_lut, eye_care_lut, identity_lut};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone, Default)]
struct FakeCtl {
    sr_flag: Arc<Mutex<u32>>,
    sr_writes: Arc<Mutex<usize>>,
    reject_state: Arc<Mutex<bool>>,
}

impl IntelCtlApi for FakeCtl {
    fn primary_output(&self) -> Result<Option<*mut std::ffi::c_void>> {
        Ok(Some(std::ptr::null_mut()))
    }

    fn super_resolution_flag(&self) -> Result<u32> {
        if *self.reject_state.lock() {
            return Err(LctrlError::Unsupported {
                feature: "panel.super-resolution.state-query.rejected".to_string(),
            });
        }
        Ok(*self.sr_flag.lock())
    }

    fn set_super_resolution_flag(&self, flag: u32) -> Result<()> {
        *self.sr_writes.lock() += 1;
        *self.sr_flag.lock() = flag;
        Ok(())
    }
}

#[test]
fn identity_lut_is_linear_full_range() {
    let lut = identity_lut();
    assert_eq!(lut[0], 0.0);
    assert_eq!(lut[255], 1.0);
    assert!((lut[128] - 128.0 / 255.0).abs() < 1e-12);
    // All three channels identical.
    assert_eq!(lut[0], lut[256]);
    assert_eq!(lut[255], lut[511]);
    assert_eq!(lut[300], lut[556]);
}

#[test]
fn eye_care_lut_reduces_blue_more_than_red() {
    let high = eye_care_lut(EyeCareLevel::High);
    let mid = eye_care_lut(EyeCareLevel::Mid);
    // Blue channel gain below red at the last sample for both levels.
    assert!(high[767] < high[255]);
    assert!(mid[767] < mid[255]);
    // The stronger level cuts blue further.
    assert!(high[767] < mid[767]);
    // Off equals identity.
    assert_eq!(eye_care_lut(EyeCareLevel::Off), identity_lut());
}

#[test]
fn classify_lut_round_trips_known_levels() {
    for level in [EyeCareLevel::Off, EyeCareLevel::Mid, EyeCareLevel::High] {
        assert_eq!(classify_lut(&eye_care_lut(level)).expect("level"), level);
    }
    let mut other = identity_lut();
    other[100] = 0.123;
    assert!(classify_lut(&other).is_err());
}

#[test]
fn eye_care_channel_is_closed_while_the_block_contract_is_unrecovered() {
    let features = WindowsPanelFeatures::new(FakeCtl::default());
    let error = features.eye_care().expect_err("channel stays closed");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    let error = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::DryRun)
        .expect_err("channel stays closed");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    let error = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::Commit)
        .expect_err("channel stays closed");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
}

#[test]
fn super_resolution_state_is_unknown_when_the_driver_rejects_the_query() {
    let ctl = FakeCtl {
        reject_state: Arc::new(Mutex::new(true)),
        ..FakeCtl::default()
    };
    let features = WindowsPanelFeatures::new(ctl);
    assert_eq!(
        features.super_resolution().expect("unknown, not an error"),
        None
    );
}

#[test]
fn super_resolution_reads_the_flag_when_the_driver_answers() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    assert_eq!(features.super_resolution().expect("state"), Some(false));
    *ctl.sr_flag.lock() = 2;
    assert_eq!(features.super_resolution().expect("state"), Some(true));
}
