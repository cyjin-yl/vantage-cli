//! Conservative Windows PnP runtime control for privacy devices and touchpads.
//!
//! `DeviceState::Disabled` in this module means that the selected device node
//! has `CM_PROB_DISABLED`. It does not mean that an EC privacy shutter, a
//! hardware microphone mute circuit, or a persistent BIOS switch is engaged.
//! Disabling a device node removes its runtime PnP function from the bus; the
//! selected instance ID is retained so the same device can be enabled again.

use std::{thread, time::Duration};

use lctrl_core::{ApplyMode, ChangeReport, DeviceState, LctrlError, PrivacyDeviceState, Result};
use lctrl_hal::{PrivacyControl, TouchpadControl, poll_readback};

const READBACK_ATTEMPTS: usize = 10;
const READBACK_DELAY: Duration = Duration::from_millis(50);
/// A restarted HID collection re-enumerates before it reports a state again,
/// so recovery polls for longer than a plain transition does.
const RECOVERY_READBACK_ATTEMPTS: usize = 40;
const MAX_TOUCHPAD_INPUT_LEAVES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDeviceClass {
    Imaging,
    AudioEndpoint,
    Mouse,
    HidClass,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDeviceKind {
    Camera,
    Microphone,
    Touchpad,
}

impl RuntimeDeviceKind {
    #[must_use]
    pub const fn class(self) -> RuntimeDeviceClass {
        match self {
            Self::Camera => RuntimeDeviceClass::Imaging,
            Self::Microphone => RuntimeDeviceClass::AudioEndpoint,
            Self::Touchpad => RuntimeDeviceClass::HidClass,
        }
    }

    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Self::Camera => "privacy.camera",
            Self::Microphone => "privacy.microphone",
            Self::Touchpad => "touchpad",
        }
    }
}

/// The subset of the public `EndpointFormFactor` values needed for admission.
/// Other values are retained so diagnostics never mistake an unknown value for
/// a microphone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioEndpointFormFactor {
    Microphone,
    Other(u32),
}

/// A device problem relevant to PnP state decoding. In particular, code 29
/// (`CM_PROB_HARDWARE_DISABLED`) remains `Other` rather than being mislabeled
/// as a software PnP disable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PnpDeviceProblem {
    Disabled,
    Other(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PnpDeviceStatus {
    pub started: bool,
    /// `CM_Get_DevNode_Status` defines a problem code only when
    /// `DN_HAS_PROBLEM` is set, hence the outer `Option`.
    pub problem: Option<PnpDeviceProblem>,
}

impl PnpDeviceStatus {
    pub fn state(self, kind: RuntimeDeviceKind) -> Result<DeviceState> {
        match self.problem {
            Some(PnpDeviceProblem::Disabled) => Ok(DeviceState::Disabled),
            None if self.started => Ok(DeviceState::Enabled),
            _ => Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{} has inconclusive PnP status (started={}, problem={:?})",
                    kind.feature(),
                    self.started,
                    self.problem
                ),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDevice {
    pub class: RuntimeDeviceClass,
    pub instance_id: String,
    pub hardware_ids: Vec<String>,
    pub compatible_ids: Vec<String>,
    pub description: Option<String>,
    pub friendly_name: Option<String>,
    pub bus_description: Option<String>,
    pub audio_form_factor: Option<AudioEndpointFormFactor>,
    /// Instance ID of the parent devnode, when the platform reports one. The
    /// parent separates integrated hardware (ACPI/INTELAUDIO) from virtual
    /// software devices (ROOT\...) without relying on localized names.
    pub parent_instance_id: Option<String>,
}

impl RuntimeDevice {
    #[must_use]
    pub fn new(class: RuntimeDeviceClass, instance_id: impl Into<String>) -> Self {
        Self {
            class,
            instance_id: instance_id.into(),
            hardware_ids: Vec::new(),
            compatible_ids: Vec::new(),
            description: None,
            friendly_name: None,
            bus_description: None,
            audio_form_factor: None,
            parent_instance_id: None,
        }
    }
}

/// Select exactly one device with positive identity evidence. A missing or
/// ambiguous result is an error; callers must never fall back to the first
/// device in a PnP class.
pub fn select_runtime_device(
    kind: RuntimeDeviceKind,
    devices: &[RuntimeDevice],
) -> Result<RuntimeDevice> {
    let mut selected: Option<&RuntimeDevice> = None;
    for device in devices.iter().filter(|device| admits_device(kind, device)) {
        if let Some(first) = selected {
            if !first.instance_id.eq_ignore_ascii_case(&device.instance_id) {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "{} PnP selection is ambiguous between {:?} and {:?}; refusing to mutate either device",
                        kind.feature(),
                        first.instance_id,
                        device.instance_id
                    ),
                });
            }
        } else {
            selected = Some(device);
        }
    }
    selected.cloned().ok_or_else(|| LctrlError::Unsupported {
        feature: format!(
            "{}.runtime_pnp (no uniquely identifiable {:?} device)",
            kind.feature(),
            kind.class()
        ),
    })
}

