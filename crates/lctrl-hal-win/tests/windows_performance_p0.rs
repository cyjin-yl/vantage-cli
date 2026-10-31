use parking_lot::Mutex;

use lctrl_core::{ApplyMode, DispatcherVersion, LctrlError, PerformanceMode};
use lctrl_hal::PerformanceControl;
use lctrl_hal_win::{
    OverlayGuid, PerformanceRegistryReader, PowerOverlayControl, WindowsPerformanceP0,
    mode_for_overlay, overlay_for_mode,
};

struct FakeRegistry {
    values: Mutex<Vec<(&'static str, u32)>>,
}

impl FakeRegistry {
    fn new(values: impl IntoIterator<Item = (&'static str, u32)>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().collect()),
        }
    }
}

impl PerformanceRegistryReader for FakeRegistry {
    fn read_dword(&self, value: &str) -> lctrl_core::Result<u32> {
        self.values
            .lock()
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, number)| *number)
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("missing test registry {value}"),
            })
    }
}

fn reader(version: u32, requested: u32, active: u32, automatic: u32, mask: u32) -> FakeRegistry {
    FakeRegistry::new([
        ("VERSION", version),
        ("CURRENT_SETTING", requested),
        ("CURRENT_STATE", active),
        ("AUTOMATIC_MODE_SETTING", automatic),
        ("POWER_SLIDER", mask),
    ])
}

#[derive(Default)]
struct FakeOverlay {
    scheme: Mutex<OverlayGuid>,
    writes: Mutex<Vec<OverlayGuid>>,
    ignore_writes: bool,
}

impl FakeOverlay {
    fn starting_at(scheme: OverlayGuid) -> Self {
        Self {
            scheme: Mutex::new(scheme),
            writes: Mutex::new(Vec::new()),
            ignore_writes: false,
        }
    }

    fn ignoring_writes(scheme: OverlayGuid) -> Self {
        Self {
            scheme: Mutex::new(scheme),
            writes: Mutex::new(Vec::new()),
            ignore_writes: true,
        }
    }

    fn writes(&self) -> Vec<OverlayGuid> {
        self.writes.lock().clone()
    }
}

impl PowerOverlayControl for FakeOverlay {
    fn actual(&self) -> lctrl_core::Result<OverlayGuid> {
        Ok(*self.scheme.lock())
    }

    fn effective(&self) -> lctrl_core::Result<OverlayGuid> {
        Ok(*self.scheme.lock())
    }

    fn set_active(&self, scheme: &OverlayGuid) -> lctrl_core::Result<()> {
        self.writes.lock().push(*scheme);
        if !self.ignore_writes {
            *self.scheme.lock() = *scheme;
        }
        Ok(())
    }
}

#[test]
fn state_uses_registry_requested_and_active_independently() {
    let controller = WindowsPerformanceP0::new(reader(0x3000, 1, 2, 1, 0x1b));
    let state = controller.performance_state().unwrap();

    assert_eq!(state.version, DispatcherVersion::V4);
    assert_eq!(state.requested, Some(PerformanceMode::Balanced));
    assert_eq!(state.active, Some(PerformanceMode::Quiet));
    assert!(state.automatic);
    assert!(
        state
            .capabilities
            .supports(PerformanceMode::Geek, state.version)
    );
}

#[test]
fn unknown_registry_modes_remain_unmapped_not_fabricated() {
    let controller = WindowsPerformanceP0::new(reader(0x2000, 99, 0, 0, 0));
    let state = controller.performance_state().unwrap();

    assert_eq!(state.version, DispatcherVersion::V3);
    assert_eq!(state.requested, None);
    assert_eq!(state.active, None);
    assert!(!state.automatic);
}

#[test]
fn portable_modes_map_to_their_os_overlay_scheme() {
    assert_eq!(
        overlay_for_mode(PerformanceMode::Balanced).unwrap(),
        OverlayGuid::BALANCED
    );
    assert_eq!(
        overlay_for_mode(PerformanceMode::Quiet).unwrap(),
        OverlayGuid::POWER_EFFICIENCY
    );
    assert_eq!(
        overlay_for_mode(PerformanceMode::Performance).unwrap(),
        OverlayGuid::PERFORMANCE
    );
    for guid in [
        OverlayGuid::BALANCED,
        OverlayGuid::POWER_EFFICIENCY,
        OverlayGuid::PERFORMANCE,
    ] {
        let mode = mode_for_overlay(&guid).expect("known scheme maps back");
        assert_eq!(overlay_for_mode(mode).unwrap(), guid);
    }
}

#[test]
fn vendor_only_modes_are_refused_instead_of_approximated() {
    for mode in [
        PerformanceMode::Geek,
        PerformanceMode::SilentHighPerformance,
        PerformanceMode::Custom,
    ] {
        let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
        let controller =
            WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);
        assert!(matches!(
            controller.set_performance_mode(mode, ApplyMode::Commit),
            Err(LctrlError::Unsupported { .. })
        ));
        assert!(overlay.writes().is_empty());
    }
}

#[test]
fn dry_run_reports_previous_without_touching_the_overlay() {
    let overlay = FakeOverlay::starting_at(OverlayGuid::POWER_EFFICIENCY);
    let controller = WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);

    let report = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.mode(), ApplyMode::DryRun);
    assert_eq!(*report.previous(), PerformanceMode::Quiet);
    assert_eq!(*report.requested(), PerformanceMode::Performance);
    assert!(report.actual().is_none());
    assert!(overlay.writes().is_empty());
}

#[test]
fn commit_writes_the_overlay_and_verifies_the_readback() {
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    let controller = WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);

    let report = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.mode(), ApplyMode::Commit);
    assert_eq!(*report.previous(), PerformanceMode::Balanced);
    assert_eq!(*report.requested(), PerformanceMode::Performance);
    assert_eq!(report.actual(), Some(&PerformanceMode::Performance));
    assert_eq!(overlay.writes(), vec![OverlayGuid::PERFORMANCE]);
}

#[test]
fn readback_mismatch_restores_the_previous_scheme() {
    let overlay = FakeOverlay::ignoring_writes(OverlayGuid::BALANCED);
    let controller = WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);

    let error = controller
        .set_performance_mode(PerformanceMode::Quiet, ApplyMode::Commit)
        .expect_err("a provider that ignores the write must not report success");

    assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
    assert_eq!(
        overlay.writes(),
        vec![OverlayGuid::POWER_EFFICIENCY, OverlayGuid::BALANCED]
    );
}

#[test]
fn an_unowned_current_scheme_is_refused_before_writing() {
    let custom = OverlayGuid {
        data1: 0x1234_5678,
        data2: 0,
        data3: 0,
        data4: [0; 8],
    };
    let overlay = FakeOverlay::starting_at(custom);
    let controller = WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);

    let error = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .expect_err("an unrollbackable scheme must be refused");

    assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    assert!(overlay.writes().is_empty());
}
