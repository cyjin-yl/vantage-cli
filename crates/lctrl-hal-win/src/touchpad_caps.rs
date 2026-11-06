//! Read-only HID capability descriptor of a Windows precision touchpad.
//!
//! The capability facts (declared contacts, surface ranges, button type, the
//! usages the collection declares) live in the parsed report descriptor of the
//! precision-touchpad top-level collection `HID_DEVICE_UP:000D_U:0005`. They are
//! not exposed through WMI, the PrecisionTouchPad registry trees or the HID
//! configuration collection — see `docs/evidence/re-touchpad-caps-20261105.md`.
//!
//! The native half opens the collection with `FILE_READ_ATTRIBUTES` and
//! `FILE_SHARE_READ | FILE_SHARE_WRITE` only: the Windows input stack holds the
//! collection, so `GENERIC_READ` answers `ERROR_SHARING_VIOLATION`. A failed
//! open is reported as `ChannelUnavailable`; it is never retried with a broader
//! access mask, because a refusal must not be turned into an assumed success.
//!
//! Everything in [`derive_capabilities`] is pure so the derivation can be
//! tested without a device.

use lctrl_core::{
    LctrlError, Result, TouchpadAxis, TouchpadButtonType, TouchpadButtons, TouchpadCapabilities,
    TouchpadContacts, TouchpadDerivedSize, TouchpadIdentity, TouchpadReports,
    TouchpadSupportBits, TouchpadSurface, TouchpadUsage,
};

/// USAGE_PAGE(Digitizers).
pub const DIGITIZER_USAGE_PAGE: u16 = 0x000D;
/// USAGE(Touch Pad) — the precision-touchpad top-level collection.
pub const TOUCHPAD_USAGE: u16 = 0x0005;
/// USAGE(Finger) — the link collection each contact is reported in.
const FINGER_USAGE: u16 = 0x0022;
/// USAGE(Contact Identifier).
const CONTACT_IDENTIFIER_USAGE: u16 = 0x0051;
/// USAGE(Contact Count).
const CONTACT_COUNT_USAGE: u16 = 0x0054;
/// USAGE(Contact Count Maximum) — device capabilities feature report.
const CONTACT_COUNT_MAXIMUM_USAGE: u16 = 0x0055;
/// USAGE(Scan Time) — 16-bit input field present in a precision touchpad report.
const SCAN_TIME_USAGE: u16 = 0x0056;
/// USAGE(Button Type / Pad Type) — device capabilities feature report.
const BUTTON_TYPE_USAGE: u16 = 0x0059;
/// USAGE(Tip Switch).
const TIP_SWITCH_USAGE: u16 = 0x0042;
/// USAGE(Confidence) — the palm-rejection bit.
const CONFIDENCE_USAGE: u16 = 0x0047;
/// USAGE(Pressure).
const PRESSURE_USAGE: u16 = 0x0030;
/// USAGE(Button Press Threshold) — haptic pressure-pad feature report.
const BUTTON_PRESS_THRESHOLD_USAGE: u16 = 0x00B0;

/// USAGE_PAGE(Generic Desktop).
const DESKTOP_USAGE_PAGE: u16 = 0x0001;
/// USAGE(X).
const X_USAGE: u16 = 0x0030;
/// USAGE(Y).
const Y_USAGE: u16 = 0x0031;
/// USAGE_PAGE(Button).
const BUTTON_USAGE_PAGE: u16 = 0x0009;
/// USAGE(Button 1) — the button integrated with the digitizer.
const BUTTON_1_USAGE: u16 = 0x0001;
/// USAGE_PAGE(Haptics).
const HAPTICS_USAGE_PAGE: u16 = 0x000E;
/// USAGE(Simple Haptic Controller).
const HAPTIC_CONTROLLER_USAGE: u16 = 0x0001;
/// USAGE(Intensity) — the device-initiated haptic intensity feature report.
const HAPTIC_INTENSITY_USAGE: u16 = 0x0023;

/// `UNIT(Inch, EngLinear)`: the unit a precision touchpad declares for X/Y.
const ENGLISH_LINEAR_INCH: u32 = 0x0013;
const MM_PER_INCH: f32 = 25.4;
/// A precision touchpad reports at most five concurrent contacts; a descriptor
/// that claims more link collections than this is not decoded silently.
const MAX_DECLARED_CONTACTS: usize = 16;

/// One declared value usage, reduced to the fields the capability contract
/// needs. `usage_min..=usage_max` collapses a single usage when the descriptor
/// declares a usage range instead of one usage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueCap {
    pub usage_page: u16,
    pub usage_min: u16,
    pub usage_max: u16,
    pub report_id: u8,
    pub link_collection: u16,
    pub link_usage_page: u16,
    pub link_usage: u16,
    pub bit_size: u16,
    pub report_count: u16,
    pub logical_min: i32,
    pub logical_max: i32,
    pub physical_min: i32,
    pub physical_max: i32,
    pub unit: u32,
    pub unit_exponent: i32,
}

impl ValueCap {
    /// Whether this declaration covers `usage` on `usage_page`.
    #[must_use]
    pub const fn declares(&self, usage_page: u16, usage: u16) -> bool {
        self.usage_page == usage_page && self.usage_min <= usage && usage <= self.usage_max
    }

    fn is_finger_collection(&self) -> bool {
        self.link_usage_page == DIGITIZER_USAGE_PAGE && self.link_usage == FINGER_USAGE
    }
}

/// One declared button usage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonCap {
    pub usage_page: u16,
    pub usage_min: u16,
    pub usage_max: u16,
    pub link_collection: u16,
}