/// Select every devnode of one logical runtime device.
///
/// A camera is routinely exposed as more than one Imaging devnode: the reference
/// machine publishes an RGB and an IR collection under a single USB parent, and
/// both must move together for a privacy toggle to mean anything. Every other
/// kind stays a single-device selection.
pub fn select_runtime_devices(
    kind: RuntimeDeviceKind,
    devices: &[RuntimeDevice],
) -> Result<Vec<RuntimeDevice>> {
    if kind != RuntimeDeviceKind::Camera {
        return select_runtime_device(kind, devices).map(|device| vec![device]);
    }
    let mut admitted: Vec<RuntimeDevice> = Vec::new();
    for device in devices.iter().filter(|device| admits_device(kind, device)) {
        if !admitted
            .iter()
            .any(|known| known.instance_id.eq_ignore_ascii_case(&device.instance_id))
        {
            admitted.push(device.clone());
        }
    }
    let Some(first) = admitted.first() else {
        return Err(LctrlError::Unsupported {
            feature: format!(
                "{}.runtime_pnp (no uniquely identifiable {:?} device)",
                kind.feature(),
                kind.class()
            ),
        });
    };

    if admitted.len() > 1 {
        // Separate physical cameras would need separate consent; only devnodes
        // that share one parent are treated as the same camera.
        let parent = first.parent_instance_id.as_deref();
        let Some(parent) = parent else {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{} runtime selection found {} devnodes without a parent identity; refusing to treat them as one camera",
                    kind.feature(),
                    admitted.len()
                ),
            });
        };
        let split = admitted.iter().any(|device| {
            device
                .parent_instance_id
                .as_deref()
                .is_none_or(|other| !other.eq_ignore_ascii_case(parent))
        });
        if split {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{} runtime selection spans multiple physical cameras; refusing to mutate any of them",
                    kind.feature()
                ),
            });
        }
    }
    Ok(admitted)
}

/// Select only the touchpad's digitizer and mouse-input collections. Never
/// target the ACPI controller: disabling it removes the children needed to
/// discover the device again in a subsequent process.
pub fn select_touchpad_group(
    mouse_devices: &[RuntimeDevice],
    hid_devices: &[RuntimeDevice],
) -> Result<Vec<RuntimeDevice>> {
    let touchpad = select_runtime_device(RuntimeDeviceKind::Touchpad, hid_devices)?;
    let mut group = vec![touchpad];
    for device in mouse_devices {
        if device.class == RuntimeDeviceClass::Mouse
            && is_hid_collection(device)
            && has_hid_usage(device, "HID_DEVICE_UP:0001_U:0002")
            && device.parent_instance_id.as_deref().is_some_and(|parent| {
                group[0]
                    .parent_instance_id
                    .as_deref()
                    .is_some_and(|controller| parent.eq_ignore_ascii_case(controller))
            })
            && !group
                .iter()
                .any(|known| known.instance_id.eq_ignore_ascii_case(&device.instance_id))
        {
            if group.len() == MAX_TOUCHPAD_INPUT_LEAVES {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "touchpad input group exceeds the safe leaf limit".into(),
                });
            }
            group.push(device.clone());
        }
    }
    Ok(group)
}

fn admits_device(kind: RuntimeDeviceKind, device: &RuntimeDevice) -> bool {
    if device.class != kind.class()
        || device.instance_id.is_empty()
        || device.instance_id.contains('\0')
    {
        return false;
    }
    match kind {
        RuntimeDeviceKind::Camera => is_internal_camera(device),
        RuntimeDeviceKind::Microphone => is_internal_microphone(device),
        RuntimeDeviceKind::Touchpad => is_touchpad(device),
    }
}

fn is_internal_microphone(device: &RuntimeDevice) -> bool {
    let capture_endpoint = matches!(
        device.audio_form_factor,
        Some(AudioEndpointFormFactor::Microphone)
    ) || device.instance_id.contains("{0.0.1.");
    capture_endpoint
        && device
            .parent_instance_id
            .as_deref()
            .is_some_and(is_internal_audio_bus)
}

fn is_internal_audio_bus(parent: &str) -> bool {
    // Intel Smart Sound and classic HD Audio own the integrated capture array;
    // software endpoints live under ROOT\... devnodes.
    starts_with_ascii(parent, "INTELAUDIO\\") || starts_with_ascii(parent, "HDAUDIO\\")
}

fn is_internal_camera(device: &RuntimeDevice) -> bool {
    if has_magicbay_identity(device) || !names(device).any(has_camera_name) {
        return false;
    }
    names(device).any(has_internal_camera_name) || is_acpi_device(device)
}

fn has_magicbay_identity(device: &RuntimeDevice) -> bool {
    device
        .hardware_ids
        .iter()
        .chain(&device.compatible_ids)
        .chain(std::iter::once(&device.instance_id))
        .any(|id| contains_ascii(id, "VID_17EF&PID_7005"))
        || names(device).any(|name| contains_ascii(name, "magicbay"))
}

fn is_acpi_device(device: &RuntimeDevice) -> bool {
    starts_with_ascii(&device.instance_id, "ACPI\\")
        || device
            .hardware_ids
            .iter()
            .any(|id| starts_with_ascii(id, "ACPI\\"))
}

fn has_camera_name(name: &str) -> bool {
    contains_ascii(name, "camera") || contains_ascii(name, "webcam")
}

fn has_internal_camera_name(name: &str) -> bool {
    has_camera_name(name)
        && ["integrated", "internal", "built-in", "built in"]
            .iter()
            .any(|marker| contains_ascii(name, marker))
}

