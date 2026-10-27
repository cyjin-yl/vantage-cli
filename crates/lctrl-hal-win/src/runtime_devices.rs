//! Conservative Windows PnP runtime control for privacy devices and touchpads.
//!
//! `DeviceState::Disabled` in this module means that the selected device node
//! has `CM_PROB_DISABLED`. It does not mean that an EC privacy shutter, a
//! hardware microphone mute circuit, or a persistent BIOS switch is engaged.
//! Disabling a device node removes its runtime PnP function from the bus; the
//! selected instance ID is retained so the same device can be enabled again.

use std::time::Duration;

use lctrl_core::{ApplyMode, ChangeReport, DeviceState, LctrlError, Result};
use lctrl_hal::{PrivacyControl, TouchpadControl, poll_readback};

const READBACK_ATTEMPTS: usize = 10;
const READBACK_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeDeviceClass {
    Imaging,
    AudioEndpoint,
    Mouse,
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
            Self::Touchpad => RuntimeDeviceClass::Mouse,
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
    for device in devices
        .iter()
        .filter(|device| admits_device(kind, device))
    {
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

fn admits_device(kind: RuntimeDeviceKind, device: &RuntimeDevice) -> bool {
    if device.class != kind.class()
        || device.instance_id.is_empty()
        || device.instance_id.contains('\0')
    {
        return false;
    }
    match kind {
        RuntimeDeviceKind::Camera => is_internal_camera(device),
        RuntimeDeviceKind::Microphone => {
            matches!(
                device.audio_form_factor,
                Some(AudioEndpointFormFactor::Microphone)
            )
        }
        RuntimeDeviceKind::Touchpad => is_touchpad(device),
    }
}

fn is_internal_camera(device: &RuntimeDevice) -> bool {
    if has_magicbay_identity(device) || !names(device).any(|name| has_camera_name(name)) {
        return false;
    }
    names(device).any(|name| has_internal_camera_name(name)) || is_acpi_device(device)
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
    let has_hardware_id = device
        .hardware_ids
        .iter()
        .chain(&device.compatible_ids)
        .any(|id| {
            id.split(['\\', '&']).any(|component| {
                component
                    .trim_start_matches('*')
                    .eq_ignore_ascii_case("PNP0C50")
            }) || contains_ascii(id, "HID_DEVICE_UP:000D_U:0005")
        });
    has_hardware_id || names(device).any(|name| has_touchpad_name(name))
}

fn has_touchpad_name(name: &str) -> bool {
    ["touchpad", "touch pad", "trackpad", "track pad"]
        .iter()
        .any(|marker| contains_ascii(name, marker))
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

    pub fn state(&self, kind: RuntimeDeviceKind) -> Result<DeviceState> {
        let device = self.selected_device(kind)?;
        self.selected_state(kind, &device)
    }

    pub fn set(
        &self,
        kind: RuntimeDeviceKind,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        let device = self.selected_device(kind)?;
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

    fn selected_state(&self, kind: RuntimeDeviceKind, device: &RuntimeDevice) -> Result<DeviceState> {
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
            | LctrlError::ChannelUnavailable { .. }
            | LctrlError::Unsupported { .. }
            | LctrlError::InvalidArgument { .. }
    )
}

impl<A> PrivacyControl for WindowsRuntimeDevices<A>
where
    A: RuntimeDeviceApi,
{
    fn camera_state(&self) -> Result<DeviceState> {
        self.state(RuntimeDeviceKind::Camera)
    }

    fn set_camera(
        &self,
        state: DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DeviceState>> {
        self.set(RuntimeDeviceKind::Camera, state, apply)
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
}

#[cfg(windows)]
mod native {
    use std::{mem, ptr};

    use lctrl_core::{DeviceState, LctrlError, Result};
    use windows_sys::{
        Win32::{
            Devices::{
                DeviceAndDriverInstallation::{
                    CM_Disable_DevNode, CM_Enable_DevNode, CM_Get_DevNode_Status,
                    CM_LOCATE_DEVNODE_NORMAL, CM_Locate_DevNodeW, CM_MapCrToWin32Err,
                    CM_PROB_DISABLED, CM_DISABLE_UI_NOT_OK, CR_ACCESS_DENIED,
                    CR_CALL_NOT_IMPLEMENTED, CR_DEVICE_NOT_THERE, CR_INVALID_API,
                    CR_INVALID_DEVNODE, CR_NEED_RESTART, CR_NOT_DISABLEABLE, CR_NO_CM_SERVICES,
                    CR_NO_SUCH_DEVNODE, CR_SUCCESS, DIGCF_PRESENT, DN_HAS_PROBLEM, DN_STARTED,
                    GUID_DEVCLASS_IMAGE, GUID_DEVCLASS_MOUSE, HDEVINFO, SP_DEVINFO_DATA,
                    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
                },
                Properties::{
                    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_CompatibleIds,
                    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName,
                    DEVPKEY_Device_HardwareIds, DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_LIST,
                    DEVPROP_TYPE_UINT32,
                },
            },
            Foundation::{
                DEVPROPKEY, ERROR_GEN_FAILURE, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS,
                ERROR_NOT_FOUND, GetLastError,
            },
            Media::{
                Audio::{Microphone, PKEY_AudioEndpoint_FormFactor},
                KernelStreaming::AUDIOENDPOINT_CLASS_UUID,
            },
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
            require_configret(status, || {
                format!("CM_Get_DevNode_Status({instance_id})")
            })?;
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
            let set = unsafe {
                SetupDiGetClassDevsW(&guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT)
            };
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
            RuntimeDeviceClass::Imaging => GUID_DEVCLASS_IMAGE,
            RuntimeDeviceClass::AudioEndpoint => AUDIOENDPOINT_CLASS_UUID,
            RuntimeDeviceClass::Mouse => GUID_DEVCLASS_MOUSE,
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
        let status = unsafe {
            CM_Locate_DevNodeW(&mut devinst, wide.as_ptr(), CM_LOCATE_DEVNODE_NORMAL)
        };
        require_configret(status, || {
            format!("CM_Locate_DevNodeW({instance_id})")
        })?;
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
        let mut buffer = vec![
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

    fn audio_form_factor(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Result<Option<AudioEndpointFormFactor>> {
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
            return Err(map_win_error(
                error,
                "PKEY_AudioEndpoint_FormFactor",
            ));
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