impl ButtonCap {
    /// Whether this declaration covers `usage` on `usage_page`.
    #[must_use]
    pub const fn declares(&self, usage_page: u16, usage: u16) -> bool {
        self.usage_page == usage_page && self.usage_min <= usage && usage <= self.usage_max
    }
}

/// Values read back from the device capabilities feature report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    pub contact_count_maximum: u16,
    pub button_type: u16,
}

/// Everything the platform observed about one collection. The derivation below
/// is a pure function of this value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TouchpadDescriptor {
    pub instance_id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    pub product: Option<String>,
    pub usage_page: u16,
    pub usage: u16,
    pub input_report_bytes: u16,
    pub output_report_bytes: u16,
    pub feature_report_bytes: u16,
    pub link_collections: u16,
    pub input_values: Vec<ValueCap>,
    pub input_buttons: Vec<ButtonCap>,
    pub feature_values: Vec<ValueCap>,
    /// Absent when the capability feature report could not be read; the reason
    /// is then carried by [`Self::capability_report_error`].
    pub capability_report: Option<CapabilityReport>,
    pub capability_report_error: Option<String>,
}

/// Derive the reported capability contract from one observed descriptor.
///
/// Refusals are explicit: a non-PTP collection, a missing mandatory usage or a
/// truncated declaration is an error naming the usage instead of a defaulted
/// value.
pub fn derive_capabilities(descriptor: &TouchpadDescriptor) -> Result<TouchpadCapabilities> {
    if descriptor.usage_page != DIGITIZER_USAGE_PAGE || descriptor.usage != TOUCHPAD_USAGE {
        return Err(LctrlError::Unsupported {
            feature: format!(
                "touchpad.caps (top-level collection 0x{:04X}/0x{:04X} is not the precision-touchpad collection 0x{DIGITIZER_USAGE_PAGE:04X}/0x{TOUCHPAD_USAGE:04X})",
                descriptor.usage_page, descriptor.usage
            ),
        });
    }

    let finger_collections = finger_collections(descriptor)?;
    let x = declared_axis(descriptor, &finger_collections, X_USAGE)?;
    let y = declared_axis(descriptor, &finger_collections, Y_USAGE)?;
    let identifier_max = contact_identifier_max(descriptor, &finger_collections)?;
    let count_field_max = contact_count_field_max(descriptor)?;

    // The two remaining mandatory usages of the PTP input report: a relative
    // Scan Time at report level and a Tip switch per contact. Their absence
    // means this is not the collection the capability contract describes.
    if !descriptor
        .input_values
        .iter()
        .any(|cap| cap.declares(DIGITIZER_USAGE_PAGE, SCAN_TIME_USAGE))
    {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps input report declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{SCAN_TIME_USAGE:04X}"
            ),
        });
    }
    if !descriptor
        .input_buttons
        .iter()
        .any(|cap| cap.declares(DIGITIZER_USAGE_PAGE, TIP_SWITCH_USAGE))
    {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps input report declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{TIP_SWITCH_USAGE:04X} button"
            ),
        });
    }

    let declared_collections = u8::try_from(finger_collections.len()).map_err(|_| {
        LctrlError::ChannelUnavailable {
            channel: "touchpad.caps declares more finger collections than this build decodes"
                .into(),
        }
    })?;

    let count_maximum = descriptor
        .capability_report
        .map(|report| report.contact_count_maximum);
    let button_type = descriptor
        .capability_report
        .map(|report| TouchpadButtonType::from_raw(report.button_type));

    let support = support_bits(
        descriptor,
        &finger_collections,
        declared_collections,
        count_maximum,
    );
    let derived_size_mm = derived_size(x, y);

    Ok(TouchpadCapabilities {
        instance_id: descriptor.instance_id.clone(),
        usage: TouchpadUsage {
            page: descriptor.usage_page,
            usage: descriptor.usage,
        },
        identity: TouchpadIdentity {
            vendor_id: descriptor.vendor_id,
            product_id: descriptor.product_id,
            version: descriptor.version,
            product: descriptor.product.clone(),
        },
        reports: TouchpadReports {
            input_bytes: descriptor.input_report_bytes,
            output_bytes: descriptor.output_report_bytes,
            feature_bytes: descriptor.feature_report_bytes,
            link_collections: descriptor.link_collections,
        },
        contacts: TouchpadContacts {
            declared_collections,
            identifier_max,
            count_field_max,
            count_maximum,
        },
        surface: TouchpadSurface {
            x,
            y,
            derived_size_mm,
        },
        buttons: TouchpadButtons {
            button_type,
            integrated_button: descriptor
                .input_buttons
                .iter()
                .any(|cap| cap.declares(BUTTON_USAGE_PAGE, BUTTON_1_USAGE)),
        },
        support,
        capability_report_error: descriptor.capability_report_error.clone(),
    })
}

fn finger_collections(descriptor: &TouchpadDescriptor) -> Result<Vec<u16>> {
    let mut collections: Vec<u16> = Vec::new();
    for cap in descriptor
        .input_values
        .iter()
        .filter(|cap| cap.is_finger_collection())
    {
        if !collections.contains(&cap.link_collection) {
            collections.push(cap.link_collection);
        }
    }
    if collections.is_empty() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps input report declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{FINGER_USAGE:04X} finger collection"
            ),
        });
    }
    if collections.len() > MAX_DECLARED_CONTACTS {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps declares {} finger collections; refusing to decode more than {MAX_DECLARED_CONTACTS}",
                collections.len()
            ),
        });
    }
    Ok(collections)
}