fn is_touchpad(device: &RuntimeDevice) -> bool {
    is_hid_collection(device)
        && has_hid_usage(device, "HID_DEVICE_UP:000D_U:0005")
        && device
            .parent_instance_id
            .as_deref()
            .is_some_and(|parent| starts_with_ascii(parent, "ACPI\\") && !parent.contains('\0'))
}

fn is_hid_collection(device: &RuntimeDevice) -> bool {
    starts_with_ascii(&device.instance_id, "HID\\") && !device.instance_id.contains('\0')
}

fn has_hid_usage(device: &RuntimeDevice, usage: &str) -> bool {
    device
        .hardware_ids
        .iter()
        .chain(&device.compatible_ids)
        .any(|id| id.eq_ignore_ascii_case(usage))
}

fn names(device: &RuntimeDevice) -> impl Iterator<Item = &str> {
    [
        device.friendly_name.as_deref(),
        device.description.as_deref(),
        device.bus_description.as_deref(),
    ]
    .into_iter()
    .flatten()
}

fn starts_with_ascii(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn contains_ascii(value: &str, needle: &str) -> bool {
    !needle.is_empty()
        && value
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

pub trait RuntimeDeviceApi: Send + Sync {
    fn devices(&self, class: RuntimeDeviceClass) -> Result<Vec<RuntimeDevice>>;
    fn status(&self, instance_id: &str) -> Result<PnpDeviceStatus>;
    fn set_state(&self, instance_id: &str, state: DeviceState) -> Result<()>;
}

/// Transactional runtime control over one conservatively selected PnP device.
#[derive(Debug)]
pub struct WindowsRuntimeDevices<A> {
    api: A,
}

impl<A> WindowsRuntimeDevices<A> {
    #[must_use]
    pub const fn new(api: A) -> Self {
        Self { api }
    }

    #[must_use]
    pub const fn api(&self) -> &A {
        &self.api
    }
}

impl<A> Default for WindowsRuntimeDevices<A>
where
    A: Default,
{
    fn default() -> Self {
        Self::new(A::default())
    }
}

impl<A> WindowsRuntimeDevices<A>
where
    A: RuntimeDeviceApi,
{
    pub fn selected_device(&self, kind: RuntimeDeviceKind) -> Result<RuntimeDevice> {
        select_runtime_device(kind, &self.api.devices(kind.class())?)
    }

    pub fn selected_devices(&self, kind: RuntimeDeviceKind) -> Result<Vec<RuntimeDevice>> {
        select_runtime_devices(kind, &self.api.devices(kind.class())?)
    }

    pub fn camera_group(&self) -> Result<Vec<RuntimeDevice>> {
        self.selected_devices(RuntimeDeviceKind::Camera)
    }

    /// Input leaves only; the controller, configuration and vendor-defined
    /// collections remain enabled and discoverable.
    pub fn touchpad_group(&self) -> Result<Vec<RuntimeDevice>> {
        let hid = self.api.devices(RuntimeDeviceClass::HidClass)?;
        let mouse = self.api.devices(RuntimeDeviceClass::Mouse)?;
        select_touchpad_group(&mouse, &hid)
    }

    /// The admitted touchpad input leaves with their independently read
    /// states. Unlike [`Self::state`] this does not collapse the group, so a
    /// split group — the state recovery exists for — is reported as observed
    /// instead of being refused.
    pub fn touchpad_leaves(&self) -> Result<(Vec<RuntimeDevice>, Vec<DeviceState>)> {
        let group = self.touchpad_group()?;
        if group.len() < 2 {
            return Err(LctrlError::ChannelUnavailable {
                channel: "touchpad recovery cannot verify a mouse input collection".into(),
            });
        }
        let states = self.group_states(RuntimeDeviceKind::Touchpad, &group)?;
        Ok((group, states))
    }

    /// One privacy state for the whole camera group. A group that is split
    /// between enabled and disabled devnodes has no single answer and refuses.
    pub fn camera_group_state(&self) -> Result<DeviceState> {
        let group = self.camera_group()?;
        self.group_state(RuntimeDeviceKind::Camera, &group)
    }

    /// The admitted camera devnodes with their independently read states.
    /// Unlike [`Self::camera_group_state`] this does not collapse the group, so
    /// a split group — and a devnode whose PnP status is neither started nor
    /// disabled — is attributed to the instance instead of failing the read.
    pub fn camera_devices(&self) -> Result<Vec<PrivacyDeviceState>> {
        let group = self.camera_group()?;
        let mut devices = Vec::with_capacity(group.len());
        for device in group {
            let mut node = PrivacyDeviceState {
                instance_id: device.instance_id.clone(),
                description: device
                    .friendly_name
                    .clone()
                    .or_else(|| device.description.clone()),
                state: None,
                error: None,
            };
            match self.api.status(&device.instance_id)?.state(RuntimeDeviceKind::Camera) {
                Ok(state) => node.state = Some(state),
                Err(error) => node.error = Some(error.to_string()),
            }
            devices.push(node);
        }
        Ok(devices)
    }

    fn group_state(&self, kind: RuntimeDeviceKind, group: &[RuntimeDevice]) -> Result<DeviceState> {
        let states = self.group_states(kind, group)?;
        aggregate_group_state(kind, &states)
    }

    fn group_states(
        &self,
        kind: RuntimeDeviceKind,
        group: &[RuntimeDevice],
    ) -> Result<Vec<DeviceState>> {
        let mut states = Vec::with_capacity(group.len());
        for device in group {
            states.push(self.selected_state(kind, device)?);
        }
        Ok(states)
    }

    pub fn set_camera_group(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let group = self.camera_group()?;
        self.set_group(RuntimeDeviceKind::Camera, group, state, apply)
    }

    fn set_group(
        &self,
        kind: RuntimeDeviceKind,
        group: Vec<RuntimeDevice>,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let states = self.group_states(kind, &group)?;
        let previous = aggregate_group_state(kind, &states)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, state));
        }
        if previous == state {
            return Ok(ChangeReport::committed(previous, state, state));
        }
        for (index, device) in group.iter().enumerate() {
            if let Err(error) = self.api.set_state(&device.instance_id, state) {
                // A failed call can have taken effect. Only a documented
                // pre-write refusal excludes the attempted node from recovery.
                let attempted = index + usize::from(!preserve_prewrite_error(&error));
                return Err(self.rollback_group(
                    kind,
                    &group[..attempted],
                    &states[..attempted],
                    error,
                ));
            }
        }
        match poll_readback(&state, READBACK_ATTEMPTS, READBACK_DELAY, || {
            self.group_state(kind, &group)
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, state, actual)),
            Err(error) => Err(self.rollback_group(kind, &group, &states, error)),
        }
    }

    fn rollback_group(
        &self,
        kind: RuntimeDeviceKind,
        group: &[RuntimeDevice],
        previous: &[DeviceState],
        error: LctrlError,
    ) -> LctrlError {
        let mut rollback_error = None;
        for (device, before) in group.iter().zip(previous).rev() {
            if let Err(failure) = self.api.set_state(&device.instance_id, *before) {
                rollback_error.get_or_insert(failure);
            }
        }
        for (device, before) in group.iter().zip(previous) {
            if let Err(failure) = poll_readback(before, READBACK_ATTEMPTS, READBACK_DELAY, || {
                self.selected_state(kind, device)
            }) {
                rollback_error.get_or_insert(failure);
            }
        }
        match rollback_error {
            None => error,
            Some(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "{} PnP mutation failed ({error}); rollback also failed ({rollback})",
                    kind.feature()
                ),
            },
        }
    }

    pub fn state(&self, kind: RuntimeDeviceKind) -> Result<DeviceState> {
        if kind == RuntimeDeviceKind::Touchpad {
            return self.group_state(kind, &self.touchpad_group()?);
        }
        let device = self.selected_device(kind)?;
        self.selected_state(kind, &device)
    }

    pub fn set(
        &self,
        kind: RuntimeDeviceKind,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        if kind == RuntimeDeviceKind::Touchpad {
            return self.set_group(kind, self.touchpad_group()?, state, apply);
        }
        let device = self.selected_device(kind)?;
        self.set_device(kind, device, state, apply)
    }

    /// Apply a state change to one already admitted devnode.
    fn set_device(
        &self,
        kind: RuntimeDeviceKind,
        device: RuntimeDevice,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let previous = self.selected_state(kind, &device)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, state));
        }

        if let Err(error) = self.api.set_state(&device.instance_id, state) {
            return if preserve_prewrite_error(&error) {
                Err(error)
            } else {
                Err(self.rollback(kind, &device, previous, error))
            };
        }

        // Every read is a fresh API call for the admitted instance ID. The
        // native backend locates the devnode again rather than trusting the
        // mutating call's success or a cached status value.
        match poll_readback(&state, READBACK_ATTEMPTS, READBACK_DELAY, || {
            self.selected_state(kind, &device)
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, state, actual)),
            Err(error) => Err(self.rollback(kind, &device, previous, error)),
        }
    }

    pub fn microphone_state(&self) -> Result<DeviceState> {
        self.state(RuntimeDeviceKind::Microphone)
    }

    pub fn set_microphone(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        self.set(RuntimeDeviceKind::Microphone, state, apply)
    }

    /// Restart one admitted input collection.
    ///
    /// The disable goes through the same runtime-only path as `touchpad off`
    /// (no persistent flag), so a failed cycle can never leave the pad
    /// disabled across a reboot. The restart is only counted when the disabled
    /// state was actually observed: a silently ignored disable would otherwise
    /// be reported as a completed restart.
    fn cycle_input_leaf(&self, index: usize, device: &RuntimeDevice, failures: &mut Vec<String>) {
        if let Err(error) = self
            .api
            .set_state(&device.instance_id, DeviceState::Disabled)
        {
            failures.push(format!("leaf {index} restart disable failed: {error}"));
            return;
        }
        if let Err(error) = self.poll_input_leaf_state(device, DeviceState::Disabled) {
            failures.push(format!("leaf {index} restart was not observed: {error}"));
        }
        if let Err(error) = self
            .api
            .set_state(&device.instance_id, DeviceState::Enabled)
        {
            failures.push(format!("leaf {index} restart enable failed: {error}"));
        }
    }

    /// Bounded state poll for a leaf that may be re-enumerating.
    ///
    /// Unlike [`poll_readback`], a read error is treated as "not observed yet"
    /// rather than as the result, because a devnode that is restarting
    /// legitimately stops answering queries for a moment.
    fn poll_input_leaf_state(
        &self,
        device: &RuntimeDevice,
        requested: DeviceState,
    ) -> Result<DeviceState> {
        let mut observed = String::from("no read completed");
        for attempt in 0..RECOVERY_READBACK_ATTEMPTS {
            if attempt > 0 {
                thread::sleep(READBACK_DELAY);
            }
            match self.selected_state(RuntimeDeviceKind::Touchpad, device) {
                Ok(state) if state == requested => return Ok(state),
                Ok(state) => observed = state.to_string(),
                Err(error) => observed = error.to_string(),
            }
        }
        Err(LctrlError::VerifyMismatch {
            requested: requested.to_string(),
            actual: observed,
        })
    }

    fn selected_state(
        &self,
        kind: RuntimeDeviceKind,
        device: &RuntimeDevice,
    ) -> Result<DeviceState> {
        self.api.status(&device.instance_id)?.state(kind)
    }

    fn rollback(
        &self,
        kind: RuntimeDeviceKind,
        device: &RuntimeDevice,
        previous: DeviceState,
        error: LctrlError,
    ) -> LctrlError {
        let rollback = self
            .api
            .set_state(&device.instance_id, previous)
            .and_then(|()| {
                poll_readback(&previous, READBACK_ATTEMPTS, READBACK_DELAY, || {
                    self.selected_state(kind, device)
                })
                .map(|_| ())
            });
        match rollback {
            Ok(()) => error,
            Err(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "{} PnP mutation failed ({error}); rollback also failed ({rollback})",
                    kind.feature()
                ),
            },
        }
    }
}

