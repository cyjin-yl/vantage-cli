use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, PanelMode, PanelRefreshCapability, PanelRefreshRequest,
    PanelTarget, RefreshMode, Result,
};
use lctrl_hal::{PanelControl, poll_readback};

use crate::wmi_contract::ROOT_WMI;
use crate::{WmiObject, WmiTransport, WmiValue};

const PANEL_REFRESH_DATA: &str = "LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA";

/// Public-display-API operations needed by the panel controller. Implementations
/// must return the active (not registry-stored) current mode and modes compatible
/// with the currently connected monitor, without raw/rotated-mode expansion.
pub trait DisplayApi: Send + Sync {
    fn connected_targets(&self) -> Result<Vec<PanelTarget>>;
    fn current_mode(&self, target: &PanelTarget) -> Result<PanelMode>;
    fn compatible_modes(&self, target: &PanelTarget) -> Result<Vec<PanelMode>>;
    fn test_mode(&self, mode: &PanelMode) -> Result<()>;
    fn apply_mode(&self, mode: &PanelMode) -> Result<()>;
}

#[derive(Debug)]
pub struct WindowsPanelController<D, W> {
    display: D,
    wmi: W,
}

impl<D, W> WindowsPanelController<D, W> {
    #[must_use]
    pub const fn new(display: D, wmi: W) -> Self {
        Self { display, wmi }
    }

    #[must_use]
    pub const fn display(&self) -> &D {
        &self.display
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

impl<D, W> WindowsPanelController<D, W>
where
    D: DisplayApi,
    W: WmiTransport,
{
    fn selected_target(&self) -> Result<PanelTarget> {
        let mut internal = self
            .display
            .connected_targets()?
            .into_iter()
            .filter(|target| target.internal);
        let target = internal.next().ok_or_else(|| LctrlError::Unsupported {
            feature: "panel.internal.connected".into(),
        })?;
        if internal.next().is_some() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "native panel selection is ambiguous: multiple active internal panels"
                    .into(),
            });
        }
        if target.device_name.is_empty() || target.device_name.contains('\0') {
            return Err(LctrlError::ChannelUnavailable {
                channel: "native panel returned an invalid GDI device name".into(),
            });
        }
        Ok(target)
    }

    fn current_on(&self, target: &PanelTarget) -> Result<PanelMode> {
        let current = self.display.current_mode(target)?;
        if current.target != *target {
            return Err(LctrlError::ChannelUnavailable {
                channel: "native current mode belongs to a different display path".into(),
            });
        }
        require_valid_mode(&current)?;
        Ok(current)
    }

    fn panel_refresh_object(&self) -> Result<WmiObject> {
        let query = format!(
            "SELECT MinimumRefreshRate, MaximumRefreshRate, DefaultRefreshRate, Mode FROM {PANEL_REFRESH_DATA} WHERE Active = TRUE"
        );
        let objects = self.wmi.query(ROOT_WMI, &query)?;
        // The ACPI provider exposes one active record per firmware WMI block: the
        // reference ThinkBook reports the single internal panel 22 times. Records
        // that agree on every field this controller reads describe the same panel,
        // so they collapse to one; records that disagree still fail closed rather
        // than selecting one arbitrarily.
        let mut distinct = Vec::new();
        for object in &objects {
            let identity = (
                required_u16(object, "MinimumRefreshRate")?,
                required_u16(object, "MaximumRefreshRate")?,
                required_u16(object, "DefaultRefreshRate")?,
                required_u16(object, "Mode")?,
            );
            if !distinct.contains(&identity) {
                distinct.push(identity);
            }
        }
        match distinct.len() {
            0 => Err(LctrlError::Unsupported {
                feature: "panel.refresh".into(),
            }),
            1 => Ok(objects.into_iter().next().expect("length checked")),
            count => Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "WMI panel refresh selection is ambiguous: {count} disagreeing active records"
                ),
            }),
        }
    }

    fn requested_mode(
        &self,
        target: &PanelTarget,
        previous: &PanelMode,
        request: PanelRefreshRequest,
    ) -> Result<PanelMode> {
        let capability = self.refresh_capability()?;
        validate_capability(&capability)?;
        let hz = match request {
            PanelRefreshRequest::Fixed(hz) => {
                if hz <= 1 {
                    return Err(LctrlError::InvalidArgument {
                        detail: "panel refresh rate must be a fixed value greater than 1 Hz".into(),
                    });
                }
                if !capability.supports_hz(hz) {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!(
                            "panel refresh rate {hz} Hz is outside firmware range {}..={} Hz",
                            capability.min_hz, capability.max_hz
                        ),
                    });
                }
                hz
            }
            PanelRefreshRequest::Auto => capability.default_hz,
        };

        let modes = self.display.compatible_modes(target)?;
        modes
            .into_iter()
            .filter(|mode| {
                mode.target == *target
                    && previous.same_mode_except_refresh(mode)
                    && valid_mode(mode)
            })
            .find(|mode| mode.refresh_hz == u32::from(hz))
            .ok_or_else(|| LctrlError::Unsupported {
                feature: format!("panel.refresh.{hz}Hz.exact_native_mode"),
            })
    }

    fn rollback_mode(&self, previous: &PanelMode, error: LctrlError) -> LctrlError {
        let target = &previous.target;
        let rollback = self.display.apply_mode(previous).and_then(|()| {
            poll_readback(previous, 1, Duration::ZERO, || self.current_on(target)).map(|_| ())
        });
        match rollback {
            Ok(()) => error,
            Err(rollback) => LctrlError::FirmwareRejected {
                detail: format!(
                    "panel refresh transition failed ({error}); rollback also failed ({rollback})"
                ),
            },
        }
    }
}

