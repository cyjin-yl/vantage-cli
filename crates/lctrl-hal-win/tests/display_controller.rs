use std::collections::{BTreeMap, VecDeque};

use lctrl_core::{
    ApplyMode, LctrlError, PanelAdapterId, PanelMode, PanelRefreshRequest, PanelRotation,
    PanelTarget, RefreshMode, Result,
};
use lctrl_hal::PanelControl;
use lctrl_hal_win::{DisplayApi, WindowsPanelController, WmiObject, WmiTransport, WmiValue};
use parking_lot::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Call {
    Targets,
    Current,
    Compatible,
    Test(PanelMode),
    Apply(PanelMode),
}

struct FakeDisplay {
    targets: Vec<PanelTarget>,
    modes: Vec<PanelMode>,
    current: Mutex<PanelMode>,
    calls: Mutex<Vec<Call>>,
    apply_results: Mutex<VecDeque<Result<()>>>,
    readbacks: Mutex<VecDeque<PanelMode>>,
}

impl FakeDisplay {
    fn fixture() -> Self {
        let internal = target(r"\\.\DISPLAY1", 1, true);
        let external = target(r"\\.\DISPLAY2", 2, false);
        let current = mode(&internal, 60);
        let exact_120 = current.with_refresh_hz(120);
        let exact_90 = current.with_refresh_hz(90);

        let mut wrong_width = exact_120.clone();
        wrong_width.width += 1;
        let mut wrong_height = exact_120.clone();
        wrong_height.height += 1;
        let mut wrong_depth = exact_120.clone();
        wrong_depth.bits_per_pixel = 16;
        let mut wrong_position = exact_120.clone();
        wrong_position.position_x += 1;
        let mut wrong_rotation = exact_120.clone();
        wrong_rotation.rotation = PanelRotation::Rotate90;
        let wrong_adapter = PanelMode::new(
            target(r"\\.\DISPLAY9", 9, true),
            current.width,
            current.height,
            current.bits_per_pixel,
            120,
        );

        Self {
            targets: vec![internal, external],
            modes: vec![
                current.clone(),
                exact_120,
                exact_90,
                wrong_width,
                wrong_height,
                wrong_depth,
                wrong_position,
                wrong_rotation,
                wrong_adapter,
            ],
            current: Mutex::new(current),
            calls: Mutex::new(Vec::new()),
            apply_results: Mutex::new(VecDeque::new()),
            readbacks: Mutex::new(VecDeque::new()),
        }
    }

    fn with_apply_results(mut self, results: impl IntoIterator<Item = Result<()>>) -> Self {
        self.apply_results = Mutex::new(results.into_iter().collect());
        self
    }

    fn with_readbacks(mut self, modes: impl IntoIterator<Item = PanelMode>) -> Self {
        self.readbacks = Mutex::new(modes.into_iter().collect());
        self
    }
}

impl DisplayApi for FakeDisplay {
    fn connected_targets(&self) -> Result<Vec<PanelTarget>> {
        self.calls.lock().push(Call::Targets);
        Ok(self.targets.clone())
    }

    fn current_mode(&self, target: &PanelTarget) -> Result<PanelMode> {
        self.calls.lock().push(Call::Current);
        let applied = self
            .calls
            .lock()
            .iter()
            .any(|call| matches!(call, Call::Apply(_)));
        if applied {
            if let Some(mode) = self.readbacks.lock().pop_front() {
                return Ok(mode);
            }
        }
        let current = self.current.lock().clone();
        if current.target != *target {
            return Err(LctrlError::ChannelUnavailable {
                channel: "fake current target mismatch".into(),
            });
        }
        Ok(current)
    }

    fn compatible_modes(&self, _target: &PanelTarget) -> Result<Vec<PanelMode>> {
        self.calls.lock().push(Call::Compatible);
        Ok(self.modes.clone())
    }

    fn test_mode(&self, mode: &PanelMode) -> Result<()> {
        self.calls.lock().push(Call::Test(mode.clone()));
        Ok(())
    }

    fn apply_mode(&self, mode: &PanelMode) -> Result<()> {
        self.calls.lock().push(Call::Apply(mode.clone()));
        let result = self.apply_results.lock().pop_front().unwrap_or(Ok(()));
        if result.is_ok() {
            *self.current.lock() = mode.clone();
        }
        result
    }
}

#[derive(Default)]
struct FakeWmi {
    objects: Vec<WmiObject>,
}

impl FakeWmi {
    fn fixture() -> Self {
        Self {
            objects: vec![panel_object(60, 120, 60, RefreshMode::Adaptive)],
        }
    }
}

impl WmiTransport for FakeWmi {
    fn query(&self, _namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        if wql.contains("LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA") {
            Ok(self.objects.clone())
        } else {
            Ok(Vec::new())
        }
    }

    fn invoke_instance(
        &self,
        _namespace: &str,
        _class: &str,
        _path: &str,
        _method: &str,
        _input: &WmiObject,
    ) -> Result<WmiObject> {
        Err(LctrlError::ChannelUnavailable {
            channel: "panel metadata test must not invoke WMI methods".into(),
        })
    }
}

