//! Native probe: the overlay API must resolve and report a scheme on any
//! supported Windows build. This exercises the real `powrprof.dll` path rather
//! than a fake, so a failure here is a real environment limitation.

#[test]
fn native_overlay_api_resolves_and_reports_a_scheme() {
    let overlay = lctrl_hal_win::NativePowerOverlay;
    let actual = lctrl_hal_win::PowerOverlayControl::actual(&overlay)
        .unwrap_or_else(|error| panic!("PowerGetActualOverlayScheme failed: {error}"));
    let effective = lctrl_hal_win::PowerOverlayControl::effective(&overlay)
        .unwrap_or_else(|error| panic!("PowerGetEffectiveOverlayScheme failed: {error}"));
    println!("actual overlay = {actual}");
    println!("effective overlay = {effective}");
    assert!(
        lctrl_hal_win::mode_for_overlay(&actual).is_some(),
        "actual overlay {actual} should be one of the schemes this build owns"
    );
}

#[test]
fn native_hal_records_the_perf_mode_capability() {
    use lctrl_hal::Hal;

    let hal =
        lctrl_hal_win::NativeWindowsHal::new(lctrl_hal_win::NativeWmi, lctrl_hal_win::NativeIoctl);
    let capabilities = hal.capabilities().expect("capabilities must evaluate");
    println!("perf.mode = {:?}", capabilities.get("perf.mode"));
    assert!(
        capabilities.get("perf.mode").is_some(),
        "the Windows HAL must record perf.mode"
    );
}