impl<D, W> PanelControl for WindowsPanelController<D, W>
where
    D: DisplayApi,
    W: WmiTransport,
{
    fn refresh_capability(&self) -> Result<PanelRefreshCapability> {
        let object = self.panel_refresh_object()?;
        let capability = PanelRefreshCapability::new(
            required_u16(&object, "MinimumRefreshRate")?,
            required_u16(&object, "MaximumRefreshRate")?,
            required_u16(&object, "DefaultRefreshRate")?,
        );
        validate_capability(&capability)?;
        Ok(capability)
    }

    fn refresh_mode(&self) -> Result<RefreshMode> {
        let object = self.panel_refresh_object()?;
        Ok(RefreshMode::from_raw(required_u16(&object, "Mode")?))
    }

    fn panel_mode(&self) -> Result<PanelMode> {
        let target = self.selected_target()?;
        self.current_on(&target)
    }

    fn set_refresh_rate(
        &self,
        request: PanelRefreshRequest,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PanelMode>> {
        let target = self.selected_target()?;
        let previous = self.current_on(&target)?;
        let requested = self.requested_mode(&target, &previous, request)?;

        // CDS_TEST is meaningful in a dry run: it asks the display driver to
        // validate this exact enumerated mode without changing the desktop.
        self.display.test_mode(&requested)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        if previous == requested {
            let actual = previous.clone();
            return Ok(ChangeReport::committed(previous, requested, actual));
        }

        let result = self.display.apply_mode(&requested).and_then(|()| {
            poll_readback(&requested, 1, Duration::ZERO, || self.current_on(&target))
        });
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => Err(self.rollback_mode(&previous, error)),
        }
    }
}

fn validate_capability(capability: &PanelRefreshCapability) -> Result<()> {
    if capability.min_hz <= 1
        || capability.max_hz < capability.min_hz
        || capability.default_hz < capability.min_hz
        || capability.default_hz > capability.max_hz
    {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "invalid firmware panel refresh range {}..={} Hz with default {} Hz",
                capability.min_hz, capability.max_hz, capability.default_hz
            ),
        });
    }
    Ok(())
}

fn valid_mode(mode: &PanelMode) -> bool {
    mode.target.internal
        && !mode.target.device_name.is_empty()
        && !mode.target.device_name.contains('\0')
        && mode.width != 0
        && mode.height != 0
        && mode.bits_per_pixel != 0
        && mode.refresh_hz > 1
}

fn require_valid_mode(mode: &PanelMode) -> Result<()> {
    if valid_mode(mode) {
        Ok(())
    } else {
        Err(LctrlError::ChannelUnavailable {
            channel: format!("native panel returned an invalid current mode: {mode}"),
        })
    }
}