/// Collapse one axis declaration across the contact collections. Every contact
/// must declare the same range: a disagreement means the descriptor cannot be
/// read as one surface.
fn declared_axis(
    descriptor: &TouchpadDescriptor,
    finger_collections: &[u16],
    usage: u16,
) -> Result<TouchpadAxis> {
    let mut axis: Option<TouchpadAxis> = None;
    let mut seen = false;
    for cap in descriptor.input_values.iter().filter(|cap| {
        cap.declares(DESKTOP_USAGE_PAGE, usage)
            && finger_collections.contains(&cap.link_collection)
    }) {
        seen = true;
        if cap.bit_size == 0 || cap.report_count != 1 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps 0x{DESKTOP_USAGE_PAGE:04X}/0x{usage:04X} is declared as {} bit(s) x {} value(s); only a single scalar axis is decoded",
                    cap.bit_size, cap.report_count
                ),
            });
        }
        if cap.logical_max <= cap.logical_min || cap.physical_max < cap.physical_min {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps 0x{DESKTOP_USAGE_PAGE:04X}/0x{usage:04X} declares logical {}..{} and physical {}..{}",
                    cap.logical_min, cap.logical_max, cap.physical_min, cap.physical_max
                ),
            });
        }
        let declared = TouchpadAxis {
            logical_min: cap.logical_min,
            logical_max: cap.logical_max,
            physical_min: cap.physical_min,
            physical_max: cap.physical_max,
            unit: cap.unit,
            unit_exponent: cap.unit_exponent,
        };
        match axis {
            Some(previous) if previous != declared => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "touchpad.caps contact collections disagree about 0x{DESKTOP_USAGE_PAGE:04X}/0x{usage:04X}"
                    ),
                });
            }
            Some(_) => {}
            None => axis = Some(declared),
        }
    }
    if !seen {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps input report declares no 0x{DESKTOP_USAGE_PAGE:04X}/0x{usage:04X} in a finger collection"
            ),
        });
    }
    axis.ok_or_else(|| LctrlError::ChannelUnavailable {
        channel: format!(
            "touchpad.caps 0x{DESKTOP_USAGE_PAGE:04X}/0x{usage:04X} has no readable declaration"
        ),
    })
}

fn contact_identifier_max(
    descriptor: &TouchpadDescriptor,
    finger_collections: &[u16],
) -> Result<i32> {
    let mut maximum: Option<i32> = None;
    for cap in descriptor.input_values.iter().filter(|cap| {
        cap.declares(DIGITIZER_USAGE_PAGE, CONTACT_IDENTIFIER_USAGE)
            && finger_collections.contains(&cap.link_collection)
    }) {
        if cap.bit_size == 0 || cap.logical_max <= cap.logical_min {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps 0x{DIGITIZER_USAGE_PAGE:04X}/0x{CONTACT_IDENTIFIER_USAGE:04X} declares {} bit(s) with logical {}..{}",
                    cap.bit_size, cap.logical_min, cap.logical_max
                ),
            });
        }
        maximum = Some(maximum.map_or(cap.logical_max, |known| known.max(cap.logical_max)));
    }
    maximum.ok_or_else(|| LctrlError::ChannelUnavailable {
        channel: format!(
            "touchpad.caps input report declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{CONTACT_IDENTIFIER_USAGE:04X}"
        ),
    })
}

fn contact_count_field_max(descriptor: &TouchpadDescriptor) -> Result<i32> {
    let mut maximum: Option<i32> = None;
    for cap in descriptor
        .input_values
        .iter()
        .filter(|cap| cap.declares(DIGITIZER_USAGE_PAGE, CONTACT_COUNT_USAGE))
    {
        if cap.bit_size == 0 || cap.report_count != 1 || cap.logical_max <= cap.logical_min {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps 0x{DIGITIZER_USAGE_PAGE:04X}/0x{CONTACT_COUNT_USAGE:04X} declares {} bit(s) x {} value(s) with logical {}..{}",
                    cap.bit_size, cap.report_count, cap.logical_min, cap.logical_max
                ),
            });
        }
        maximum = Some(maximum.map_or(cap.logical_max, |known| known.max(cap.logical_max)));
    }
    maximum.ok_or_else(|| LctrlError::ChannelUnavailable {
        channel: format!(
            "touchpad.caps input report declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{CONTACT_COUNT_USAGE:04X}"
        ),
    })
}

/// Capability bits are the input a Windows gesture needs, bounded by the number
/// of contacts the report can actually carry.
fn support_bits(
    descriptor: &TouchpadDescriptor,
    finger_collections: &[u16],
    declared_collections: u8,
    count_maximum: Option<u16>,
) -> TouchpadSupportBits {
    let capacity = match count_maximum {
        Some(maximum) => declared_collections.min(u8::try_from(maximum).unwrap_or(u8::MAX)),
        None => declared_collections,
    };
    TouchpadSupportBits {
        two_finger: capacity >= 2,
        three_finger: capacity >= 3,
        four_finger: capacity >= 4,
        five_finger: capacity >= 5,
        pressure: descriptor
            .input_values
            .iter()
            .any(|cap| cap.declares(DIGITIZER_USAGE_PAGE, PRESSURE_USAGE) && cap.is_finger_collection()),
        palm_confidence: descriptor
            .input_buttons
            .iter()
            .any(|cap| {
                cap.declares(DIGITIZER_USAGE_PAGE, CONFIDENCE_USAGE)
                    && finger_collections.contains(&cap.link_collection)
            }),
        haptic_intensity: descriptor.feature_values.iter().any(|cap| {
            cap.declares(HAPTICS_USAGE_PAGE, HAPTIC_INTENSITY_USAGE)
                && cap.link_usage_page == HAPTICS_USAGE_PAGE
                && cap.link_usage == HAPTIC_CONTROLLER_USAGE
        }),
        button_press_threshold: descriptor
            .feature_values
            .iter()
            .any(|cap| cap.declares(DIGITIZER_USAGE_PAGE, BUTTON_PRESS_THRESHOLD_USAGE)),
    }
}

