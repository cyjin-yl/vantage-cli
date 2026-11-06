#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
//! Read-only checks for the pinned Lenovo Fn OSD contract
//! (`docs/evidence/re-osd-20261105.md`). No test here sends a service control
//! code: probing and dry-runs must not change machine state.

use lctrl_hal::{OsdControl, OsdIndicatorState};

#[test]
fn fn_osd_status_reports_the_pinned_contract() {
    let osd = lctrl_hal_win::NativeOsd::new();
    let status = osd
        .fn_osd()
        .unwrap_or_else(|error| panic!("reading the Fn OSD preference is read-only: {error}"));

    assert_eq!(status.service, "LenovoFnAndFunctionKeys");
    assert_eq!(status.capslock.indicator, "capslock");
    assert_eq!(status.capslock.value_name, "ShowCapslkOSD");
    assert_eq!(
        (
            status.capslock.enable_control,
            status.capslock.disable_control
        ),
        (146, 147)
    );
    assert_eq!(status.numlock.indicator, "numlock");
    assert_eq!(status.numlock.value_name, "ShowNumlkOSD");
    assert_eq!(
        (
            status.numlock.enable_control,
            status.numlock.disable_control
        ),
        (148, 149)
    );

    // A supported indicator carries the value it was read from; an unsupported
    // one must say which gate suppressed it instead of inventing a state.
    for indicator in [&status.capslock, &status.numlock] {
        assert!(
            !indicator.detail.is_empty(),
            "{} must explain its state",
            indicator.indicator
        );
        match indicator.state {
            OsdIndicatorState::On | OsdIndicatorState::Off => {
                assert!(
                    indicator.registry_value.is_some(),
                    "{} reports {:?} without a registry value",
                    indicator.indicator,
                    indicator.state
                );
            }
            // `not_supported` may still carry the raw value (for example a
            // NumLk preference present on a machine whose
            // `Capbility\SupportNumlkKey` is 0); the detail names the gate.
            OsdIndicatorState::NotSupported => {}
        }
    }
}

#[test]
fn fn_osd_dry_run_sends_nothing() {
    let channel = lctrl_hal_win::NativeFnOsd::new();
    let change = channel
        .set(true, true)
        .unwrap_or_else(|error| panic!("a dry-run only reads the preference: {error}"));
    assert_eq!(change.mode, "dry_run");
    assert_eq!(change.requested, OsdIndicatorState::On);
    assert_eq!(change.before, change.after);
}
