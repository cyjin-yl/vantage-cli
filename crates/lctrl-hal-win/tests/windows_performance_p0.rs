use parking_lot::Mutex;

use lctrl_core::{ApplyMode, DispatcherVersion, LctrlError, PerformanceMode};
use lctrl_hal::PerformanceControl;
use lctrl_hal_win::{PerformanceRegistryReader, WindowsPerformanceP0};

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
fn mode_set_is_unsupported_without_verified_versioned_scm_code() {
    let controller = WindowsPerformanceP0::new(reader(0x3000, 1, 1, 0, 0x1b));

    assert!(matches!(
        controller.set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit),
        Err(LctrlError::Unsupported { feature }) if feature == "perf.mode.set"
    ));
}

#[test]
fn mode_dry_run_is_also_unsupported_without_an_executable_contract() {
    let controller = WindowsPerformanceP0::new(reader(0x3000, 1, 1, 0, 0x1b));

    assert!(matches!(
        controller.set_performance_mode(PerformanceMode::Quiet, ApplyMode::DryRun),
        Err(LctrlError::Unsupported { .. })
    ));
}
