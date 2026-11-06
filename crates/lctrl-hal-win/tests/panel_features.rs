#![cfg(windows)]
// The IGCL panel channel only exists on Windows.
use lctrl_core::{ApplyMode, EyeCareLevel, LctrlError, Result};
use lctrl_hal::PanelFeaturesControl;
use lctrl_hal_win::{
    GammaRampApi, IntelCtlApi, PixtxBlock, PixtxCapability, RampSamples, SuperResolutionCaps,
    WindowsPanelFeatures, classify_lut, eye_care_lut, identity_lut,
};
use parking_lot::Mutex;
use std::sync::Arc;

/// Neutral desktop ramp: entry `i` maps input `i/255` to itself.
fn identity_ramp() -> RampSamples {
    std::array::from_fn(|index| {
        let sample = (index % 256) as u32;
        (sample * 65535 / 255) as u16
    })
}

#[derive(Clone)]
struct FakeGamma {
    ramp: Arc<Mutex<RampSamples>>,
    writes: Arc<Mutex<usize>>,
    ignore_writes: Arc<Mutex<bool>>,
}

impl Default for FakeGamma {
    fn default() -> Self {
        Self {
            ramp: Arc::new(Mutex::new(identity_ramp())),
            writes: Arc::new(Mutex::new(0)),
            ignore_writes: Arc::new(Mutex::new(false)),
        }
    }
}

impl GammaRampApi for FakeGamma {
    fn get(&self) -> Result<RampSamples> {
        Ok(*self.ramp.lock())
    }