fn preserve_prewrite_error(error: &LctrlError) -> bool {
    matches!(
        error,
        LctrlError::PermissionDenied { .. }
            | LctrlError::Unsupported { .. }
            | LctrlError::InvalidArgument { .. }
    )
}

fn aggregate_group_state(kind: RuntimeDeviceKind, states: &[DeviceState]) -> Result<DeviceState> {
    let Some(first) = states.first() else {
        return Err(LctrlError::Unsupported {
            feature: format!("{}.runtime_pnp (no devnode in the group)", kind.feature()),
        });
    };
    if states.iter().all(|state| state == first) {
        return Ok(*first);
    }
    Err(LctrlError::ChannelUnavailable {
        channel: format!(
            "{} devnodes report split states {states:?}; refusing to report one state",
            kind.feature(),
        ),
    })
}

impl<A> PrivacyControl for WindowsRuntimeDevices<A>
where
    A: RuntimeDeviceApi,
{
    fn camera_state(&self) -> Result<DeviceState> {
        self.camera_group_state()
    }

    fn camera_devices(&self) -> Result<Vec<PrivacyDeviceState>> {
        WindowsRuntimeDevices::camera_devices(self)
    }

    /// The consent-store read is independent of the PnP camera group on
    /// purpose: it must answer while the devnodes are disabled, which is the
    /// state `privacy cam off --runtime` creates.
    fn camera_consent(&self) -> Result<lctrl_core::CameraConsentReport> {
        crate::camera_consent::read_camera_consent()
    }

    fn set_camera(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        self.set_camera_group(state, apply)
    }

    fn microphone_state(&self) -> Result<DeviceState> {
        WindowsRuntimeDevices::microphone_state(self)
    }

    fn set_microphone(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        WindowsRuntimeDevices::set_microphone(self, state, apply)
    }
}

