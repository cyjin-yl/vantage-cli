use std::collections::BTreeMap;

use lctrl_core::{ApplyMode, DeviceState, LctrlError};
use lctrl_hal::{PrivacyControl, TouchpadControl};
use lctrl_hal_win::{
    AudioEndpointFormFactor, PnpDeviceProblem, PnpDeviceStatus, RuntimeDevice,
    RuntimeDeviceApi, RuntimeDeviceClass, RuntimeDeviceKind, WindowsRuntimeDevices,
    select_runtime_device,
};
use parking_lot::Mutex;

fn enabled() -> PnpDeviceStatus {
    PnpDeviceStatus {
        started: true,
        problem: None,
    }
}

fn disabled() -> PnpDeviceStatus {
    PnpDeviceStatus {
        started: false,
        problem: Some(PnpDeviceProblem::Disabled),
    }
}

fn camera(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::Imaging, id);
    device.friendly_name = Some("Integrated Camera".into());
    device.hardware_ids = vec!["USB\\VID_30C9&PID_1234".into()];
    device
}

fn microphone(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::AudioEndpoint, id);
    device.friendly_name = Some("Microphone Array".into());
    device.audio_form_factor = Some(AudioEndpointFormFactor::Microphone);
    device
}

fn touchpad(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::Mouse, id);
    device.friendly_name = Some("ELAN Precision Touchpad".into());
    device.hardware_ids = vec!["ACPI\\VEN_ELAN&DEV_1234".into()];
    device.compatible_ids = vec!["ACPI\\PNP0C50".into()];
    device
}

fn usb_mouse(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::Mouse, id);
    device.friendly_name = Some("USB Optical Mouse".into());
    device.hardware_ids = vec!["USB\\VID_046D&PID_C52B".into()];
    device
}

#[test]
fn camera_admission_requires_imaging_class_and_internal_identity() {
    let internal = camera("USB\\INTERNAL-CAMERA");
    let mut external = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "USB\\EXTERNAL-CAMERA");
    external.friendly_name = Some("USB Camera".into());
    let mut wrong_class = RuntimeDevice::new(RuntimeDeviceClass::Mouse, "HID\\NOT-A-CAMERA");
    wrong_class.friendly_name = Some("Integrated Camera".into());
    let mut magicbay = camera("USB\\MAGICBAY-CAMERA");
    magicbay.hardware_ids = vec!["USB\\VID_17EF&PID_7005&MI_01".into()];

    let selected = select_runtime_device(
        RuntimeDeviceKind::Camera,
        &[external, wrong_class, magicbay, internal.clone()],
    )
    .unwrap();

    assert_eq!(selected, internal);
}

#[test]
fn acpi_camera_identity_does_not_depend_on_a_friendly_name() {
    let mut sensor = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "ACPI\\INTC10A0\\0");
    sensor.description = Some("Camera Sensor".into());
    sensor.hardware_ids = vec!["ACPI\\INTC10A0".into()];

    let selected = select_runtime_device(RuntimeDeviceKind::Camera, &[sensor.clone()]).unwrap();

    assert_eq!(selected, sensor);
}

