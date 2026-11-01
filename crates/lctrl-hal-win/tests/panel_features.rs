use lctrl_core::{ApplyMode, EyeCareLevel, LctrlError, Result};
use lctrl_hal::PanelFeaturesControl;
use lctrl_hal_win::{IntelCtlApi, WindowsPanelFeatures, classify_lut, eye_care_lut, identity_lut};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone)]
struct FakeCtl {
    lut: Arc<Mutex<Option<[f64; 768]>>>,
    lut_writes: Arc<Mutex<usize>>,
    sr_flag: Arc<Mutex<u32>>,
    sr_writes: Arc<Mutex<usize>>,
    fail_sr: Arc<Mutex<bool>>,
    has_pixtx: Arc<Mutex<bool>>,
}

impl Default for FakeCtl {
    fn default() -> Self {
        Self {
            lut: Arc::new(Mutex::new(None)),
            lut_writes: Arc::new(Mutex::new(0)),
            sr_flag: Arc::new(Mutex::new(0)),
            sr_writes: Arc::new(Mutex::new(0)),
            fail_sr: Arc::new(Mutex::new(false)),
            has_pixtx: Arc::new(Mutex::new(true)),
        }
    }
}

const SR_DISABLE: u32 = 1;
const SR_ENABLE_DEFAULT: u32 = 2;

impl IntelCtlApi for FakeCtl {
    fn primary_output(&self) -> Result<Option<*mut std::ffi::c_void>> {
        Ok(Some(std::ptr::null_mut()))
    }

    fn set_lut(&self, values: Option<&[f64; 768]>) -> Result<()> {
        self.lut_writes.lock().checked_add(1).unwrap();
        *self.lut.lock() = values.copied();
        Ok(())
    }

    fn current_lut(&self) -> Result<[f64; 768]> {
        let guard = self.lut.lock();
        match guard.as_ref() {
            Some(lut) => Ok(*lut),
            None => Err(LctrlError::Unsupported {
                feature: "panel.eye-care.readback.driver-query".to_string(),
            }),
        }
    }

    fn super_resolution_flag(&self) -> Result<u32> {
        if *self.fail_sr.lock() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "injected SR failure".to_string(),
            });
        }
        Ok(*self.sr_flag.lock())
    }

    fn set_super_resolution_flag(&self, flag: u32) -> Result<()> {
        *self.sr_writes.lock() += 1;
        *self.sr_flag.lock() = flag;
        Ok(())
    }

    fn has_pixel_transformation(&self) -> Result<bool> {
        Ok(*self.has_pixtx.lock())
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
    // Blue channel gain below red at max sample for both levels.
    assert!(high[767] < high[255]);
    assert!(mid[767] < mid[255]);
    // Stronger level cuts blue further.
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
fn set_eye_care_commit_writes_and_reports() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    let report = features
        .set_eye_care(EyeCareLevel::High, ApplyMode::Commit)
        .expect("commit");
    assert_eq!(*ctl.lut_writes.lock(), 1);
    assert_eq!(report.actual(), Some(&Some(EyeCareLevel::High)));
    assert_eq!(report.previous(), &None);
    let observed = classify_lut(&ctl.lut.lock().as_ref().expect("lut set")).expect("level");
    assert_eq!(observed, EyeCareLevel::High);
}

#[test]
fn set_eye_care_off_restores_default() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::Commit)
        .expect("set");
    features
        .set_eye_care(EyeCareLevel::Off, ApplyMode::Commit)
        .expect("off");
    assert_eq!(*ctl.lut_writes.lock(), 2);
    assert!(ctl.lut.lock().is_none(), "restore-default clears the LUT");
}

#[test]
fn dry_run_sends_no_lut_writes() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    let report = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::DryRun)
        .expect("dry run");
    assert_eq!(*ctl.lut_writes.lock(), 0);
    assert!(report.actual().is_none());
    assert_eq!(report.requested(), &Some(EyeCareLevel::Mid));
}

#[test]
fn custom_level_is_rejected() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl);
    let error = features
        .set_eye_care(EyeCareLevel::Custom, ApplyMode::Commit)
        .expect_err("custom rejected");
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
}

#[test]
fn super_resolution_commit_toggles_flag() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    assert_eq!(
        features.super_resolution().expect("initial state"),
        Some(false)
    );
    let report = features
        .set_super_resolution(true, ApplyMode::Commit)
        .expect("enable");
    assert_eq!(*ctl.sr_writes.lock(), 1);
    assert_eq!(*ctl.sr_flag.lock(), SR_ENABLE_DEFAULT);
    assert_eq!(report.actual(), Some(&Some(true)));
    assert_eq!(features.super_resolution().expect("readback"), Some(true));
    features
        .set_super_resolution(false, ApplyMode::Commit)
        .expect("disable");
    assert_eq!(*ctl.sr_flag.lock(), SR_DISABLE);
}

#[test]
fn super_resolution_dry_run_sends_no_writes() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    let report = features
        .set_super_resolution(true, ApplyMode::DryRun)
        .expect("dry run");
    assert_eq!(*ctl.sr_writes.lock(), 0);
    assert!(report.actual().is_none());
}

#[test]
fn eye_care_is_unknown_until_the_driver_reports_a_lut() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    // Nothing written yet and the driver cannot report a current LUT: the
    // state is unknown, never invented.
    let error = features.eye_care().expect_err("no readback yet");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    let report = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::Commit)
        .expect("set");
    assert_eq!(report.previous(), &None);
    assert_eq!(
        features.eye_care().expect("after write"),
        Some(EyeCareLevel::Mid)
    );
}

#[test]
fn eye_care_is_unsupported_without_a_pixel_transformation_pipeline() {
    let ctl = FakeCtl {
        has_pixtx: Arc::new(Mutex::new(false)),
        ..FakeCtl::default()
    };
    let features = WindowsPanelFeatures::new(ctl.clone());
    let error = features.eye_care().expect_err("no pipeline");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    let error = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::DryRun)
        .expect_err("no pipeline write either");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert_eq!(*ctl.lut_writes.lock(), 0);
}