impl<A> TouchpadControl for WindowsRuntimeDevices<A>
where
    A: RuntimeDeviceApi,
{
    fn touchpad_state(&self) -> Result<DeviceState> {
        self.state(RuntimeDeviceKind::Touchpad)
    }

    fn set_touchpad(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        self.set(RuntimeDeviceKind::Touchpad, state, apply)
    }

    fn recover_touchpad(&self, apply: ApplyMode) -> Result<lctrl_core::TouchpadRecoveryReport> {
        let (group, states) = self.touchpad_leaves()?;
        // A split group has a known repair: enable what an interrupted
        // transition left disabled. A group whose leaves are all enabled while
        // the pad does not serve input can only be repaired by restarting the
        // input collections themselves.
        let strategy = if states.iter().any(|state| *state == DeviceState::Disabled) {
            lctrl_core::TouchpadRecoveryStrategy::EnableDisabledLeaves
        } else {
            lctrl_core::TouchpadRecoveryStrategy::CycleInputLeaves
        };
        let mut leaves: Vec<_> = states
            .iter()
            .enumerate()
            .map(|(index, &previous)| lctrl_core::TouchpadRecoveryLeaf {
                role: if index == 0 { "digitizer" } else { "mouse" },
                previous,
                requested: DeviceState::Enabled,
                actual: None,
            })
            .collect();
        if apply == ApplyMode::DryRun {
            return Ok(lctrl_core::TouchpadRecoveryReport {
                mode: apply,
                strategy,
                leaves,
            });
        }
        let mut failures = Vec::new();
        for (index, (device, previous)) in group.iter().zip(&states).enumerate() {
            match strategy {
                lctrl_core::TouchpadRecoveryStrategy::EnableDisabledLeaves => {
                    if *previous == DeviceState::Disabled {
                        if let Err(error) = self
                            .api
                            .set_state(&device.instance_id, DeviceState::Enabled)
                        {
                            failures.push(format!("leaf {index} enable failed: {error}"));
                        }
                    }
                }
                lctrl_core::TouchpadRecoveryStrategy::CycleInputLeaves => {
                    self.cycle_input_leaf(index, device, &mut failures);
                }
            }
        }
        for (index, device) in group.iter().enumerate() {
            match self.poll_input_leaf_state(device, DeviceState::Enabled) {
                Ok(actual) => leaves[index].actual = Some(actual),
                Err(first) => {
                    // Recovery must never leave an input leaf disabled: retry
                    // the enable once and re-read before reporting the failure.
                    let retry = self
                        .api
                        .set_state(&device.instance_id, DeviceState::Enabled)
                        .and_then(|()| self.poll_input_leaf_state(device, DeviceState::Enabled));
                    match retry {
                        Ok(actual) => leaves[index].actual = Some(actual),
                        Err(second) => failures.push(format!(
                            "leaf {index} independent readback failed: {first}; enable retry failed: {second}"
                        )),
                    }
                }
            }
        }
        if !failures.is_empty() {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "touchpad recovery incomplete: {}; retry recovery after resolving errors",
                    failures.join("; ")
                ),
            });
        }
        Ok(lctrl_core::TouchpadRecoveryReport {
            mode: apply,
            strategy,
            leaves,
        })
    }

    fn touchpad_caps(&self) -> Result<lctrl_core::TouchpadCapabilities> {
        // The capability descriptor belongs to the admitted digitizer
        // collection; the input-leaf group a switch reads does not carry the
        // declared contacts, surface ranges or button type.
        let device = self.selected_device(RuntimeDeviceKind::Touchpad)?;
        crate::read_touchpad_capabilities(&device.instance_id)
    }
}