/// Millimetres from the declared physical range. Only the unit a precision
/// touchpad declares (`UNIT(Inch, EngLinear)`) is converted; anything else
/// leaves the derived size absent rather than guessing a length unit.
fn derived_size(x: TouchpadAxis, y: TouchpadAxis) -> Option<TouchpadDerivedSize> {
    Some(TouchpadDerivedSize {
        width_mm: declared_millimetres(x)?,
        height_mm: declared_millimetres(y)?,
    })
}

fn declared_millimetres(axis: TouchpadAxis) -> Option<f32> {
    if axis.unit != ENGLISH_LINEAR_INCH || !(-6..=6).contains(&axis.unit_exponent) {
        return None;
    }
    let span = axis.physical_max - axis.physical_min;
    if span <= 0 {
        return None;
    }
    let scale = 10f32.powi(axis.unit_exponent);
    let millimetres = span as f32 * scale * MM_PER_INCH;
    if !millimetres.is_finite() || millimetres <= 0.0 {
        return None;
    }
    Some((millimetres * 10.0).round() / 10.0)
}

/// Read the capability descriptor of one admitted touchpad devnode.
#[cfg(windows)]
pub fn read_capabilities(instance_id: &str) -> Result<TouchpadCapabilities> {
    native::read_capabilities(instance_id)
}

/// A platform without a verified HID capability channel reports `Unsupported`.
#[cfg(not(windows))]
pub fn read_capabilities(_instance_id: &str) -> Result<TouchpadCapabilities> {
    Err(LctrlError::Unsupported {
        feature: "touchpad.caps".into(),
    })
}

#[cfg(windows)]
mod native {
    use std::{ffi::c_void, mem, ptr};

