#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use parking_lot::Mutex;

use lctrl_core::{ApplyMode, DispatcherVersion, LctrlError, PerformanceMode};
use lctrl_hal::PerformanceControl;
use lctrl_hal_win::{
    OverlayGuid, PerformanceRegistryReader, PowerOverlayControl, WindowsPerformanceP0,
    mode_for_overlay, overlay_for_mode,
};

struct FakeRegistry {
    values: Mutex<Vec<(&'static str, u32)>>,
    writes: Mutex<Vec<(&'static str, u32)>>,
    ignore_writes: bool,
}

impl FakeRegistry {
    fn new(values: impl IntoIterator<Item = (&'static str, u32)>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().collect()),
            writes: Mutex::new(Vec::new()),
            ignore_writes: false,
        }
    }

    fn ignoring_writes(values: impl IntoIterator<Item = (&'static str, u32)>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().collect()),
            writes: Mutex::new(Vec::new()),
            ignore_writes: true,
        }
    }

    fn writes(&self) -> Vec<(&'static str, u32)> {
        self.writes.lock().clone()
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

    fn write_dword(&self, value: &str, data: u32) -> lctrl_core::Result<()> {
        // Leak the key so the stored tuple can keep a 'static name only when
        // the caller passed one; read path looks up by the same literal.
        let name: &'static str = Box::leak(value.to_string().into_boxed_str());
        self.writes.lock().push((name, data));
        let mut values = self.values.lock();
        if self.ignore_writes {
            return Ok(());
        }
        if let Some(entry) = values.iter_mut().find(|(n, _)| *n == value) {
            entry.1 = data;
        } else {
            values.push((name, data));
        }
        Ok(())
    }
}

impl PerformanceRegistryReader for &FakeRegistry {
    fn read_dword(&self, value: &str) -> lctrl_core::Result<u32> {
        (**self).read_dword(value)
    }

    fn write_dword(&self, value: &str, data: u32) -> lctrl_core::Result<()> {
        (**self).write_dword(value, data)
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
fn its_schema_without_legacy_values_still_reports() {
    // The installed dispatcher on 21VG publishes the ITS-era names and no
    // VERSION/CURRENT_SETTING/CURRENT_STATE/POWER_SLIDER at all. The report
    // must not fail on the absent names, must decode the corroborated
    // 1:BSM / 3:EPM mapping, and must surface the raw values it cannot
    // attribute.
    let controller = WindowsPerformanceP0::new(FakeRegistry::new([
        ("ITS_CurrentSetting", 3),
        ("ITS_CurrentSettingV", 3),
        ("ITS_AutomaticModeSetting", 7),
        ("ITS_HW_Capability", 16),
        ("ITS_FN_Capability", 4122),
        ("ITS_FanMode", 4),
    ]));
    let state = controller.performance_state().unwrap();

    assert_eq!(state.requested, Some(PerformanceMode::Performance));
    assert_eq!(state.active, Some(PerformanceMode::Performance));
    assert_eq!(state.version, DispatcherVersion::Legacy(0));
    let its = state.its.expect("ITS values are carried raw");
    assert_eq!(its.current_setting, Some(3));
    assert_eq!(its.hw_capability, Some(16));
    assert_eq!(its.fn_capability, Some(4122));
    assert_eq!(its.fan_mode, Some(4));
}

#[test]
fn legacy_schema_still_reports_and_omits_the_its_block() {
    let controller = WindowsPerformanceP0::new(reader(0x3000, 1, 2, 1, 0x1b));
    let state = controller.performance_state().unwrap();

    assert_eq!(state.requested, Some(PerformanceMode::Balanced));
    assert_eq!(state.active, Some(PerformanceMode::Quiet));
    assert!(state.its.is_none(), "no ITS values were published");
}

#[test]
fn its_mode_outside_the_known_enum_is_not_coerced() {
    let controller = WindowsPerformanceP0::new(FakeRegistry::new([
        ("ITS_CurrentSetting", 9),
        ("ITS_CurrentSettingV", 0),
    ]));
    let state = controller.performance_state().unwrap();

    assert_eq!(state.requested, None);
    assert_eq!(state.active, None);
    assert_eq!(state.its.and_then(|its| its.current_setting), Some(9));
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

fn modern_registry(values: &[(&'static str, u32)]) -> FakeRegistry {
    FakeRegistry::new(
        // Overrides come first: `read_dword` returns the first matching entry,
        // so a duplicate name must have the override ahead of the base value.
        values.iter().copied().chain(
            [
                ("VERSION", 0x4000u32),
                ("ITS_CurrentSetting", 1),
                ("ITS_CurrentSettingV", 1),
                ("ITS_AutomaticModeSetting", 0),
                ("Policy_ModernITS", 0),
            ]
            .into_iter(),
        ),
    )
}

#[test]
fn dispatcher_owned_automatic_mode_writes_only_the_overlay() {
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    // AutomaticModeSetting == 7: the dispatcher chooses ITS_CurrentSetting; a
    // manual tier write would fight it, so only the overlay may change.
    let registry = modern_registry(&[("ITS_AutomaticModeSetting", 7)]);
    let controller = WindowsPerformanceP0::with_overlay(&registry, &overlay);

    let report = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&PerformanceMode::Performance));
    // Overlay committed; the ITS tier must not have been touched.
    let writes = registry.writes();
    assert!(
        writes.is_empty(),
        "auto-mode must not write ITS, got {writes:?}"
    );
}

#[test]
fn modern_its_policy_blocks_the_tier_write() {
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    let registry = modern_registry(&[("Policy_ModernITS", 1)]);
    let controller = WindowsPerformanceP0::with_overlay(&registry, &overlay);

    controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert!(registry.writes().is_empty());
}

#[test]
fn dispatcher_free_mode_writes_and_verifies_the_its_tier() {
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    let registry = modern_registry(&[]);
    let controller = WindowsPerformanceP0::with_overlay(&registry, &overlay);

    let report = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&PerformanceMode::Performance));
    let writes = registry.writes();
    assert_eq!(writes, vec![("ITS_CurrentSetting", 3)]);
}

#[test]
fn its_readback_mismatch_rolls_back_tier_and_overlay() {
    // A registry that accepts the write but never reflects it: the firmware
    // did not take the tier, so both the registry and overlay must restore.
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    let registry = FakeRegistry::ignoring_writes([
        ("VERSION", 0x4000u32),
        ("ITS_CurrentSetting", 1),
        ("ITS_CurrentSettingV", 1),
        ("ITS_AutomaticModeSetting", 0),
        ("Policy_ModernITS", 0),
    ]);
    let controller = WindowsPerformanceP0::with_overlay(&registry, &overlay);

    let error = controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .expect_err("an ITS readback that never confirms must roll back");

    assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
    // Overlay restored to the prior scheme.
    assert_eq!(
        overlay.writes(),
        vec![OverlayGuid::PERFORMANCE, OverlayGuid::BALANCED]
    );
}

#[test]
fn legacy_schema_never_fabricates_modern_its_keys() {
    // The shared `reader()` fixture has only CURRENT_* legacy names; the ITS
    // path must stay inert and the overlay change still reported.
    let overlay = FakeOverlay::starting_at(OverlayGuid::BALANCED);
    let controller = WindowsPerformanceP0::with_overlay(reader(0x3000, 1, 1, 0, 0x1b), &overlay);

    controller
        .set_performance_mode(PerformanceMode::Performance, ApplyMode::Commit)
        .unwrap();

    assert!(overlay.writes() == vec![OverlayGuid::PERFORMANCE]);
}