#[cfg(windows)]
mod native {
    use std::{mem, ptr};

    use lctrl_core::{DeviceState, LctrlError, Result};
    use windows_sys::{
        Win32::{
            Devices::{
                DeviceAndDriverInstallation::{
                    CM_DISABLE_UI_NOT_OK, CM_Disable_DevNode, CM_Enable_DevNode,
                    CM_Get_DevNode_Status, CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW,
                    CM_MapCrToWin32Err, CM_PROB_DISABLED, CR_ACCESS_DENIED,
                    CR_CALL_NOT_IMPLEMENTED, CR_DEVICE_NOT_THERE, CR_INVALID_API,
                    CR_INVALID_DEVNODE, CR_NEED_RESTART, CR_NO_CM_SERVICES, CR_NO_SUCH_DEVNODE,
                    CR_NOT_DISABLEABLE, CR_SUCCESS, DIGCF_PRESENT, DN_HAS_PROBLEM, DN_STARTED,
                    GUID_DEVCLASS_HIDCLASS, GUID_DEVCLASS_MOUSE, HDEVINFO, SP_DEVINFO_DATA,
                    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
                },
                Properties::{
                    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_CompatibleIds,
                    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName,
                    DEVPKEY_Device_HardwareIds, DEVPKEY_Device_Parent, DEVPROP_TYPE_STRING,
                    DEVPROP_TYPE_STRING_LIST, DEVPROP_TYPE_UINT32,
                },
            },
            Foundation::{
                DEVPROPKEY, ERROR_GEN_FAILURE, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS,
                ERROR_NOT_FOUND, GetLastError,
            },
            Media::Audio::{Microphone, PKEY_AudioEndpoint_FormFactor},
        },
        core::GUID,
    };

    use super::{
        AudioEndpointFormFactor, PnpDeviceProblem, PnpDeviceStatus, RuntimeDevice,
        RuntimeDeviceApi, RuntimeDeviceClass,
    };
    use crate::map_win_error;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeRuntimeDeviceApi;

    pub type NativeWindowsRuntimeDevices = super::WindowsRuntimeDevices<NativeRuntimeDeviceApi>;

    /// The modern Windows "Camera" setup class. `GUID_DEVCLASS_IMAGE` enumerates
    /// an empty set on the reference machine, while this class lists the UVC
    /// camera devnodes and excludes the firmware-update interface of the same
    /// USB composite device.
    const CAMERA_CLASS_GUID: GUID = GUID::from_u128(0xCA3E7AB9_B4C3_4AE6_8251_579EF933890F);

    /// The "AudioEndpoint" setup class reported by the audio endpoint devnodes.
    const AUDIO_ENDPOINT_CLASS_GUID: GUID = GUID::from_u128(0xC166523C_FE0C_4A94_A586_F1A80CFBBF3E);

    impl RuntimeDeviceApi for NativeRuntimeDeviceApi {
        fn devices(&self, class: RuntimeDeviceClass) -> Result<Vec<RuntimeDevice>> {
            let set = DeviceInfoSet::open(class)?;
            let mut devices = Vec::new();
            for index in 0.. {
                let mut info = SP_DEVINFO_DATA {
                    cbSize: u32::try_from(mem::size_of::<SP_DEVINFO_DATA>())
                        .expect("SP_DEVINFO_DATA size fits u32"),
                    ..Default::default()
                };
                let success = unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) };
                if success == 0 {
                    let error = unsafe { GetLastError() };
                    if error == ERROR_NO_MORE_ITEMS {
                        break;
                    }
                    return Err(map_win_error(error, "SetupDiEnumDeviceInfo"));
                }