    use lctrl_core::{LctrlError, Result};
    use windows_sys::Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_ListW,
                CM_Get_Device_Interface_List_SizeW, CR_SUCCESS,
            },
            HumanInterfaceDevice::{
                GUID_DEVINTERFACE_HID, HIDD_ATTRIBUTES, HIDP_BUTTON_CAPS, HIDP_CAPS,
                HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidD_FreePreparsedData, HidD_GetAttributes,
                HidD_GetFeature, HidD_GetPreparsedData, HidD_GetProductString, HidP_Feature,
                HidP_GetButtonCaps, HidP_GetCaps, HidP_GetUsageValue, HidP_GetValueCaps, HidP_Input,
            },
        },
        Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GetLastError, HANDLE,
            INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    use super::{
        BUTTON_TYPE_USAGE, ButtonCap, CONTACT_COUNT_MAXIMUM_USAGE, CapabilityReport,
        DIGITIZER_USAGE_PAGE, TouchpadDescriptor, ValueCap,
    };

    const PRODUCT_STRING_CHARS: u32 = 256;

    /// The HID collection handle, closed on every exit path.
    struct CollectionHandle(HANDLE);

    impl CollectionHandle {
        /// Open the collection with the only mask this device class accepts.
        ///
        /// `GENERIC_READ` is held by the Windows input stack and answers
        /// `ERROR_SHARING_VIOLATION`; a failure here stays a failure.
        fn open(path: &str) -> Result<Self> {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    FILE_READ_ATTRIBUTES,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(open_error(unsafe { GetLastError() }));
            }
            Ok(Self(handle))
        }
    }

    impl Drop for CollectionHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open_error(code: u32) -> LctrlError {
        match code {
            ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION => LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps cannot open the precision-touchpad collection with FILE_READ_ATTRIBUTES and share READ|WRITE (Win32 error {code}); the input stack holds the collection, and this read does not fall back to a broader access mask"
                ),
            },
            other => crate::map_win_error(other, "touchpad.caps open precision-touchpad collection"),
        }
    }

    /// Parsed report descriptor, freed on every exit path.
    struct PreparsedData(isize);

    impl PreparsedData {
        fn new(handle: HANDLE) -> Result<Self> {
            let mut preparsed: isize = 0;
            if !unsafe { HidD_GetPreparsedData(handle, &mut preparsed) } {
                return Err(crate::map_win_error(
                    unsafe { GetLastError() },
                    "touchpad.caps HidD_GetPreparsedData",
                ));
            }
            Ok(Self(preparsed))
        }

        const fn raw(&self) -> isize {
            self.0
        }
    }

    impl Drop for PreparsedData {
        fn drop(&mut self) {
            unsafe { HidD_FreePreparsedData(self.0) };
        }
    }

    pub(super) fn read_capabilities(
        instance_id: &str,
    ) -> Result<lctrl_core::TouchpadCapabilities> {
        let path = interface_path(instance_id)?;
        let collection = CollectionHandle::open(&path)?;
        let descriptor = describe(collection.0, instance_id)?;
        super::derive_capabilities(&descriptor)
    }

    /// The HID device interface whose symbolic link carries this devnode.
    ///
    /// The link embeds the device instance id (`\\?\hid#<col>#<instance>#{guid}`),
    /// so it is matched against the admitted instance instead of assuming the
    /// first enumerated interface belongs to this pad.
    fn interface_path(instance_id: &str) -> Result<String> {
        let mut length: u32 = 0;
        let size = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut length,
                &GUID_DEVINTERFACE_HID,
                ptr::null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if size != CR_SUCCESS || length == 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps CM_Get_Device_Interface_List_SizeW failed (CONFIGRET 0x{size:08X})"
                ),
            });
        }
        let mut buffer = vec![0u16; length as usize];
        let listed = unsafe {
            CM_Get_Device_Interface_ListW(
                &GUID_DEVINTERFACE_HID,
                ptr::null(),
                buffer.as_mut_ptr(),
                length,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if listed != CR_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "touchpad.caps CM_Get_Device_Interface_ListW failed (CONFIGRET 0x{listed:08X})"
                ),
            });
        }

        let needle = instance_id.replace('\\', "#").to_ascii_lowercase();
        for link in buffer.split(|unit| *unit == 0) {
            if link.is_empty() {
                continue;
            }
            let link = String::from_utf16_lossy(link);
            if link.to_ascii_lowercase().contains(&needle) {
                return Ok(link);
            }
        }
        Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "touchpad.caps enumerated no HID interface for {instance_id}; the capability descriptor is unreadable"
            ),
        })
    }

    fn describe(handle: HANDLE, instance_id: &str) -> Result<TouchpadDescriptor> {
        let mut attributes = HIDD_ATTRIBUTES {
            Size: u32::try_from(mem::size_of::<HIDD_ATTRIBUTES>())
                .expect("HIDD_ATTRIBUTES size fits u32"),
            VendorID: 0,
            ProductID: 0,
            VersionNumber: 0,
        };
        if !unsafe { HidD_GetAttributes(handle, &mut attributes) } {
            return Err(crate::map_win_error(
                unsafe { GetLastError() },
                "touchpad.caps HidD_GetAttributes",
            ));
        }

        let mut product_buffer = vec![0u16; PRODUCT_STRING_CHARS as usize];
        let product = if unsafe {
            HidD_GetProductString(
                handle,
                product_buffer.as_mut_ptr().cast::<c_void>(),
                PRODUCT_STRING_CHARS * 2,
            )
        } {
            let text = String::from_utf16_lossy(&product_buffer);
            let text = text.trim_end_matches('\0').to_owned();
            (!text.is_empty()).then_some(text)
        } else {
            // A missing product string is not a capability: the identity stays
            // complete without it.
            None
        };

        let preparsed = PreparsedData::new(handle)?;
        let mut caps = HIDP_CAPS::default();
        check_status(
            unsafe { HidP_GetCaps(preparsed.raw(), &mut caps) },
            "HidP_GetCaps",
        )?;

        let input_values = value_caps(
            preparsed.raw(),
            HidP_Input,
            caps.NumberInputValueCaps,
        )?;
        let feature_values = value_caps(
            preparsed.raw(),
            HidP_Feature,
            caps.NumberFeatureValueCaps,
        )?;
        let input_buttons = button_caps(
            preparsed.raw(),
            HidP_Input,
            caps.NumberInputButtonCaps,
        )?;
        let (capability_report, capability_report_error) = capability_report(
            handle,
            preparsed.raw(),
            &caps,
            &feature_values,
        );

        Ok(TouchpadDescriptor {
            instance_id: instance_id.to_owned(),
            vendor_id: attributes.VendorID,
            product_id: attributes.ProductID,
            version: attributes.VersionNumber,
            product,
            usage_page: caps.UsagePage,
            usage: caps.Usage,
            input_report_bytes: caps.InputReportByteLength,
            output_report_bytes: caps.OutputReportByteLength,
            feature_report_bytes: caps.FeatureReportByteLength,
            link_collections: caps.NumberLinkCollectionNodes,
            input_values,
            input_buttons,
            feature_values,
            capability_report,
            capability_report_error,
        })
    }

    fn check_status(status: i32, call: &str) -> Result<()> {
        if status == HIDP_STATUS_SUCCESS {
            return Ok(());
        }
        Err(LctrlError::ChannelUnavailable {
            channel: format!("touchpad.caps {call} returned HID status 0x{status:08X}"),
        })
    }

    fn value_caps(preparsed: isize, report_type: i32, declared: u16) -> Result<Vec<ValueCap>> {
        // `HidP_GetCaps` already reported how many value caps this report type
        // has, so the array is sized from that count instead of a probe call.
        if declared == 0 {
            return Ok(Vec::new());
        }
        let mut raw = vec![HIDP_VALUE_CAPS::default(); usize::from(declared)];
        let mut length = declared;
        let status =
            unsafe { HidP_GetValueCaps(report_type, raw.as_mut_ptr(), &mut length, preparsed) };
        check_status(status, "HidP_GetValueCaps")?;
        raw.truncate(usize::from(length));
        Ok(raw.iter().map(reduce_value_cap).collect())
    }

    fn reduce_value_cap(cap: &HIDP_VALUE_CAPS) -> ValueCap {
        let (usage_min, usage_max) = unsafe {
            if cap.IsRange {
                (cap.Anonymous.Range.UsageMin, cap.Anonymous.Range.UsageMax)
            } else {
                let usage = cap.Anonymous.NotRange.Usage;
                (usage, usage)
            }
        };
        ValueCap {
            usage_page: cap.UsagePage,
            usage_min,
            usage_max,
            report_id: cap.ReportID,
            link_collection: cap.LinkCollection,
            link_usage_page: cap.LinkUsagePage,
            link_usage: cap.LinkUsage,
            bit_size: cap.BitSize,
            report_count: cap.ReportCount,
            logical_min: cap.LogicalMin,
            logical_max: cap.LogicalMax,
            physical_min: cap.PhysicalMin,
            physical_max: cap.PhysicalMax,
            unit: cap.Units,
            unit_exponent: exponent(cap.UnitsExp),
        }
    }

    fn button_caps(preparsed: isize, report_type: i32, declared: u16) -> Result<Vec<ButtonCap>> {
        if declared == 0 {
            return Ok(Vec::new());
        }
        let mut raw = vec![HIDP_BUTTON_CAPS::default(); usize::from(declared)];
        let mut length = declared;
        let status =
            unsafe { HidP_GetButtonCaps(report_type, raw.as_mut_ptr(), &mut length, preparsed) };
        check_status(status, "HidP_GetButtonCaps")?;
        raw.truncate(usize::from(length));
        Ok(raw.iter().map(reduce_button_cap).collect())
    }

    fn reduce_button_cap(cap: &HIDP_BUTTON_CAPS) -> ButtonCap {
        let (usage_min, usage_max) = unsafe {
            if cap.IsRange {
                (cap.Anonymous.Range.UsageMin, cap.Anonymous.Range.UsageMax)
            } else {
                let usage = cap.Anonymous.NotRange.Usage;
                (usage, usage)
            }
        };
        ButtonCap {
            usage_page: cap.UsagePage,
            usage_min,
            usage_max,
            link_collection: cap.LinkCollection,
        }
    }

    /// `HIDP_UNIT_EXPONENT` is a four-bit signed nibble; the capsule reports it
    /// as a `ULONG`, so the sign must be restored here.
    const fn exponent(raw: u32) -> i32 {
        let nibble = (raw & 0x0F) as i32;
        if nibble >= 8 { nibble - 16 } else { nibble }
    }

    /// Read the device capabilities feature report. A read failure leaves both
    /// declared values absent and records why; nothing is defaulted.
    fn capability_report(
        handle: HANDLE,
        preparsed: isize,
        caps: &HIDP_CAPS,
        feature_values: &[ValueCap],
    ) -> (Option<CapabilityReport>, Option<String>) {
        let Some(report_id) = capability_report_id(feature_values) else {
            return (
                None,
                Some(format!(
                    "descriptor declares no 0x{DIGITIZER_USAGE_PAGE:04X}/0x{CONTACT_COUNT_MAXIMUM_USAGE:04X} + 0x{DIGITIZER_USAGE_PAGE:04X}/0x{BUTTON_TYPE_USAGE:04X} capability feature report"
                )),
            );
        };
        let length = caps.FeatureReportByteLength;
        if length == 0 {
            return (
                None,
                Some("descriptor declares no feature report".to_owned()),
            );
        }
        let mut buffer = vec![0u8; usize::from(length)];
        buffer[0] = report_id;
        if !unsafe { HidD_GetFeature(handle, buffer.as_mut_ptr().cast::<c_void>(), u32::from(length)) }
        {
            return (
                None,
                Some(format!(
                    "HidD_GetFeature(report {report_id}) failed with Win32 error {}",
                    unsafe { GetLastError() }
                )),
            );
        }

        let mut contact_count_maximum: u32 = 0;
        let mut button_type: u32 = 0;
        for (usage, slot) in [
            (CONTACT_COUNT_MAXIMUM_USAGE, &mut contact_count_maximum),
            (BUTTON_TYPE_USAGE, &mut button_type),
        ] {
            let status = unsafe {
                HidP_GetUsageValue(
                    HidP_Feature,
                    DIGITIZER_USAGE_PAGE,
                    0,
                    usage,
                    slot,
                    preparsed,
                    buffer.as_ptr().cast(),
                    u32::from(length),
                )
            };
            if status != HIDP_STATUS_SUCCESS {
                return (
                    None,
                    Some(format!(
                        "HidP_GetUsageValue(0x{DIGITIZER_USAGE_PAGE:04X}/0x{usage:04X}) returned HID status 0x{status:08X}"
                    )),
                );
            }
        }

        let contact_count_maximum = u16::try_from(contact_count_maximum).ok();
        let button_type = u16::try_from(button_type).ok();
        match (contact_count_maximum, button_type) {
            (Some(contact_count_maximum), Some(button_type)) => (
                Some(CapabilityReport {
                    contact_count_maximum,
                    button_type,
                }),
                None,
            ),
            _ => (
                None,
                Some("capability feature report carries a value outside the 16-bit declaration"
                    .to_owned()),
            ),
        }
    }

    /// Both capability usages must live in one feature report; hidclass reports
    /// the report id per cap, so it is read from the descriptor instead of
    /// being assumed to be 2.
    fn capability_report_id(feature_values: &[ValueCap]) -> Option<u8> {
        let contact = feature_values
            .iter()
            .find(|cap| cap.declares(DIGITIZER_USAGE_PAGE, CONTACT_COUNT_MAXIMUM_USAGE))?;
        let button = feature_values
            .iter()
            .find(|cap| cap.declares(DIGITIZER_USAGE_PAGE, BUTTON_TYPE_USAGE))?;
        // The two usages belong to one report; a descriptor that splits them is
        // not the PTP device capabilities report and is not decoded.
        if contact.report_id != button.report_id {
            return None;
        }
        Some(contact.report_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BUTTON_1_USAGE, BUTTON_PRESS_THRESHOLD_USAGE, BUTTON_TYPE_USAGE, BUTTON_USAGE_PAGE,
        ButtonCap, CONTACT_COUNT_MAXIMUM_USAGE, CONTACT_COUNT_USAGE, CONTACT_IDENTIFIER_USAGE,
        CONFIDENCE_USAGE, CapabilityReport, DESKTOP_USAGE_PAGE, DIGITIZER_USAGE_PAGE,
        ENGLISH_LINEAR_INCH, FINGER_USAGE, HAPTIC_CONTROLLER_USAGE, HAPTIC_INTENSITY_USAGE,
        HAPTICS_USAGE_PAGE, LctrlError, PRESSURE_USAGE, TOUCHPAD_USAGE, TIP_SWITCH_USAGE,
        TouchpadButtonType, TouchpadDescriptor, ValueCap, derive_capabilities,
    };

    const X_USAGE: u16 = 0x0030;
    const Y_USAGE: u16 = 0x0031;
    const SCAN_TIME_USAGE: u16 = 0x0056;
    const CONTACT_LINKS: [u16; 5] = [1, 2, 3, 4, 5];

    fn finger_value(
        usage_page: u16,
        usage: u16,
        link: u16,
        bit_size: u16,
        logical_max: i32,
        physical_max: i32,
        unit: u32,
        unit_exponent: i32,
    ) -> ValueCap {
        ValueCap {
            usage_page,
            usage_min: usage,
            usage_max: usage,
            report_id: 4,
            link_collection: link,
            link_usage_page: DIGITIZER_USAGE_PAGE,
            link_usage: FINGER_USAGE,
            bit_size,
            report_count: 1,
            logical_min: 0,
            logical_max,
            physical_min: 0,
            physical_max,
            unit,
            unit_exponent,
        }
    }

    fn top_level_value(usage: u16, bit_size: u16, logical_max: i32) -> ValueCap {
        ValueCap {
            usage_page: DIGITIZER_USAGE_PAGE,
            usage_min: usage,
            usage_max: usage,
            report_id: 4,
            link_collection: 0,
            link_usage_page: DIGITIZER_USAGE_PAGE,
            link_usage: TOUCHPAD_USAGE,
            bit_size,
            report_count: 1,
            logical_min: 0,
            logical_max,
            physical_min: 0,
            physical_max: 65_535,
            unit: 0x1001,
            unit_exponent: -4,
        }
    }

    fn finger_button(usage: u16, link: u16) -> ButtonCap {
        ButtonCap {
            usage_page: DIGITIZER_USAGE_PAGE,
            usage_min: usage,
            usage_max: usage,
            link_collection: link,
        }
    }

    fn feature_value(usage_page: u16, usage: u16, report_id: u8, link: (u16, u16)) -> ValueCap {
        ValueCap {
            usage_page,
            usage_min: usage,
            usage_max: usage,
            report_id,
            link_collection: if usage_page == HAPTICS_USAGE_PAGE { 6 } else { 0 },
            link_usage_page: link.0,
            link_usage: link.1,
            bit_size: 8,
            report_count: 1,
            logical_min: 0,
            logical_max: 100,
            physical_min: 0,
            physical_max: 65_535,
            unit: 0x1001,
            unit_exponent: -4,
        }
    }

    /// The descriptor the reference machine published on 2026-11-06, reduced to
    /// the fields the derivation reads (`docs/evidence/re-touchpad-caps-20261105.md`).
    fn reference_descriptor() -> TouchpadDescriptor {
        let mut input_values = Vec::new();
        for link in CONTACT_LINKS {
            input_values.push(finger_value(
                DIGITIZER_USAGE_PAGE,
                CONTACT_IDENTIFIER_USAGE,
                link,
                4,
                15,
                0,
                0,
                0,
            ));
            input_values.push(finger_value(
                DESKTOP_USAGE_PAGE,
                X_USAGE,
                link,
                16,
                4_149,
                519,
                ENGLISH_LINEAR_INCH,
                -2,
            ));
            input_values.push(finger_value(
                DESKTOP_USAGE_PAGE,
                Y_USAGE,
                link,
                16,
                2_147,
                302,
                ENGLISH_LINEAR_INCH,
                -2,
            ));
            input_values.push(finger_value(
                DIGITIZER_USAGE_PAGE,
                PRESSURE_USAGE,
                link,
                16,
                2_000,
                2_000,
                0x0021,
                -2,
            ));
        }
        input_values.push(top_level_value(SCAN_TIME_USAGE, 16, 65_535));
        input_values.push(top_level_value(CONTACT_COUNT_USAGE, 8, 127));

        let mut input_buttons = Vec::new();
        for link in CONTACT_LINKS {
            input_buttons.push(finger_button(TIP_SWITCH_USAGE, link));
            input_buttons.push(finger_button(CONFIDENCE_USAGE, link));
        }
        input_buttons.push(ButtonCap {
            usage_page: BUTTON_USAGE_PAGE,
            usage_min: BUTTON_1_USAGE,
            usage_max: BUTTON_1_USAGE,
            link_collection: 0,
        });

        let touchpad_link = (DIGITIZER_USAGE_PAGE, TOUCHPAD_USAGE);
        let feature_values = vec![
            feature_value(
                DIGITIZER_USAGE_PAGE,
                BUTTON_TYPE_USAGE,
                2,
                touchpad_link,
            ),
            feature_value(
                DIGITIZER_USAGE_PAGE,
                CONTACT_COUNT_MAXIMUM_USAGE,
                2,
                touchpad_link,
            ),
            feature_value(
                HAPTICS_USAGE_PAGE,
                HAPTIC_INTENSITY_USAGE,
                9,
                (HAPTICS_USAGE_PAGE, HAPTIC_CONTROLLER_USAGE),
            ),
            feature_value(
                DIGITIZER_USAGE_PAGE,
                BUTTON_PRESS_THRESHOLD_USAGE,
                8,
                touchpad_link,
            ),
        ];

        TouchpadDescriptor {
            instance_id: "HID\\GXTP5100&COL02\\5&43d3fe9&0&0001".into(),
            vendor_id: 0x27C6,
            product_id: 0x01E7,
            version: 0xFF03,
            product: Some("HIDI2C Device".into()),
            usage_page: DIGITIZER_USAGE_PAGE,
            usage: TOUCHPAD_USAGE,
            input_report_bytes: 40,
            output_report_bytes: 0,
            feature_report_bytes: 737,
            link_collections: 7,
            input_values,
            input_buttons,
            feature_values,
            capability_report: Some(CapabilityReport {
                contact_count_maximum: 5,
                button_type: 1,
            }),
            capability_report_error: None,
        }
    }

    #[test]
    fn reference_descriptor_reports_the_observed_capabilities() {
        let caps = derive_capabilities(&reference_descriptor()).unwrap();
        assert_eq!(caps.usage.page, DIGITIZER_USAGE_PAGE);
        assert_eq!(caps.usage.usage, TOUCHPAD_USAGE);
        assert_eq!(caps.identity.vendor_id, 0x27C6);
        assert_eq!(caps.reports.feature_bytes, 737);
        assert_eq!(caps.contacts.declared_collections, 5);
        assert_eq!(caps.contacts.count_maximum, Some(5));
        assert_eq!(caps.contacts.identifier_max, 15);
        assert_eq!(caps.contacts.count_field_max, 127);
        assert_eq!(caps.surface.x.logical_max, 4_149);
        assert_eq!(caps.surface.y.physical_max, 302);
        let size = caps.surface.derived_size_mm.unwrap();
        assert_eq!(size.width_mm, 131.8);
        assert_eq!(size.height_mm, 76.7);
        assert_eq!(
            caps.buttons.button_type,
            Some(TouchpadButtonType::NonDepressible)
        );
        assert!(caps.buttons.integrated_button);
        assert!(caps.support.two_finger);
        assert!(caps.support.five_finger);
        assert!(caps.support.pressure);
        assert!(caps.support.palm_confidence);
        assert!(caps.support.haptic_intensity);
        assert!(caps.support.button_press_threshold);
        assert!(caps.capability_report_error.is_none());
    }

    #[test]
    fn unreadable_capability_report_leaves_both_values_absent() {
        let mut descriptor = reference_descriptor();
        descriptor.capability_report = None;
        descriptor.capability_report_error =
            Some("HidD_GetFeature(report 2) failed with Win32 error 32".into());

        let caps = derive_capabilities(&descriptor).unwrap();
        assert_eq!(caps.contacts.count_maximum, None);
        assert_eq!(caps.buttons.button_type, None);
        assert_eq!(
            caps.capability_report_error.as_deref(),
            Some("HidD_GetFeature(report 2) failed with Win32 error 32")
        );
        // The descriptor still declares the contacts, so the contact bits stay.
        assert!(caps.support.five_finger);
    }

    #[test]
    fn truncated_value_cap_refuses_instead_of_defaulting() {
        let mut descriptor = reference_descriptor();
        for cap in &mut descriptor.input_values {
            if cap.declares(DESKTOP_USAGE_PAGE, X_USAGE) {
                cap.bit_size = 0;
            }
        }
        assert!(matches!(
            derive_capabilities(&descriptor),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }

    #[test]
    fn missing_finger_collection_refuses() {
        let mut descriptor = reference_descriptor();
        descriptor.input_values.retain(|cap| !cap.is_finger_collection());
        descriptor.input_buttons.clear();
        assert!(matches!(
            derive_capabilities(&descriptor),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }

    #[test]
    fn contact_collections_that_disagree_about_an_axis_refuse() {
        let mut descriptor = reference_descriptor();
        for cap in &mut descriptor.input_values {
            if cap.declares(DESKTOP_USAGE_PAGE, X_USAGE) && cap.link_collection == 5 {
                cap.logical_max = 4_095;
            }
        }
        assert!(matches!(
            derive_capabilities(&descriptor),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }

    #[test]
    fn zero_unit_exponent_scales_by_the_declared_unit() {
        let mut descriptor = reference_descriptor();
        for cap in &mut descriptor.input_values {
            if cap.unit == ENGLISH_LINEAR_INCH {
                cap.unit_exponent = 0;
            }
        }
        let caps = derive_capabilities(&descriptor).unwrap();
        let size = caps.surface.derived_size_mm.unwrap();
        assert_eq!(size.width_mm, 13_182.6);
        assert_eq!(size.height_mm, 7_670.8);
    }

    #[test]
    fn unknown_unit_leaves_the_derived_size_absent() {
        let mut descriptor = reference_descriptor();
        for cap in &mut descriptor.input_values {
            if cap.unit == ENGLISH_LINEAR_INCH {
                cap.unit = 0x0011;
            }
        }
        let caps = derive_capabilities(&descriptor).unwrap();
        assert!(caps.surface.derived_size_mm.is_none());
        assert_eq!(caps.surface.x.unit, 0x0011);
    }

    #[test]
    fn non_touchpad_collection_is_unsupported() {
        let mut descriptor = reference_descriptor();
        // The MTConfig configuration collection of the same physical pad.
        descriptor.usage = 0x000E;
        assert!(matches!(
            derive_capabilities(&descriptor),
            Err(LctrlError::Unsupported { .. })
        ));
    }
}