fn target(device_name: &str, adapter_low: u32, internal: bool) -> PanelTarget {
    PanelTarget::new(
        device_name,
        PanelAdapterId::new(0x1020_3040, adapter_low),
        7,
        11,
        internal,
    )
}

fn mode(target: &PanelTarget, refresh_hz: u32) -> PanelMode {
    let mut mode = PanelMode::new(target.clone(), 3200, 2000, 32, refresh_hz);
    mode.position_x = 40;
    mode.position_y = -20;
    mode
}

fn panel_object(min: u16, max: u16, default: u16, refresh_mode: RefreshMode) -> WmiObject {
    BTreeMap::from([
        ("MinimumRefreshRate".into(), WmiValue::U16(min)),
        ("MaximumRefreshRate".into(), WmiValue::U16(max)),
        ("DefaultRefreshRate".into(), WmiValue::U16(default)),
        ("Mode".into(), WmiValue::U16(refresh_mode_raw(refresh_mode))),
    ])
}

const fn refresh_mode_raw(mode: RefreshMode) -> u16 {
    match mode {
        RefreshMode::Manual => 0,
        RefreshMode::Adaptive => 1,
        RefreshMode::Performance => 2,
        RefreshMode::Unknown(raw) => raw,
    }
}

fn controller(display: FakeDisplay) -> WindowsPanelController<FakeDisplay, FakeWmi> {
    WindowsPanelController::new(display, FakeWmi::fixture())
}

fn apply_rates(display: &FakeDisplay) -> Vec<u32> {
    display
        .calls
        .lock()
        .iter()
        .filter_map(|call| match call {
            Call::Apply(mode) => Some(mode.refresh_hz),
            _ => None,
        })
        .collect()
}

#[test]
fn fixed_rate_uses_exact_native_mode_and_preserves_layout() {
    let controller = controller(FakeDisplay::fixture());

    let report = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.previous().refresh_hz, 60);
    assert_eq!(report.requested().refresh_hz, 120);
    assert_eq!(report.actual(), Some(report.requested()));
    assert!(
        report
            .previous()
            .same_mode_except_refresh(report.requested())
    );
    assert_eq!(report.requested().position_x, 40);
    assert_eq!(report.requested().position_y, -20);
    assert_eq!(report.requested().bits_per_pixel, 32);
    assert_eq!(report.requested().rotation, PanelRotation::Default);
    assert_eq!(
        report.requested().target.adapter,
        report.previous().target.adapter
    );
    assert_eq!(apply_rates(controller.display()), vec![120]);
}

#[test]
fn firmware_range_does_not_admit_a_mode_missing_from_native_enumeration() {
    let mut display = FakeDisplay::fixture();
    let current = display.current.lock().clone();
    display
        .modes
        .retain(|mode| mode.refresh_hz != 120 || !current.same_mode_except_refresh(mode));
    let controller = controller(display);

    let error = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::Unsupported { ref feature }
            if feature == "panel.refresh.120Hz.exact_native_mode"
    ));
    assert!(apply_rates(controller.display()).is_empty());
    assert!(
        !controller
            .display()
            .calls
            .lock()
            .iter()
            .any(|call| matches!(call, Call::Test(_)))
    );
}

#[test]
fn dry_run_tests_the_exact_mode_but_never_applies_it() {
    let controller = controller(FakeDisplay::fixture());

    let report = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.previous().refresh_hz, 60);
    assert_eq!(report.requested().refresh_hz, 120);
    assert_eq!(report.actual(), None);
    assert!(apply_rates(controller.display()).is_empty());
    assert_eq!(controller.display().current.lock().refresh_hz, 60);
    assert!(
        controller
            .display()
            .calls
            .lock()
            .iter()
            .any(|call| matches!(call, Call::Test(mode) if mode.refresh_hz == 120))
    );
}

#[test]
fn auto_performs_a_real_transition_to_the_firmware_default() {
    let display = FakeDisplay::fixture();
    let internal = display.targets[0].clone();
    *display.current.lock() = mode(&internal, 120);
    let controller = controller(display);

    let report = controller
        .set_refresh_rate(PanelRefreshRequest::Auto, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.previous().refresh_hz, 120);
    assert_eq!(report.requested().refresh_hz, 60);
    assert_eq!(report.actual().unwrap().refresh_hz, 60);
    assert_eq!(apply_rates(controller.display()), vec![60]);
}

#[test]
fn fixed_60_also_performs_a_verified_transition() {
    let display = FakeDisplay::fixture();
    let internal = display.targets[0].clone();
    *display.current.lock() = mode(&internal, 120);
    let controller = controller(display);

    let report = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(60), ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.previous().refresh_hz, 120);
    assert_eq!(report.actual().unwrap().refresh_hz, 60);
    assert_eq!(apply_rates(controller.display()), vec![60]);
}