#[test]
fn camera_selection_rejects_missing_and_ambiguous_devices() {
    let mut external = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "USB\\EXTERNAL");
    external.friendly_name = Some("USB Webcam".into());
    assert!(matches!(
        select_runtime_device(RuntimeDeviceKind::Camera, &[external]),
        Err(LctrlError::Unsupported { .. })
    ));

    assert!(matches!(
        select_runtime_device(
            RuntimeDeviceKind::Camera,
            &[camera("USB\\CAMERA-A"), camera("USB\\CAMERA-B")]
        ),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn selection_rejects_empty_or_nul_terminated_instance_ids() {
    for invalid in [camera(""), camera("USB\\VALID-PREFIX\0\\ROOT-ALIAS")] {
        assert!(matches!(
            select_runtime_device(RuntimeDeviceKind::Camera, &[invalid]),
            Err(LctrlError::Unsupported { .. })
        ));
    }
}

#[test]
fn microphone_admission_uses_audio_endpoint_form_factor() {
    let mic = microphone("SWD\\MMDEVAPI\\MIC");
    let mut speakers = RuntimeDevice::new(RuntimeDeviceClass::AudioEndpoint, "SWD\\MMDEVAPI\\SPK");
    speakers.friendly_name = Some("Speakers".into());
    speakers.audio_form_factor = Some(AudioEndpointFormFactor::Other(1));
    let mut unknown = RuntimeDevice::new(RuntimeDeviceClass::AudioEndpoint, "SWD\\MMDEVAPI\\UNKNOWN");
    unknown.friendly_name = Some("Microphone Array".into());
    let mut wrong_class = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "ROOT\\NOT-A-MIC");
    wrong_class.audio_form_factor = Some(AudioEndpointFormFactor::Microphone);

    let selected = select_runtime_device(
        RuntimeDeviceKind::Microphone,
        &[speakers.clone(), unknown, wrong_class, mic.clone()],
    )
    .unwrap();
    assert_eq!(selected, mic);

    assert!(matches!(
        select_runtime_device(RuntimeDeviceKind::Microphone, &[speakers]),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(matches!(
        select_runtime_device(
            RuntimeDeviceKind::Microphone,
            &[microphone("SWD\\MIC-A"), microphone("SWD\\MIC-B")]
        ),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn touchpad_admission_requires_mouse_class_and_touchpad_evidence() {
    let pad = touchpad("ACPI\\ELAN-TOUCHPAD");
    let mut wrong_class = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "ACPI\\NOT-A-PAD");
    wrong_class.compatible_ids = vec!["*PNP0C50".into()];

    let selected = select_runtime_device(
        RuntimeDeviceKind::Touchpad,
        &[usb_mouse("USB\\MOUSE"), wrong_class, pad.clone()],
    )
    .unwrap();
    assert_eq!(selected, pad);

    assert!(matches!(
        select_runtime_device(RuntimeDeviceKind::Touchpad, &[usb_mouse("USB\\ONLY-MOUSE")]),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(matches!(
        select_runtime_device(
            RuntimeDeviceKind::Touchpad,
            &[touchpad("ACPI\\PAD-A"), touchpad("ACPI\\PAD-B")]
        ),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn hid_touchpad_usage_and_explicit_touchpad_names_are_positive_evidence() {
    let mut hid = RuntimeDevice::new(RuntimeDeviceClass::Mouse, "HID\\TOUCHPAD-USAGE");
    hid.hardware_ids = vec!["HID_DEVICE_UP:000D_U:0005".into()];
    assert_eq!(
        select_runtime_device(RuntimeDeviceKind::Touchpad, &[hid.clone()]).unwrap(),
        hid
    );

    let mut named = RuntimeDevice::new(RuntimeDeviceClass::Mouse, "HID\\NAMED-TOUCHPAD");
    named.friendly_name = Some("HID-compliant touch pad".into());
    assert_eq!(
        select_runtime_device(RuntimeDeviceKind::Touchpad, &[named.clone()]).unwrap(),
        named
    );
}

#[test]
fn pnp_state_requires_public_started_or_disabled_evidence() {
    assert_eq!(
        enabled().state(RuntimeDeviceKind::Camera).unwrap(),
        DeviceState::Enabled
    );
    assert_eq!(
        disabled().state(RuntimeDeviceKind::Camera).unwrap(),
        DeviceState::Disabled
    );

    for status in [
        PnpDeviceStatus {
            started: false,
            problem: None,
        },
        PnpDeviceStatus {
            started: false,
            problem: Some(PnpDeviceProblem::Other(29)),
        },
        PnpDeviceStatus {
            started: true,
            problem: Some(PnpDeviceProblem::Other(10)),
        },
    ] {
        assert!(matches!(
            status.state(RuntimeDeviceKind::Camera),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }
}

#[derive(Clone, Copy, Debug)]
enum FakeError {
    Permission,
    Channel(&'static str),
    Rejected(&'static str),
}

impl FakeError {
    fn into_lctrl(self) -> LctrlError {
        match self {
            Self::Permission => LctrlError::PermissionDenied {
                need: "administrator".into(),
            },
            Self::Channel(message) => LctrlError::ChannelUnavailable {
                channel: message.into(),
            },
            Self::Rejected(message) => LctrlError::FirmwareRejected {
                detail: message.into(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Call {
    Devices(RuntimeDeviceClass),
    Status(String),
    Write(String, DeviceState),
}

struct FakeRuntimeDeviceApi {
    devices: Vec<RuntimeDevice>,
    status: Mutex<PnpDeviceStatus>,
    status_calls: Mutex<usize>,
    write_calls: Mutex<usize>,
    status_errors: BTreeMap<usize, FakeError>,
    write_errors: BTreeMap<usize, FakeError>,
    write_effects: BTreeMap<usize, bool>,
    calls: Mutex<Vec<Call>>,
}

impl FakeRuntimeDeviceApi {
    fn new(devices: Vec<RuntimeDevice>) -> Self {
        Self {
            devices,
            status: Mutex::new(enabled()),
            status_calls: Mutex::new(0),
            write_calls: Mutex::new(0),
            status_errors: BTreeMap::new(),
            write_errors: BTreeMap::new(),
            write_effects: BTreeMap::new(),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl RuntimeDeviceApi for FakeRuntimeDeviceApi {
    fn devices(&self, class: RuntimeDeviceClass) -> lctrl_core::Result<Vec<RuntimeDevice>> {
        self.calls.lock().push(Call::Devices(class));
        Ok(self
            .devices
            .iter()
            .filter(|device| device.class == class)
            .cloned()
            .collect())
    }

    fn status(&self, instance_id: &str) -> lctrl_core::Result<PnpDeviceStatus> {
        self.calls.lock().push(Call::Status(instance_id.into()));
        let mut count = self.status_calls.lock();
        *count += 1;
        if let Some(error) = self.status_errors.get(&*count) {
            return Err(error.into_lctrl());
        }
        Ok(*self.status.lock())
    }

    fn set_state(&self, instance_id: &str, state: DeviceState) -> lctrl_core::Result<()> {
        self.calls
            .lock()
            .push(Call::Write(instance_id.into(), state));
        let mut count = self.write_calls.lock();
        *count += 1;
        let apply = self.write_effects.get(&*count).copied().unwrap_or(true);
        if apply {
            *self.status.lock() = match state {
                DeviceState::Enabled => enabled(),
                DeviceState::Disabled => disabled(),
            };
        }
        if let Some(error) = self.write_errors.get(&*count) {
            return Err(error.into_lctrl());
        }
        Ok(())
    }
}

#[test]
fn every_dry_run_entry_point_reads_without_any_device_write() {
    let api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA"), microphone("MIC"), touchpad("PAD")]);
    let controller = WindowsRuntimeDevices::new(api);

    let camera_report = controller
        .set_camera(DeviceState::Disabled, ApplyMode::DryRun)
        .unwrap();
    let microphone_report = controller
        .set_microphone(DeviceState::Disabled, ApplyMode::DryRun)
        .unwrap();
    let touchpad_report = controller
        .set_touchpad(DeviceState::Disabled, ApplyMode::DryRun)
        .unwrap();

    for report in [camera_report, microphone_report, touchpad_report] {
        assert_eq!(report.previous(), &DeviceState::Enabled);
        assert_eq!(report.requested(), &DeviceState::Disabled);
        assert_eq!(report.actual(), None);
    }
    assert_eq!(
        &*controller.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
            Call::Devices(RuntimeDeviceClass::AudioEndpoint),
            Call::Status("MIC".into()),
            Call::Devices(RuntimeDeviceClass::Mouse),
            Call::Status("PAD".into()),
        ]
    );
    assert_eq!(*controller.api().status.lock(), enabled());
}

#[test]
fn commit_writes_the_admitted_device_and_independently_reads_back() {
    let controller = WindowsRuntimeDevices::new(FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]));

    let report = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.previous(), &DeviceState::Enabled);
    assert_eq!(report.requested(), &DeviceState::Disabled);
    assert_eq!(report.actual(), Some(&DeviceState::Disabled));
    assert_eq!(*controller.api().status.lock(), disabled());
    assert_eq!(
        &*controller.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Disabled),
            Call::Status("CAMERA".into()),
        ]
    );
}

#[test]
fn touchpad_and_microphone_methods_route_to_their_own_classes() {
    let api = FakeRuntimeDeviceApi::new(vec![microphone("MIC"), touchpad("PAD")]);
    let controller = WindowsRuntimeDevices::new(api);

    controller
        .set_microphone(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();
    controller
        .set_touchpad(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();

    let calls = controller.api().calls.lock();
    assert!(calls.contains(&Call::Devices(RuntimeDeviceClass::AudioEndpoint)));
    assert!(calls.contains(&Call::Devices(RuntimeDeviceClass::Mouse)));
    assert!(calls.contains(&Call::Write("MIC".into(), DeviceState::Disabled)));
    assert!(calls.contains(&Call::Write("PAD".into(), DeviceState::Disabled)));
}

#[test]
fn selection_and_initial_read_failures_never_attempt_a_write() {
    let missing = WindowsRuntimeDevices::new(FakeRuntimeDeviceApi::new(vec![usb_mouse("MOUSE")]));
    assert!(matches!(
        missing.set_camera(DeviceState::Disabled, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
    assert_eq!(
        &*missing.api().calls.lock(),
        &[Call::Devices(RuntimeDeviceClass::Imaging)]
    );

    let ambiguous = WindowsRuntimeDevices::new(FakeRuntimeDeviceApi::new(vec![
        camera("CAMERA-A"),
        camera("CAMERA-B"),
    ]));
    assert!(matches!(
        ambiguous.set_camera(DeviceState::Disabled, ApplyMode::Commit),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert_eq!(
        &*ambiguous.api().calls.lock(),
        &[Call::Devices(RuntimeDeviceClass::Imaging)]
    );

    let mut api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]);
    api.status_errors.insert(1, FakeError::Channel("initial-read"));
    let unreadable = WindowsRuntimeDevices::new(api);
    assert!(matches!(
        unreadable.set_camera(DeviceState::Disabled, ApplyMode::Commit),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert_eq!(
        &*unreadable.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
        ]
    );
}

#[test]
fn prewrite_permission_failure_is_primary_and_does_not_trigger_rollback() {
    let mut api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]);
    api.write_effects.insert(1, false);
    api.write_errors.insert(1, FakeError::Permission);
    let controller = WindowsRuntimeDevices::new(api);

    assert!(matches!(
        controller.set_camera(DeviceState::Disabled, ApplyMode::Commit),
        Err(LctrlError::PermissionDenied { .. })
    ));
    assert_eq!(*controller.api().status.lock(), enabled());
    assert_eq!(
        &*controller.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Disabled),
        ]
    );
}

#[test]
fn readback_failure_restores_previous_state_and_preserves_primary_error() {
    let mut api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]);
    api.status_errors
        .insert(2, FakeError::Channel("primary-readback"));
    let controller = WindowsRuntimeDevices::new(api);

    let error = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(
        error,
        LctrlError::ChannelUnavailable { ref channel } if channel == "primary-readback"
    ));
    assert_eq!(*controller.api().status.lock(), enabled());
    assert_eq!(
        &*controller.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Disabled),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Enabled),
            Call::Status("CAMERA".into()),
        ]
    );
}

#[test]
fn rollback_failure_reports_both_errors_without_hiding_the_primary() {
    let mut api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]);
    api.status_errors
        .insert(2, FakeError::Channel("primary-readback"));
    api.write_errors
        .insert(2, FakeError::Rejected("rollback-write"));
    api.write_effects.insert(2, false);
    let controller = WindowsRuntimeDevices::new(api);

    let error = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap_err();

    let LctrlError::FirmwareRejected { detail } = error else {
        panic!("expected combined mutation/rollback failure");
    };
    assert!(detail.contains("primary-readback"), "{detail}");
    assert!(detail.contains("rollback-write"), "{detail}");
    assert_eq!(*controller.api().status.lock(), disabled());
    assert_eq!(
        &*controller.api().calls.lock(),
        &[
            Call::Devices(RuntimeDeviceClass::Imaging),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Disabled),
            Call::Status("CAMERA".into()),
            Call::Write("CAMERA".into(), DeviceState::Enabled),
        ]
    );
}

#[test]
fn readback_mismatch_also_rolls_back() {
    let mut api = FakeRuntimeDeviceApi::new(vec![camera("CAMERA")]);
    // The first write reports success but deliberately leaves the observed
    // state unchanged, forcing bounded readback to detect a mismatch.
    api.write_effects.insert(1, false);
    let controller = WindowsRuntimeDevices::new(api);

    assert!(matches!(
        controller.set_camera(DeviceState::Disabled, ApplyMode::Commit),
        Err(LctrlError::VerifyMismatch { .. })
    ));

    assert_eq!(*controller.api().status.lock(), enabled());
    assert_eq!(*controller.api().status_calls.lock(), 12);
    let writes: Vec<_> = controller
        .api()
        .calls
        .lock()
        .iter()
        .filter_map(|call| match call {
            Call::Write(id, state) => Some((id.clone(), *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        writes,
        vec![
            ("CAMERA".into(), DeviceState::Disabled),
            ("CAMERA".into(), DeviceState::Enabled),
        ]
    );
}