fn required_u16(object: &WmiObject, name: &str) -> Result<u16> {
    match object.get(name) {
        Some(WmiValue::U16(value)) => Ok(*value),
        Some(other) => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI {name} is not u16: {other:?}"),
        }),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI omitted {name}"),
        }),
    }
}

#[cfg(windows)]
mod native {
    use std::{io, ptr};

    use lctrl_core::{LctrlError, PanelAdapterId, PanelMode, PanelRotation, PanelTarget, Result};
    use windows_sys::Win32::{
        Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
            QueryDisplayConfig,
        },
        Foundation::{ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, LUID},
        Graphics::Gdi::{
            CDS_TEST, CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW,
            DISP_CHANGE_BADDUALVIEW, DISP_CHANGE_BADFLAGS, DISP_CHANGE_BADMODE,
            DISP_CHANGE_BADPARAM, DISP_CHANGE_FAILED, DISP_CHANGE_NOTUPDATED, DISP_CHANGE_RESTART,
            DISP_CHANGE_SUCCESSFUL, DISPLAYCONFIG_PATH_ACTIVE, DM_BITSPERPEL, DM_DISPLAYFREQUENCY,
            DM_DISPLAYORIENTATION, DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION, EDS_ROTATEDMODE,
            ENUM_CURRENT_SETTINGS, EnumDisplaySettingsExW,
        },
    };

    use super::DisplayApi;
    use crate::map_win_error;

    const REQUIRED_MODE_FIELDS: u32 = DM_BITSPERPEL
        | DM_PELSWIDTH
        | DM_PELSHEIGHT
        | DM_DISPLAYFREQUENCY
        | DM_POSITION
        | DM_DISPLAYORIENTATION;
    const HOTPLUG_RETRIES: usize = 4;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeDisplayApi;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ModeFields {
        width: u32,
        height: u32,
        bits_per_pixel: u32,
        refresh_hz: u32,
        position_x: i32,
        position_y: i32,
        rotation: PanelRotation,
    }

    impl ModeFields {
        fn of(mode: &PanelMode) -> Self {
            Self {
                width: mode.width,
                height: mode.height,
                bits_per_pixel: mode.bits_per_pixel,
                refresh_hz: mode.refresh_hz,
                position_x: mode.position_x,
                position_y: mode.position_y,
                rotation: mode.rotation,
            }
        }

        fn into_mode(self, target: &PanelTarget) -> PanelMode {
            PanelMode {
                target: target.clone(),
                width: self.width,
                height: self.height,
                bits_per_pixel: self.bits_per_pixel,
                refresh_hz: self.refresh_hz,
                position_x: self.position_x,
                position_y: self.position_y,
                rotation: self.rotation,
            }
        }

        const fn shape(self) -> (u32, u32, u32, u32) {
            (
                self.width,
                self.height,
                self.bits_per_pixel,
                self.refresh_hz,
            )
        }
    }

    impl DisplayApi for NativeDisplayApi {
        fn connected_targets(&self) -> Result<Vec<PanelTarget>> {
            let paths = active_paths()?;
            let mut targets = Vec::with_capacity(paths.len());
            for path in paths {
                if path.flags & DISPLAYCONFIG_PATH_ACTIVE == 0
                    || path.targetInfo.targetAvailable == 0
                {
                    continue;
                }
                let adapter = adapter_id(path.sourceInfo.adapterId);
                if adapter != adapter_id(path.targetInfo.adapterId) {
                    return Err(LctrlError::ChannelUnavailable {
                        channel: "display path source and target adapters differ".into(),
                    });
                }

                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
                target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
                target.header.size = struct_size::<DISPLAYCONFIG_TARGET_DEVICE_NAME>();
                target.header.adapterId = path.targetInfo.adapterId;
                target.header.id = path.targetInfo.id;
                let status = unsafe { DisplayConfigGetDeviceInfo(&mut target.header) };
                if status != 0 {
                    return Err(display_config_error(
                        status as u32,
                        "DisplayConfigGetDeviceInfo(GET_TARGET_NAME)",
                    ));
                }

                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                source.header.size = struct_size::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>();
                source.header.adapterId = path.sourceInfo.adapterId;
                source.header.id = path.sourceInfo.id;
                let status = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) };
                if status != 0 {
                    return Err(display_config_error(
                        status as u32,
                        "DisplayConfigGetDeviceInfo(GET_SOURCE_NAME)",
                    ));
                }
                let device_name = utf16_string(&source.viewGdiDeviceName)?;
                if device_name.is_empty() {
                    return Err(LctrlError::ChannelUnavailable {
                        channel: "DisplayConfig returned an empty GDI device name".into(),
                    });
                }
                targets.push(PanelTarget::new(
                    device_name,
                    adapter,
                    path.sourceInfo.id,
                    path.targetInfo.id,
                    is_internal_output(target.outputTechnology),
                ));
            }
            Ok(targets)
        }

        fn current_mode(&self, target: &PanelTarget) -> Result<PanelMode> {
            let device = wide_device_name(&target.device_name)?;
            let raw = enum_raw_mode(&device, ENUM_CURRENT_SETTINGS, 0).ok_or_else(|| {
                LctrlError::ChannelUnavailable {
                    channel: format!(
                        "EnumDisplaySettingsExW(ENUM_CURRENT_SETTINGS) failed for {}",
                        target.device_name
                    ),
                }
            })?;
            let fields = decode_fields(&raw).ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!(
                    "current display mode for {} omits required fields or uses a non-fixed rate",
                    target.device_name
                ),
            })?;
            Ok(fields.into_mode(target))
        }

        fn compatible_modes(&self, target: &PanelTarget) -> Result<Vec<PanelMode>> {
            // Enumerated driver entries are only guaranteed to carry the pixel
            // geometry and refresh rate; desktop position and orientation belong
            // to the active source layout. Refresh-only admission therefore keeps
            // the current layout and varies only the four enumerated fields.
            let current = self.current_mode(target)?;
            let device = wide_device_name(&target.device_name)?;
            let mut modes = Vec::new();
            let mut index = 0u32;
            while let Some(raw) = enum_raw_mode(&device, index, 0) {
                if let Some((width, height, bits_per_pixel, refresh_hz)) = decode_shape(&raw) {
                    let mut mode = current.clone();
                    mode.width = width;
                    mode.height = height;
                    mode.bits_per_pixel = bits_per_pixel;
                    mode.refresh_hz = refresh_hz;
                    if !modes.contains(&mode) {
                        modes.push(mode);
                    }
                }
                index = index
                    .checked_add(1)
                    .ok_or_else(|| LctrlError::ChannelUnavailable {
                        channel: "display mode index overflow".into(),
                    })?;
            }
            if modes.is_empty() {
                return Err(LctrlError::Unsupported {
                    feature: "panel.refresh.native_modes".into(),
                });
            }
            Ok(modes)
        }

        fn test_mode(&self, mode: &PanelMode) -> Result<()> {
            self.change_mode(mode, CDS_TEST)
        }

        fn apply_mode(&self, mode: &PanelMode) -> Result<()> {
            self.change_mode(mode, CDS_UPDATEREGISTRY)
        }
    }

    impl NativeDisplayApi {
        fn change_mode(&self, mode: &PanelMode, flags: u32) -> Result<()> {
            if !mode.target.internal {
                return Err(LctrlError::Unsupported {
                    feature: "panel.external.write".into(),
                });
            }
            let device = wide_device_name(&mode.target.device_name)?;
            let mut raw = find_raw_mode(&device, mode)?;
            raw.dmSize = struct_size16::<DEVMODEW>();
            raw.dmDriverExtra = 0;
            // Admit exactly the documented display fields needed for a
            // refresh-only transition. Private/printer fields are never set.
            raw.dmFields = REQUIRED_MODE_FIELDS;
            let status = unsafe {
                ChangeDisplaySettingsExW(device.as_ptr(), &raw, ptr::null_mut(), flags, ptr::null())
            };
            display_change_result(status)
        }
    }

    fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
        for _ in 0..HOTPLUG_RETRIES {
            let mut path_count = 0u32;
            let mut mode_count = 0u32;
            let status = unsafe {
                GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            };
            if status != 0 {
                return Err(display_config_error(status, "GetDisplayConfigBufferSizes"));
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
            let status = unsafe {
                QueryDisplayConfig(
                    QDC_ONLY_ACTIVE_PATHS,
                    &mut path_count,
                    paths.as_mut_ptr(),
                    &mut mode_count,
                    modes.as_mut_ptr(),
                    ptr::null_mut(),
                )
            };
            if status == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if status != 0 {
                return Err(display_config_error(status, "QueryDisplayConfig"));
            }
            paths.truncate(path_count as usize);
            return Ok(paths);
        }
        Err(LctrlError::ChannelUnavailable {
            channel: "display topology changed repeatedly during native enumeration".into(),
        })
    }

    fn find_raw_mode(device: &[u16], mode: &PanelMode) -> Result<DEVMODEW> {
        let expected = ModeFields::of(mode);
        let shape = expected.shape();
        let mut index = 0u32;
        // Admission uses current-orientation modes only. Looking up an already
        // admitted/previous mode also accepts rotated modes so rollback can
        // restore rotation after an unexpected driver transition.
        while let Some(mut raw) = enum_raw_mode(device, index, EDS_ROTATEDMODE) {
            if decode_shape(&raw) == Some(shape) {
                // The driver entry may omit desktop position/orientation, so the
                // admitted layout is stamped onto it instead of trusting defaults.
                let mut display = unsafe { raw.Anonymous1.Anonymous2 };
                display.dmPosition.x = expected.position_x;
                display.dmPosition.y = expected.position_y;
                display.dmDisplayOrientation = expected.rotation.raw();
                raw.Anonymous1.Anonymous2 = display;
                return Ok(raw);
            }
            index = index
                .checked_add(1)
                .ok_or_else(|| LctrlError::ChannelUnavailable {
                    channel: "display mode index overflow".into(),
                })?;
        }
        Err(LctrlError::Unsupported {
            feature: format!("panel.refresh.{}Hz.exact_native_mode", mode.refresh_hz),
        })
    }

    fn enum_raw_mode(device: &[u16], index: u32, flags: u32) -> Option<DEVMODEW> {
        let mut raw = DEVMODEW::default();
        raw.dmSize = struct_size16::<DEVMODEW>();
        raw.dmDriverExtra = 0;
        let ok = unsafe { EnumDisplaySettingsExW(device.as_ptr(), index, &mut raw, flags) };
        (ok != 0).then_some(raw)
    }

    fn decode_fields(raw: &DEVMODEW) -> Option<ModeFields> {
        let (width, height, bits_per_pixel, refresh_hz) = decode_shape(raw)?;
        let layout_fields = DM_POSITION | DM_DISPLAYORIENTATION;
        if raw.dmFields & layout_fields != layout_fields {
            return None;
        }
        let display = unsafe { raw.Anonymous1.Anonymous2 };
        let rotation = PanelRotation::from_raw(display.dmDisplayOrientation)?;
        Some(ModeFields {
            width,
            height,
            bits_per_pixel,
            refresh_hz,
            position_x: display.dmPosition.x,
            position_y: display.dmPosition.y,
            rotation,
        })
    }

    /// Pixel geometry and refresh rate of one driver mode entry. These are the
    /// only fields every enumerated entry is required to carry.
    fn decode_shape(raw: &DEVMODEW) -> Option<(u32, u32, u32, u32)> {
        let shape_fields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;
        if raw.dmFields & shape_fields != shape_fields
            || raw.dmPelsWidth == 0
            || raw.dmPelsHeight == 0
            || raw.dmBitsPerPel == 0
            || raw.dmDisplayFrequency <= 1
        {
            return None;
        }
        Some((
            raw.dmPelsWidth,
            raw.dmPelsHeight,
            raw.dmBitsPerPel,
            raw.dmDisplayFrequency,
        ))
    }

    fn adapter_id(raw: LUID) -> PanelAdapterId {
        PanelAdapterId::new(raw.HighPart, raw.LowPart)
    }

    fn is_internal_output(raw: i32) -> bool {
        matches!(
            raw,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS
                | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                | DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
        )
    }

    fn wide_device_name(name: &str) -> Result<Vec<u16>> {
        if name.is_empty() || name.contains('\0') {
            return Err(LctrlError::InvalidArgument {
                detail: "display device name must be nonempty and contain no NUL".into(),
            });
        }
        Ok(name.encode_utf16().chain(std::iter::once(0)).collect())
    }

    fn utf16_string(raw: &[u16]) -> Result<String> {
        let length = raw
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(raw.len());
        String::from_utf16(&raw[..length]).map_err(|error| LctrlError::ChannelUnavailable {
            channel: format!("display API returned invalid UTF-16: {error}"),
        })
    }

    fn struct_size<T>() -> u32 {
        u32::try_from(std::mem::size_of::<T>()).expect("display structure size fits u32")
    }

    fn struct_size16<T>() -> u16 {
        u16::try_from(std::mem::size_of::<T>()).expect("DEVMODE size fits u16")
    }

    fn display_config_error(status: u32, context: &str) -> LctrlError {
        if status == ERROR_ACCESS_DENIED {
            LctrlError::PermissionDenied {
                need: "access to the active Windows desktop session".into(),
            }
        } else {
            map_win_error(status, context)
        }
    }

    fn display_change_result(status: i32) -> Result<()> {
        match status {
            DISP_CHANGE_SUCCESSFUL => Ok(()),
            DISP_CHANGE_RESTART => Err(LctrlError::FirmwareRejected {
                detail: "display driver requires a restart; immediate transition refused".into(),
            }),
            DISP_CHANGE_BADMODE => Err(LctrlError::Unsupported {
                feature: "panel.refresh.requested_native_mode".into(),
            }),
            DISP_CHANGE_BADFLAGS | DISP_CHANGE_BADPARAM => Err(LctrlError::InvalidArgument {
                detail: format!("ChangeDisplaySettingsExW rejected mode parameters ({status})"),
            }),
            DISP_CHANGE_NOTUPDATED => Err(LctrlError::PermissionDenied {
                need: "write access to the current user's display settings".into(),
            }),
            DISP_CHANGE_FAILED | DISP_CHANGE_BADDUALVIEW => Err(LctrlError::FirmwareRejected {
                detail: format!("ChangeDisplaySettingsExW failed with status {status}"),
            }),
            other => Err(LctrlError::Io(io::Error::other(format!(
                "ChangeDisplaySettingsExW returned unknown status {other}"
            )))),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{decode_fields, decode_shape};
        use windows_sys::Win32::Graphics::Gdi::{
            DEVMODEW, DM_BITSPERPEL, DM_DISPLAYFREQUENCY, DM_DISPLAYORIENTATION, DM_PELSHEIGHT,
            DM_PELSWIDTH, DM_POSITION,
        };

        const SHAPE_FIELDS: u32 =
            DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;

        fn raw_mode(fields: u32) -> DEVMODEW {
            let mut raw = DEVMODEW::default();
            raw.dmSize = u16::try_from(std::mem::size_of::<DEVMODEW>()).unwrap();
            raw.dmFields = fields;
            raw.dmPelsWidth = 3072;
            raw.dmPelsHeight = 1920;
            raw.dmBitsPerPel = 32;
            raw.dmDisplayFrequency = 60;
            raw
        }

        #[test]
        fn enumerated_entry_without_desktop_layout_still_yields_a_shape() {
            let raw = raw_mode(SHAPE_FIELDS);

            assert_eq!(decode_shape(&raw), Some((3072, 1920, 32, 60)));
            assert!(decode_fields(&raw).is_none());
        }

        #[test]
        fn current_mode_requires_the_full_desktop_layout() {
            let raw = raw_mode(SHAPE_FIELDS | DM_POSITION | DM_DISPLAYORIENTATION);
            let decoded = decode_fields(&raw).expect("full layout decodes");

            assert_eq!(decoded.refresh_hz, 60);
            assert!(decode_fields(&raw_mode(SHAPE_FIELDS | DM_POSITION)).is_none());
        }

        #[test]
        fn unknown_refresh_and_missing_geometry_are_rejected() {
            let mut legacy = raw_mode(SHAPE_FIELDS);
            legacy.dmDisplayFrequency = 1;
            assert!(decode_shape(&legacy).is_none());

            let mut no_bpp = raw_mode(SHAPE_FIELDS);
            no_bpp.dmBitsPerPel = 0;
            assert!(decode_shape(&no_bpp).is_none());
        }
    }
}

#[cfg(windows)]
pub use native::NativeDisplayApi;