#[test]
fn external_and_ambiguous_targets_are_rejected_before_any_write() {
    let mut external = FakeDisplay::fixture();
    external.targets = vec![target(r"\\.\DISPLAY2", 2, false)];
    let external = controller(external);
    assert!(matches!(
        external.set_refresh_rate(PanelRefreshRequest::Fixed(60), ApplyMode::Commit),
        Err(LctrlError::Unsupported { ref feature }) if feature == "panel.internal.connected"
    ));
    assert_eq!(&*external.display().calls.lock(), &[Call::Targets]);

    let mut ambiguous = FakeDisplay::fixture();
    ambiguous.targets = vec![
        target(r"\\.\DISPLAY1", 1, true),
        target(r"\\.\DISPLAY3", 3, true),
    ];
    let ambiguous = controller(ambiguous);
    assert!(matches!(
        ambiguous.set_refresh_rate(PanelRefreshRequest::Fixed(60), ApplyMode::Commit),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert_eq!(&*ambiguous.display().calls.lock(), &[Call::Targets]);
}

#[test]
fn panel_mode_reads_the_actual_native_current_mode() {
    let display = FakeDisplay::fixture();
    let internal = display.targets[0].clone();
    *display.current.lock() = mode(&internal, 120);
    let controller = controller(display);

    let current = controller.panel_mode().unwrap();

    assert_eq!(current.refresh_hz, 120);
    assert_eq!(current.width, 3200);
    assert_eq!(current.position_x, 40);
    assert_eq!(current.target.adapter, PanelAdapterId::new(0x1020_3040, 1));
}

#[test]
fn readback_mismatch_rolls_back_the_complete_previous_mode() {
    let previous = mode(&target(r"\\.\DISPLAY1", 1, true), 60);
    let unexpected = previous.with_refresh_hz(90);
    let display = FakeDisplay::fixture().with_readbacks([unexpected]);
    let controller = controller(display);

    let error = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
    assert_eq!(apply_rates(controller.display()), vec![120, 60]);
    assert_eq!(&*controller.display().current.lock(), &previous);
}

#[test]
fn apply_failure_rolls_back_and_preserves_the_primary_error() {
    let display = FakeDisplay::fixture().with_apply_results([
        Err(LctrlError::FirmwareRejected {
            detail: "injected apply failure".into(),
        }),
        Ok(()),
    ]);
    let controller = controller(display);

    let error = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { ref detail } if detail == "injected apply failure"
    ));
    assert_eq!(apply_rates(controller.display()), vec![120, 60]);
    assert_eq!(controller.display().current.lock().refresh_hz, 60);
}

#[test]
fn rollback_failure_is_not_hidden() {
    let display = FakeDisplay::fixture()
        .with_readbacks([mode(&target(r"\\.\DISPLAY1", 1, true), 90)])
        .with_apply_results([
            Ok(()),
            Err(LctrlError::ChannelUnavailable {
                channel: "injected rollback failure".into(),
            }),
        ]);
    let controller = controller(display);

    let error = controller
        .set_refresh_rate(PanelRefreshRequest::Fixed(120), ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { ref detail }
            if detail.contains("rollback also failed") && detail.contains("injected rollback failure")
    ));
    assert_eq!(apply_rates(controller.display()), vec![120, 60]);
}

#[test]
fn firmware_metadata_is_still_read_with_exact_types() {
    let controller = controller(FakeDisplay::fixture());

    let capability = controller.refresh_capability().unwrap();
    assert_eq!(capability.min_hz, 60);
    assert_eq!(capability.max_hz, 120);
    assert_eq!(capability.default_hz, 60);
    assert_eq!(controller.refresh_mode().unwrap(), RefreshMode::Adaptive);
}

#[test]
fn repeated_identical_firmware_records_describe_one_panel() {
    let object = panel_object(60, 120, 60, RefreshMode::Manual);
    let wmi = FakeWmi {
        objects: vec![object; 22],
    };
    let controller = WindowsPanelController::new(FakeDisplay::fixture(), wmi);

    let capability = controller.refresh_capability().unwrap();
    assert_eq!(capability.min_hz, 60);
    assert_eq!(capability.max_hz, 120);
    assert_eq!(capability.default_hz, 60);
    assert_eq!(controller.refresh_mode().unwrap(), RefreshMode::Manual);
}

#[test]
fn disagreeing_firmware_records_stay_ambiguous() {
    let wmi = FakeWmi {
        objects: vec![
            panel_object(60, 120, 60, RefreshMode::Manual),
            panel_object(60, 165, 60, RefreshMode::Manual),
        ],
    };
    let controller = WindowsPanelController::new(FakeDisplay::fixture(), wmi);

    assert!(matches!(
        controller.refresh_capability(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn unreadable_duplicate_record_fails_closed() {
    let mut truncated = panel_object(60, 120, 60, RefreshMode::Manual);
    truncated.remove("Mode");
    let wmi = FakeWmi {
        objects: vec![panel_object(60, 120, 60, RefreshMode::Manual), truncated],
    };
    let controller = WindowsPanelController::new(FakeDisplay::fixture(), wmi);

    assert!(matches!(
        controller.refresh_capability(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}
