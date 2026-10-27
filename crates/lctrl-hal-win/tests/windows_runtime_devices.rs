use std::collections::BTreeMap;

use lctrl_core::{ApplyMode, DeviceState, LctrlError};
use lctrl_hal::{PrivacyControl, TouchpadControl};
use lctrl_hal_win::{
    AudioEndpointFormFactor, PnpDeviceProblem, PnpDeviceStatus, RuntimeDevice, RuntimeDeviceApi,
    RuntimeDeviceClass, RuntimeDeviceKind, WindowsRuntimeDevices, select_runtime_device,
    select_runtime_devices,
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
    device.parent_instance_id = Some("USB\\VID_30C9&PID_1234\\0001".into());
    device
}

fn microphone(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::AudioEndpoint, id);
    device.friendly_name = Some("Microphone Array".into());
    device.audio_form_factor = Some(AudioEndpointFormFactor::Microphone);
    device.parent_instance_id = Some("INTELAUDIO\\CTLR_DEV_E428\\5&0001".into());
    device
}

fn touchpad(id: &str) -> RuntimeDevice {
    let mut device = RuntimeDevice::new(RuntimeDeviceClass::HidClass, id);
    device.hardware_ids = vec!["HID_DEVICE_UP:000D_U:0005".into()];
    device.parent_instance_id = Some("ACPI\\PAD\\0".into());
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
    let mut unknown =
        RuntimeDevice::new(RuntimeDeviceClass::AudioEndpoint, "SWD\\MMDEVAPI\\UNKNOWN");
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
fn microphone_admission_requires_an_integrated_audio_bus_parent() {
    let integrated = microphone("SWD\\MMDEVAPI\\INTEGRATED");
    let mut virtual_endpoint = microphone("SWD\\MMDEVAPI\\VIRTUAL");
    virtual_endpoint.parent_instance_id = Some("ROOT\\MEDIA\\0000".into());
    let mut hd_audio = microphone("SWD\\MMDEVAPI\\HDAUDIO");
    hd_audio.parent_instance_id = Some("HDAUDIO\\FUNC_01&VEN_10EC\\5&0".into());
    let mut no_parent = microphone("SWD\\MMDEVAPI\\NO-PARENT");
    no_parent.parent_instance_id = None;

    assert_eq!(
        select_runtime_device(
            RuntimeDeviceKind::Microphone,
            &[
                virtual_endpoint.clone(),
                no_parent.clone(),
                integrated.clone()
            ]
        )
        .unwrap(),
        integrated
    );
    assert_eq!(
        select_runtime_device(RuntimeDeviceKind::Microphone, &[hd_audio.clone()]).unwrap(),
        hd_audio
    );
    assert!(matches!(
        select_runtime_device(
            RuntimeDeviceKind::Microphone,
            &[virtual_endpoint, no_parent]
        ),
        Err(LctrlError::Unsupported { .. })
    ));
}

#[test]
fn camera_group_selection_accepts_rgb_and_ir_devnodes_of_one_parent() {
    let mut ir = camera("USB\\INTERNAL-CAMERA&MI_02");
    ir.friendly_name = Some("Integrated IR Camera".into());
    let rgb = camera("USB\\INTERNAL-CAMERA&MI_00");

    let group =
        select_runtime_devices(RuntimeDeviceKind::Camera, &[rgb.clone(), ir.clone()]).unwrap();

    assert_eq!(group, vec![rgb, ir]);
}

#[test]
fn camera_group_selection_refuses_two_physical_cameras() {
    let first = camera("USB\\CAMERA-A");
    let mut second = camera("USB\\CAMERA-B");
    second.parent_instance_id = Some("USB\\VID_30C9&PID_1234\\0002".into());

    assert!(matches!(
        select_runtime_devices(RuntimeDeviceKind::Camera, &[first, second]),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn camera_group_selection_refuses_missing_parent_identity() {
    let mut first = camera("USB\\CAMERA-A");
    first.parent_instance_id = None;
    let second = camera("USB\\CAMERA-B");

    assert!(matches!(
        select_runtime_devices(RuntimeDeviceKind::Camera, &[first, second]),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn touchpad_admission_requires_digitizer_usage_and_integrated_parent() {
    let pad = touchpad("HID\\PAD&COL02");
    let mut wrong_class = RuntimeDevice::new(RuntimeDeviceClass::Imaging, "ACPI\\NOT-A-PAD");
    wrong_class.hardware_ids = vec!["HID_DEVICE_UP:000D_U:0005".into()];

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
            &[touchpad("HID\\PAD-A"), touchpad("HID\\PAD-B")]
        ),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn controller_names_and_acpi_parentage_are_not_touchpad_input_evidence() {
    let mut named = RuntimeDevice::new(RuntimeDeviceClass::HidClass, "HID\\NAMED-TOUCHPAD");
    named.friendly_name = Some("HID-compliant touch pad".into());
    named.parent_instance_id = Some("ACPI\\PAD\\0".into());
    let mut controller = touchpad("ACPI\\PAD\\0");
    controller.hardware_ids = vec!["ACPI\\PNP0C50".into()];
    let mut external = touchpad("HID\\EXTERNAL-PAD");
    external.parent_instance_id = Some("USB\\EXTERNAL-PAD".into());
    let mut acpi_mouse = usb_mouse("HID\\ACPI-MOUSE");
    acpi_mouse.parent_instance_id = Some("ACPI\\OTHER-DEVICE\\0".into());
    for device in [named, controller, external, acpi_mouse] {
        assert!(matches!(
            select_runtime_device(RuntimeDeviceKind::Touchpad, &[device]),
            Err(LctrlError::Unsupported { .. })
        ));
    }
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
    let api = FakeRuntimeDeviceApi::new(vec![
        camera("CAMERA"),
        microphone("MIC"),
        touchpad("HID\\PAD"),
    ]);
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
    assert_eq!(*controller.api().write_calls.lock(), 0);
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

    let mut second_camera = camera("CAMERA-B");
    second_camera.parent_instance_id = Some("USB\\VID_30C9&PID_1234\\0002".into());
    let ambiguous = WindowsRuntimeDevices::new(FakeRuntimeDeviceApi::new(vec![
        camera("CAMERA-A"),
        second_camera,
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
    api.status_errors
        .insert(1, FakeError::Channel("initial-read"));
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

/// Per-devnode state so the RGB+IR camera group can be exercised: the shared
/// status fake cannot express a split group.
struct FakeGroupApi {
    devices: Vec<RuntimeDevice>,
    states: Mutex<BTreeMap<String, DeviceState>>,
    writes: Mutex<Vec<(String, DeviceState)>>,
    fail_write_at: Option<(usize, FakeError)>,
    failed_write_takes_effect: bool,
    write_calls: Mutex<usize>,
}

impl FakeGroupApi {
    fn new(devices: Vec<RuntimeDevice>, states: &[(&str, DeviceState)]) -> Self {
        Self {
            devices,
            states: Mutex::new(
                states
                    .iter()
                    .map(|(id, state)| ((*id).to_owned(), *state))
                    .collect(),
            ),
            writes: Mutex::new(Vec::new()),
            fail_write_at: None,
            failed_write_takes_effect: false,
            write_calls: Mutex::new(0),
        }
    }

    fn with_failure(mut self, call: usize, error: FakeError) -> Self {
        self.fail_write_at = Some((call, error));
        self
    }
}

impl RuntimeDeviceApi for FakeGroupApi {
    fn devices(&self, class: RuntimeDeviceClass) -> lctrl_core::Result<Vec<RuntimeDevice>> {
        let states = self.states.lock();
        Ok(self
            .devices
            .iter()
            .filter(|device| device.class == class)
            .filter(|device| {
                device
                    .parent_instance_id
                    .as_deref()
                    .is_none_or(|parent| states.get(parent) != Some(&DeviceState::Disabled))
            })
            .cloned()
            .collect())
    }

    fn status(&self, instance_id: &str) -> lctrl_core::Result<PnpDeviceStatus> {
        match self.states.lock().get(instance_id) {
            Some(DeviceState::Enabled) => Ok(enabled()),
            Some(DeviceState::Disabled) => Ok(disabled()),
            None => Err(LctrlError::ChannelUnavailable {
                channel: format!("fake group has no status for {instance_id}"),
            }),
        }
    }

    fn set_state(&self, instance_id: &str, state: DeviceState) -> lctrl_core::Result<()> {
        let mut count = self.write_calls.lock();
        *count += 1;
        self.writes.lock().push((instance_id.to_owned(), state));
        if let Some((at, error)) = self.fail_write_at {
            if at == *count {
                if self.failed_write_takes_effect {
                    self.states.lock().insert(instance_id.to_owned(), state);
                }
                return Err(error.into_lctrl());
            }
        }
        self.states.lock().insert(instance_id.to_owned(), state);
        Ok(())
    }
}

impl RuntimeDeviceApi for &FakeGroupApi {
    fn devices(&self, class: RuntimeDeviceClass) -> lctrl_core::Result<Vec<RuntimeDevice>> {
        (**self).devices(class)
    }

    fn status(&self, instance_id: &str) -> lctrl_core::Result<PnpDeviceStatus> {
        (**self).status(instance_id)
    }

    fn set_state(&self, instance_id: &str, state: DeviceState) -> lctrl_core::Result<()> {
        (**self).set_state(instance_id, state)
    }
}

fn touchpad_topology() -> FakeGroupApi {
    let mut controller = RuntimeDevice::new(RuntimeDeviceClass::HidClass, "ACPI\\PAD\\0");
    controller.hardware_ids = vec!["ACPI\\PNP0C50".into()];
    let digitizer = touchpad("HID\\PAD&COL02");
    let mut mouse = RuntimeDevice::new(RuntimeDeviceClass::Mouse, "HID\\PAD&COL01");
    mouse.hardware_ids = vec!["HID_DEVICE_UP:0001_U:0002".into()];
    mouse.parent_instance_id = digitizer.parent_instance_id.clone();
    let mut configuration = RuntimeDevice::new(RuntimeDeviceClass::HidClass, "HID\\PAD&COL03");
    configuration.parent_instance_id = digitizer.parent_instance_id.clone();
    let mut vendor = RuntimeDevice::new(RuntimeDeviceClass::HidClass, "HID\\PAD&COL04");
    vendor.parent_instance_id = digitizer.parent_instance_id.clone();
    let mut other_mouse = usb_mouse("HID\\OTHER-MOUSE");
    other_mouse.hardware_ids = vec!["HID_DEVICE_UP:0001_U:0002".into()];
    other_mouse.parent_instance_id = Some("ACPI\\OTHER-DEVICE\\0".into());
    FakeGroupApi::new(
        vec![
            controller,
            digitizer,
            mouse,
            configuration,
            vendor,
            other_mouse,
        ],
        &[
            ("ACPI\\PAD\\0", DeviceState::Enabled),
            ("HID\\PAD&COL01", DeviceState::Enabled),
            ("HID\\PAD&COL02", DeviceState::Enabled),
            ("HID\\PAD&COL03", DeviceState::Enabled),
            ("HID\\PAD&COL04", DeviceState::Enabled),
            ("HID\\OTHER-MOUSE", DeviceState::Enabled),
        ],
    )
}

#[test]
fn touchpad_input_toggle_preserves_controller_and_can_be_restored_by_a_fresh_client() {
    let api = touchpad_topology();
    let original = api.states.lock().clone();
    let mut expected_disabled = original.clone();
    expected_disabled.insert("HID\\PAD&COL01".into(), DeviceState::Disabled);
    expected_disabled.insert("HID\\PAD&COL02".into(), DeviceState::Disabled);

    let report = WindowsRuntimeDevices::new(&api)
        .set_touchpad(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&DeviceState::Disabled));
    assert_eq!(*api.states.lock(), expected_disabled);

    let fresh_client = WindowsRuntimeDevices::new(&api);
    assert_eq!(
        fresh_client.touchpad_state().unwrap(),
        DeviceState::Disabled
    );
    let report = fresh_client
        .set_touchpad(DeviceState::Enabled, ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&DeviceState::Enabled));
    assert_eq!(*api.states.lock(), original);
}

#[test]
fn touchpad_dry_run_does_not_change_any_input_or_controller_state() {
    let api = touchpad_topology();
    let original = api.states.lock().clone();
    let report = WindowsRuntimeDevices::new(&api)
        .set_touchpad(DeviceState::Disabled, ApplyMode::DryRun)
        .unwrap();
    assert_eq!(*report.previous(), DeviceState::Enabled);
    assert_eq!(*report.requested(), DeviceState::Disabled);
    assert_eq!(report.actual(), None);
    assert_eq!(*api.states.lock(), original);
    assert!(api.writes.lock().is_empty());
}

#[test]
fn enabling_an_already_enabled_touchpad_does_not_restart_input_devices() {
    let api = touchpad_topology();
    let original = api.states.lock().clone();
    let report = WindowsRuntimeDevices::new(&api)
        .set_touchpad(DeviceState::Enabled, ApplyMode::Commit)
        .unwrap();
    assert_eq!(*report.previous(), DeviceState::Enabled);
    assert_eq!(report.actual(), Some(&DeviceState::Enabled));
    assert_eq!(*api.states.lock(), original);
    assert!(api.writes.lock().is_empty());
}

#[test]
fn touchpad_failure_after_effect_restores_both_attempted_leaves() {
    let mut api = touchpad_topology().with_failure(2, FakeError::Rejected("failed after effect"));
    api.failed_write_takes_effect = true;
    let original = api.states.lock().clone();
    let error = WindowsRuntimeDevices::new(&api)
        .set_touchpad(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { ref detail } if detail == "failed after effect"
    ));
    assert_eq!(*api.states.lock(), original);
}

fn camera_pair() -> (RuntimeDevice, RuntimeDevice) {
    let rgb = camera("USB\\CAM&MI_00");
    let mut ir = camera("USB\\CAM&MI_02");
    ir.friendly_name = Some("Integrated IR Camera".into());
    (rgb, ir)
}

fn pair_api(states: &[(&str, DeviceState)]) -> FakeGroupApi {
    let (rgb, ir) = camera_pair();
    FakeGroupApi::new(vec![rgb, ir], states)
}

#[test]
fn camera_group_toggle_moves_every_devnode_and_reads_back() {
    let controller = WindowsRuntimeDevices::new(pair_api(&[
        ("USB\\CAM&MI_00", DeviceState::Enabled),
        ("USB\\CAM&MI_02", DeviceState::Enabled),
    ]));

    let report = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap();

    assert_eq!(*report.previous(), DeviceState::Enabled);
    assert_eq!(report.actual(), Some(&DeviceState::Disabled));
    assert_eq!(
        *controller.api().writes.lock(),
        vec![
            ("USB\\CAM&MI_00".to_owned(), DeviceState::Disabled),
            ("USB\\CAM&MI_02".to_owned(), DeviceState::Disabled),
        ]
    );
    assert_eq!(
        controller.camera_group_state().unwrap(),
        DeviceState::Disabled
    );
}

#[test]
fn camera_group_dry_run_never_writes_a_devnode() {
    let controller = WindowsRuntimeDevices::new(pair_api(&[
        ("USB\\CAM&MI_00", DeviceState::Enabled),
        ("USB\\CAM&MI_02", DeviceState::Enabled),
    ]));

    let report = controller
        .set_camera(DeviceState::Disabled, ApplyMode::DryRun)
        .unwrap();

    assert_eq!(*report.previous(), DeviceState::Enabled);
    assert!(controller.api().writes.lock().is_empty());
}

#[test]
fn camera_group_split_state_is_not_reported_as_one_answer() {
    let controller = WindowsRuntimeDevices::new(pair_api(&[
        ("USB\\CAM&MI_00", DeviceState::Enabled),
        ("USB\\CAM&MI_02", DeviceState::Disabled),
    ]));

    assert!(matches!(
        controller.camera_group_state(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn camera_group_failure_restores_every_changed_devnode() {
    let controller = WindowsRuntimeDevices::new(
        pair_api(&[
            ("USB\\CAM&MI_00", DeviceState::Enabled),
            ("USB\\CAM&MI_02", DeviceState::Enabled),
        ])
        .with_failure(2, FakeError::Rejected("second devnode refused")),
    );

    let error = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap_err();

    assert!(
        matches!(&error, LctrlError::FirmwareRejected { detail } if detail.contains("second devnode refused")),
        "unexpected error {error}"
    );
    assert_eq!(
        controller.camera_group_state().unwrap(),
        DeviceState::Enabled
    );
    assert!(
        controller
            .api()
            .states
            .lock()
            .values()
            .all(|state| *state == DeviceState::Enabled)
    );
}

#[test]
fn camera_group_prewrite_permission_error_keeps_the_primary_error() {
    let controller = WindowsRuntimeDevices::new(
        pair_api(&[
            ("USB\\CAM&MI_00", DeviceState::Enabled),
            ("USB\\CAM&MI_02", DeviceState::Enabled),
        ])
        .with_failure(1, FakeError::Permission),
    );

    let error = controller
        .set_camera(DeviceState::Disabled, ApplyMode::Commit)
        .unwrap_err();

    assert!(matches!(error, LctrlError::PermissionDenied { .. }));
    assert_eq!(controller.api().writes.lock().len(), 1);
}