                let mut device = RuntimeDevice::new(class, instance_id(set.0, &info)?);
                device.hardware_ids = string_list_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_HardwareIds,
                    "DEVPKEY_Device_HardwareIds",
                )?;
                device.compatible_ids = string_list_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_CompatibleIds,
                    "DEVPKEY_Device_CompatibleIds",
                )?;
                device.description = string_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_DeviceDesc,
                    "DEVPKEY_Device_DeviceDesc",
                )?;
                device.friendly_name = string_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_FriendlyName,
                    "DEVPKEY_Device_FriendlyName",
                )?;
                device.bus_description = string_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_BusReportedDeviceDesc,
                    "DEVPKEY_Device_BusReportedDeviceDesc",
                )?;
                device.parent_instance_id = string_property(
                    set.0,
                    &info,
                    &DEVPKEY_Device_Parent,
                    "DEVPKEY_Device_Parent",
                )?;
                if class == RuntimeDeviceClass::AudioEndpoint {
                    device.audio_form_factor = audio_form_factor(set.0, &info)?;
                }
                devices.push(device);
            }
            Ok(devices)
        }

        fn status(&self, instance_id: &str) -> Result<PnpDeviceStatus> {
            let devinst = locate_devnode(instance_id)?;
            let mut flags = 0u32;
            let mut problem = 0u32;
            let status = unsafe { CM_Get_DevNode_Status(&mut flags, &mut problem, devinst, 0) };
            require_configret(status, || format!("CM_Get_DevNode_Status({instance_id})"))?;
            Ok(PnpDeviceStatus {
                started: flags & DN_STARTED != 0,
                problem: if flags & DN_HAS_PROBLEM == 0 {
                    None
                } else if problem == CM_PROB_DISABLED {
                    Some(PnpDeviceProblem::Disabled)
                } else {
                    Some(PnpDeviceProblem::Other(problem))
                },
            })
        }

        fn set_state(&self, instance_id: &str, state: DeviceState) -> Result<()> {
            let devinst = locate_devnode(instance_id)?;
            let (status, operation) = match state {
                DeviceState::Disabled => (
                    // Runtime-only: deliberately do not set CM_DISABLE_PERSIST.
                    unsafe { CM_Disable_DevNode(devinst, CM_DISABLE_UI_NOT_OK) },
                    "CM_Disable_DevNode",
                ),
                DeviceState::Enabled => (
                    // CM_Enable_DevNode's flags are reserved and must be zero.
                    unsafe { CM_Enable_DevNode(devinst, 0) },
                    "CM_Enable_DevNode",
                ),
            };
            require_configret(status, || format!("{operation}({instance_id})"))
        }
    }

    struct DeviceInfoSet(HDEVINFO);

    impl DeviceInfoSet {
        fn open(class: RuntimeDeviceClass) -> Result<Self> {
            let guid = class_guid(class);
            let set =
                unsafe { SetupDiGetClassDevsW(&guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT) };
            if set == -1isize {
                return Err(map_win_error(
                    unsafe { GetLastError() },
                    "SetupDiGetClassDevsW runtime device class",
                ));
            }
            Ok(Self(set))
        }
    }

    impl Drop for DeviceInfoSet {
        fn drop(&mut self) {
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    fn class_guid(class: RuntimeDeviceClass) -> GUID {
        match class {
            RuntimeDeviceClass::Imaging => CAMERA_CLASS_GUID,
            RuntimeDeviceClass::AudioEndpoint => AUDIO_ENDPOINT_CLASS_GUID,
            RuntimeDeviceClass::Mouse => GUID_DEVCLASS_MOUSE,
            RuntimeDeviceClass::HidClass => GUID_DEVCLASS_HIDCLASS,
        }
    }

    fn locate_devnode(instance_id: &str) -> Result<u32> {
        if instance_id.is_empty() || instance_id.contains('\0') {
            return Err(LctrlError::InvalidArgument {
                detail: "a nonempty PnP instance ID without NUL is required".into(),
            });
        }
        let wide: Vec<u16> = instance_id.encode_utf16().chain(Some(0)).collect();
        let mut devinst = 0u32;
        let status =
            unsafe { CM_Locate_DevNodeW(&mut devinst, wide.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
        require_configret(status, || format!("CM_Locate_DevNodeW({instance_id})"))?;
        Ok(devinst)
    }

    fn require_configret(status: u32, context: impl FnOnce() -> String) -> Result<()> {
        if status == CR_SUCCESS {
            Ok(())
        } else {
            let context = context();
            Err(map_configret(status, &context))
        }
    }

    fn map_configret(status: u32, context: &str) -> LctrlError {
        match status {
            CR_ACCESS_DENIED => LctrlError::PermissionDenied {
                need: "administrator".into(),
            },
            CR_NOT_DISABLEABLE | CR_CALL_NOT_IMPLEMENTED | CR_INVALID_API => {
                LctrlError::Unsupported {
                    feature: context.into(),
                }
            }
            CR_NO_SUCH_DEVNODE | CR_INVALID_DEVNODE | CR_DEVICE_NOT_THERE | CR_NO_CM_SERVICES => {
                LctrlError::ChannelUnavailable {
                    channel: format!("{context}: CONFIGRET {status}"),
                }
            }
            CR_NEED_RESTART => LctrlError::FirmwareRejected {
                detail: format!("{context}: Windows requires a restart (CONFIGRET {status})"),
            },
            _ => {
                let gle = unsafe { CM_MapCrToWin32Err(status, ERROR_GEN_FAILURE) };
                match map_win_error(gle, context) {
                    LctrlError::Io(_) => LctrlError::Io(std::io::Error::other(format!(
                        "{context}: CONFIGRET {status}, Win32 error {gle}"
                    ))),
                    other => other,
                }
            }
        }
    }

    fn instance_id(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Result<String> {
        let mut buffer = [0u16; 1024];
        let mut required = 0u32;
        let success = unsafe {
            SetupDiGetDeviceInstanceIdW(
                set,
                info,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).expect("buffer length fits u32"),
                &mut required,
            )
        };
        if success == 0 {
            return Err(map_win_error(
                unsafe { GetLastError() },
                "SetupDiGetDeviceInstanceIdW",
            ));
        }
        let length = usize::try_from(required)
            .ok()
            .and_then(|value| value.checked_sub(1))
            .filter(|value| *value <= buffer.len())
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("SetupDi device instance id length {required} is invalid"),
            })?;
        String::from_utf16(&buffer[..length]).map_err(|error| {
            LctrlError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })
    }

    fn string_property(
        set: HDEVINFO,
        info: &SP_DEVINFO_DATA,
        key: &DEVPROPKEY,
        context: &str,
    ) -> Result<Option<String>> {
        let Some(buffer) = utf16_property(set, info, key, DEVPROP_TYPE_STRING, context)? else {
            return Ok(None);
        };
        let length = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        if length == 0 {
            return Ok(None);
        }
        String::from_utf16(&buffer[..length])
            .map(Some)
            .map_err(|error| LctrlError::ChannelUnavailable {
                channel: format!("{context} returned invalid UTF-16: {error}"),
            })
    }

    fn string_list_property(
        set: HDEVINFO,
        info: &SP_DEVINFO_DATA,
        key: &DEVPROPKEY,
        context: &str,
    ) -> Result<Vec<String>> {
        let Some(buffer) = utf16_property(set, info, key, DEVPROP_TYPE_STRING_LIST, context)?
        else {
            return Ok(Vec::new());
        };
        buffer
            .split(|value| *value == 0)
            .filter(|value| !value.is_empty())
            .map(|value| {
                String::from_utf16(value).map_err(|error| LctrlError::ChannelUnavailable {
                    channel: format!("{context} returned invalid UTF-16: {error}"),
                })
            })
            .collect()
    }

    fn utf16_property(
        set: HDEVINFO,
        info: &SP_DEVINFO_DATA,
        key: &DEVPROPKEY,
        expected_type: u32,
        context: &str,
    ) -> Result<Option<Vec<u16>>> {
        let mut property_type = 0u32;
        let mut required = 0u32;
        let success = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                info,
                key,
                &mut property_type,
                ptr::null_mut(),
                0,
                &mut required,
                0,
            )
        };
        if success == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(None);
            }
            if error != ERROR_INSUFFICIENT_BUFFER {
                return Err(map_win_error(error, context));
            }
        }
        if required == 0 {
            return Ok(Some(Vec::new()));
        }
        if required % 2 != 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{context} returned odd UTF-16 byte length {required}"),
            });
        }
        let mut buffer =
            vec![
                0u16;
                usize::try_from(required / 2).map_err(|_| LctrlError::ChannelUnavailable {
                    channel: format!("{context} length exceeds usize"),
                })?
            ];
        let mut actual_type = 0u32;
        let mut actual_size = 0u32;
        let success = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                info,
                key,
                &mut actual_type,
                buffer.as_mut_ptr().cast::<u8>(),
                required,
                &mut actual_size,
                0,
            )
        };
        if success == 0 {
            return Err(map_win_error(unsafe { GetLastError() }, context));
        }
        if actual_type != expected_type || actual_size > required || actual_size % 2 != 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{context} returned type {actual_type} and {actual_size} bytes; expected type {expected_type} within {required} bytes"
                ),
            });
        }
        Ok(Some(buffer))
    }

    fn audio_form_factor(
        set: HDEVINFO,
        info: &SP_DEVINFO_DATA,
    ) -> Result<Option<AudioEndpointFormFactor>> {
        // PROPERTYKEY and DEVPROPKEY have the same public fmtid/pid layout.
        // SetupAPI reports this Core Audio property as VT_UI4/DEVPROP_TYPE_UINT32.
        let key = DEVPROPKEY {
            fmtid: PKEY_AudioEndpoint_FormFactor.fmtid,
            pid: PKEY_AudioEndpoint_FormFactor.pid,
        };
        let mut property_type = 0u32;
        let mut value = 0u32;
        let mut required = 0u32;
        let success = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                info,
                &key,
                &mut property_type,
                (&mut value as *mut u32).cast::<u8>(),
                u32::try_from(mem::size_of::<u32>()).expect("u32 size fits u32"),
                &mut required,
                0,
            )
        };
        if success == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(map_win_error(error, "PKEY_AudioEndpoint_FormFactor"));
        }
        if property_type != DEVPROP_TYPE_UINT32
            || required != u32::try_from(mem::size_of::<u32>()).expect("u32 size fits u32")
        {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "PKEY_AudioEndpoint_FormFactor returned type {property_type} and {required} bytes; expected UINT32"
                ),
            });
        }
        Ok(Some(if value == Microphone as u32 {
            AudioEndpointFormFactor::Microphone
        } else {
            AudioEndpointFormFactor::Other(value)
        }))
    }
}

#[cfg(windows)]
pub use native::{NativeRuntimeDeviceApi, NativeWindowsRuntimeDevices};