    fn set(&self, ramp: &RampSamples) -> Result<()> {
        *self.writes.lock() += 1;
        if !*self.ignore_writes.lock() {
            *self.ramp.lock() = *ramp;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeCtl {
    sr_flag: Arc<Mutex<u32>>,
    sr_writes: Arc<Mutex<usize>>,
    reject_state: Arc<Mutex<bool>>,
    ignore_writes: Arc<Mutex<bool>>,
    force_flag: Arc<Mutex<Option<u32>>>,
    pixtx: Arc<Mutex<Option<PixtxCapability>>>,
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
        // A forced flag applies to exactly one write, so the restore write can
        // succeed and be observed.
        if let Some(forced) = self.force_flag.lock().take() {
            *self.sr_flag.lock() = forced;
        } else if !*self.ignore_writes.lock() {
            *self.sr_flag.lock() = flag;
        }
        Ok(())
    }

    fn pixtx_capability(&self) -> Result<PixtxCapability> {
        Ok(self.pixtx.lock().clone().unwrap_or_default())
    }

    fn super_resolution_capability(&self) -> Result<SuperResolutionCaps> {
        Ok(SuperResolutionCaps::default())
    }
}

#[test]
fn pixtx_blocks_carry_the_1d_lut_geometry_only_for_1d_blocks() {
    let fake = FakeCtl::default();
    *fake.pixtx.lock() = Some(PixtxCapability {
        blocks: vec![
            PixtxBlock {
                block_id: 0x0001_0001,
                block_type: 1,
                sampling_type: Some(0),
                samples_per_channel: Some(256),
                channels: Some(3),
            },
            PixtxBlock {
                block_id: 0x0001_0002,
                block_type: 3,
                sampling_type: None,
                samples_per_channel: None,
                channels: None,
            },
        ],
    });
    let capability = fake.pixtx_capability().expect("capability");
    assert_eq!(capability.blocks.len(), 2);
    assert_eq!(capability.blocks[0].samples_per_channel, Some(256));
    assert_eq!(capability.blocks[1].samples_per_channel, None);
}

#[test]
fn an_empty_block_list_is_a_proven_absent_pipeline() {
    let fake = FakeCtl::default();
    assert!(
        fake.pixtx_capability()
            .expect("capability")
            .blocks
            .is_empty()
    );
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
fn eye_care_channel_is_closed_without_a_gamma_transport() {
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
fn eye_care_reads_an_identity_ramp_as_off() {
    let gamma = FakeGamma::default();
    let features = WindowsPanelFeatures::new(FakeCtl::default())
        .with_gamma_ramp(Box::new(gamma.clone()));
    assert_eq!(features.eye_care().expect("state"), Some(EyeCareLevel::Off));
}

#[test]
fn eye_care_reports_an_unknown_ramp_as_unknown() {
    let gamma = FakeGamma::default();
    *gamma.ramp.lock() = [12345u16; 768];
    let features = WindowsPanelFeatures::new(FakeCtl::default())
        .with_gamma_ramp(Box::new(gamma));
    assert_eq!(features.eye_care().expect("state"), None);
}

#[test]
fn eye_care_commit_applies_the_level_and_reads_it_back() {
    let gamma = FakeGamma::default();
    let features = WindowsPanelFeatures::new(FakeCtl::default())
        .with_gamma_ramp(Box::new(gamma.clone()));
    let report = features
        .set_eye_care(EyeCareLevel::High, ApplyMode::Commit)
        .expect("committed");
    assert_eq!(report.previous(), &Some(EyeCareLevel::Off));
    assert_eq!(report.requested(), &Some(EyeCareLevel::High));
    assert_eq!(report.actual(), Some(&Some(EyeCareLevel::High)));
    assert_eq!(*gamma.writes.lock(), 1);
    assert_eq!(
        features.eye_care().expect("state"),
        Some(EyeCareLevel::High)
    );
}

#[test]
fn eye_care_commit_restores_the_previous_ramp_on_mismatch() {
    let gamma = FakeGamma::default();
    *gamma.ignore_writes.lock() = true;
    let features = WindowsPanelFeatures::new(FakeCtl::default())
        .with_gamma_ramp(Box::new(gamma.clone()));
    let error = features
        .set_eye_care(EyeCareLevel::High, ApplyMode::Commit)
        .expect_err("readback mismatch");
    match error {
        LctrlError::VerifyMismatch { requested, actual } => {
            assert!(requested.contains("high"), "{requested}");
            assert!(actual.contains("previous ramp restored"), "{actual}");
        }
        other => panic!("expected a verify mismatch, got {other}"),
    }
    // The applied ramp plus the restore of the captured ramp.
    assert_eq!(*gamma.writes.lock(), 2);
    assert_eq!(*gamma.ramp.lock(), identity_ramp());
}

#[test]
fn eye_care_dry_run_does_not_touch_the_ramp() {
    let gamma = FakeGamma::default();
    let features = WindowsPanelFeatures::new(FakeCtl::default())
        .with_gamma_ramp(Box::new(gamma.clone()));
    let report = features
        .set_eye_care(EyeCareLevel::Mid, ApplyMode::DryRun)
        .expect("dry run");
    assert_eq!(report.requested(), &Some(EyeCareLevel::Mid));
    assert_eq!(*gamma.writes.lock(), 0);
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

#[test]
fn super_resolution_commit_writes_and_confirms_the_flag() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    let report = features
        .set_super_resolution(true, ApplyMode::Commit)
        .expect("committed");
    assert_eq!(report.previous(), &Some(false));
    assert_eq!(report.requested(), &Some(true));
    assert_eq!(*ctl.sr_writes.lock(), 1);
    assert_eq!(*ctl.sr_flag.lock(), 2);
}

#[test]
fn super_resolution_commit_never_writes_when_the_state_read_is_rejected() {
    let ctl = FakeCtl {
        reject_state: Arc::new(Mutex::new(true)),
        ..FakeCtl::default()
    };
    let features = WindowsPanelFeatures::new(ctl.clone());
    let error = features
        .set_super_resolution(true, ApplyMode::Commit)
        .expect_err("no previous value, no write");
    assert!(matches!(error, LctrlError::Unsupported { .. }));
    assert_eq!(*ctl.sr_writes.lock(), 0);
}

#[test]
fn super_resolution_commit_reports_an_ignored_write_without_a_rollback() {
    let ctl = FakeCtl {
        ignore_writes: Arc::new(Mutex::new(true)),
        ..FakeCtl::default()
    };
    let features = WindowsPanelFeatures::new(ctl.clone());
    let error = features
        .set_super_resolution(true, ApplyMode::Commit)
        .expect_err("readback mismatch");
    match error {
        LctrlError::VerifyMismatch { requested, actual } => {
            assert!(requested.contains("super-resolution flag"), "{requested}");
            assert!(actual.contains("unchanged"), "{actual}");
        }
        other => panic!("expected a verify mismatch, got {other}"),
    }
    // The flag is already at the previous value, so no restore write is sent.
    assert_eq!(*ctl.sr_writes.lock(), 1);
}

#[test]
fn super_resolution_commit_rolls_back_a_third_value() {
    let ctl = FakeCtl {
        // The driver accepts the write but lands on a different flag.
        force_flag: Arc::new(Mutex::new(Some(4))),
        ..FakeCtl::default()
    };
    let features = WindowsPanelFeatures::new(ctl.clone());
    let error = features
        .set_super_resolution(true, ApplyMode::Commit)
        .expect_err("readback mismatch");
    match error {
        LctrlError::VerifyMismatch { actual, .. } => {
            assert!(actual.contains("rolled back"), "{actual}");
        }
        other => panic!("expected a verify mismatch, got {other}"),
    }
    // The requested write plus the restore write of the previous flag.
    assert_eq!(*ctl.sr_writes.lock(), 2);
    assert_eq!(*ctl.sr_flag.lock(), 0);
}

#[test]
fn super_resolution_dry_run_does_not_touch_the_driver() {
    let ctl = FakeCtl::default();
    let features = WindowsPanelFeatures::new(ctl.clone());
    let report = features
        .set_super_resolution(true, ApplyMode::DryRun)
        .expect("dry run");
    assert_eq!(*ctl.sr_writes.lock(), 0);
    assert_eq!(report.requested(), &Some(true));
}
