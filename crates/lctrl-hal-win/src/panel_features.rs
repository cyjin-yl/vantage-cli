//! Panel image-quality features (eye-care, super-resolution) via the Intel
//! Graphics Control Library (`IntelControlLib.dll`, IGCL).
//!
//! IGCL is a public Intel SDK shipped with the graphics driver (non-Lenovo
//! driver component, allowed dependency). ABI stability argument
//! (docs/interfaces.md §15): every call takes caller-sized,
//! caller-versioned structs (`Size` + `Version` fields), the API version is
//! pinned at build time (`CTL_API_VERSION`) and the implementation version
//! reported by `ctlInit` is checked at runtime; unsupported versions fail
//! closed instead of mis-marshalling.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;

use lctrl_core::{ApplyMode, ChangeReport, EyeCareLevel, LctrlError, Result};
use lctrl_hal::PanelFeaturesControl;
use std::sync::Mutex as StdMutex;

struct DllModule {
    handle: *mut c_void,
}

impl Drop for DllModule {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::FreeLibrary(self.handle);
        }
    }
}

unsafe fn export<T>(module: *mut c_void, name: &[u8]) -> Result<T> {
    let address = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetProcAddress(module, name.as_ptr().cast())
    };
    if address.is_none() {
        let name = String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name));
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("IntelControlLib export `{name}` is missing"),
        });
    }
    Ok(unsafe { std::mem::transmute_copy(&address) })
}

fn load_library(path: &std::path::Path) -> Result<*mut c_void> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr()) };
    if handle.is_null() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "IntelControlLib.dll failed to load from {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ),
        });
    }
    Ok(handle)
}

/// `CTL_MAKE_VERSION(major, minor)` from the public IGCL header.
const fn ctl_make_version(major: u32, minor: u32) -> u32 {
    (major << 16) | (minor & 0x0000ffff)
}

/// Pinned IGCL API version. Every structure marshalled here follows the
/// public 1.1 header, so 1.1 is what we ask for: requesting a newer version
/// than the header we marshal against would be silent ABI drift. The
/// negotiated version is verified against `SupportedVersion` after init and
/// the implementation *major* version is required to match, so a driver
/// update cannot silently change the struct layouts (every struct also
/// carries its own `Size`/`Version` fields).
const CTL_API_VERSIONS: [u32; 1] = [ctl_make_version(1, 1)];
const CTL_IMPL_MAJOR_VERSION: u32 = 1;

const CTL_RESULT_SUCCESS: u32 = 0;
/// Upper bound on the LUT samples the driver may report per channel.
const CTL_RESULT_ERROR_INVALID_ARGUMENT: u32 = 0x4000_000b;

#[allow(dead_code)]
const CTL_PIXTX_BLOCK_TYPE_1D_LUT: u32 = 1;
#[allow(dead_code)]
const CTL_PIXTX_CONFIG_OPERTAION_TYPE_RESTORE_DEFAULT: u32 = 1;
#[allow(dead_code)]
const CTL_PIXTX_CONFIG_OPERTAION_TYPE_SET_CUSTOM: u32 = 2;
#[allow(dead_code)]
const CTL_PIXTX_LUT_SAMPLING_TYPE_UNIFORM: u32 = 0;
#[allow(dead_code)]
const CTL_PIXTX_CONFIG_QUERY_TYPE_CAPABILITY: u32 = 0;
#[allow(dead_code)]
const CTL_PIXTX_CONFIG_QUERY_TYPE_CURRENT: u32 = 1;

const CTL_VIDEO_PROCESSING_FEATURE_SUPER_RESOLUTION: u32 = 4;
const CTL_PROPERTY_VALUE_TYPE_CUSTOM: u32 = 5;
const CTL_VIDEO_PROCESSING_SUPER_RESOLUTION_FLAG_DISABLE: u32 = 1;
const CTL_VIDEO_PROCESSING_SUPER_RESOLUTION_FLAG_ENABLE_DEFAULT_SCENARIO_MODE: u32 = 2;

/// Eye-care color temperatures. Lenovo's own helper keeps a ~360-entry
/// Kelvin lookup table (`kelvin_table`, 1988K-19964K, recovered from
/// PlatformInterface.dll) but the UI-level defaults are delivered at runtime
/// through Vantage config, so the two levels below are the standard
/// Tanner-Helland approximation and are marked inferred pending the
/// read-only LUT comparison acceptance (docs/interfaces.md §15).
const EYE_CARE_MID_KELVIN: u32 = 5000;
const EYE_CARE_HIGH_KELVIN: u32 = 4000;
const LUT_SAMPLES: usize = 256;

// ---------------------------------------------------------------------------
// IGCL ABI layouts (public igcl_api.h).
// ---------------------------------------------------------------------------

/// `ctl_application_id_t` = { u32 Data1; u16 Data2; u16 Data3; u8 Data4[8] }.
#[repr(C)]
#[derive(Clone, Copy)]
struct CtlApplicationId {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct CtlInitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: CtlApplicationId,
}

/// `ctl_pixtx_color_primaries_t` — CIE 1931 chromaticity, carried inline in
/// the pixel format (public `ctl_pixtx_pixel_format_t`).
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlColorPrimaries {
    size: u32,
    version: u8,
    x_r: f64,
    y_r: f64,
    x_g: f64,
    y_g: f64,
    x_b: f64,
    y_b: f64,
    x_w: f64,
    y_w: f64,
}

/// `ctl_pixtx_pixel_format_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixelFormat {
    size: u32,
    version: u8,
    bits_per_color: u32,
    is_float: bool,
    encoding_type: u32,
    color_space: u32,
    color_model: u32,
    color_primaries: CtlColorPrimaries,
    max_brightness: f64,
    min_brightness: f64,
}

/// `ctl_pixtx_1dlut_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtx1dLutConfig {
    size: u32,
    version: u8,
    sampling_type: u32,
    num_samples_per_channel: u32,
    num_channels: u32,
    p_sample_values: *mut f64,
    p_sample_positions: *mut f64,
}

/// `ctl_pixtx_3dlut_sample_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtx3dLutSample {
    red: f64,
    green: f64,
    blue: f64,
}

/// `ctl_pixtx_3dlut_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtx3dLutConfig {
    size: u32,
    version: u8,
    num_samples_per_channel: u32,
    p_sample_values: *mut CtlPixtx3dLutSample,
}

/// `ctl_pixtx_matrix_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtxMatrixConfig {
    size: u32,
    version: u8,
    pre_offsets: [f64; 3],
    post_offsets: [f64; 3],
    matrix: [[f64; 3]; 3],
}

/// `ctl_pixtx_config_t` — the union carries every block payload the public
/// header defines; its size therefore follows the largest member.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
union CtlPixtxConfig {
    one_d_lut_config: CtlPixtx1dLutConfig,
    three_d_lut_config: CtlPixtx3dLutConfig,
    matrix_config: CtlPixtxMatrixConfig,
}

/// `ctl_pixtx_block_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtxBlockConfig {
    size: u32,
    version: u8,
    block_id: u32,
    block_type: u32,
    config: CtlPixtxConfig,
}

/// `ctl_pixtx_pipe_set_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtxPipeSetConfig {
    size: u32,
    version: u8,
    opertaion_type: u32,
    flags: u32,
    num_blocks: u32,
    p_block_configs: *mut CtlPixtxBlockConfig,
}

/// `ctl_pixtx_pipe_get_config_t`.
#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
struct CtlPixtxPipeGetConfig {
    size: u32,
    version: u8,
    query_type: u32,
    input_pixel_format: CtlPixelFormat,
    output_pixel_format: CtlPixelFormat,
    num_blocks: u32,
    p_block_configs: *mut CtlPixtxBlockConfig,
}

/// `ctl_property_range_info_uint_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlPropertyRangeInfoUint {
    min_possible_value: u32,
    max_possible_value: u32,
    step_size: u32,
    default_value: u32,
}

/// `ctl_property_info_uint_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlPropertyInfoUint {
    default_enable: bool,
    range_info: CtlPropertyRangeInfoUint,
}

/// `ctl_video_processing_super_resolution_info_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlVideoProcessingSuperResolutionInfo {
    size: u32,
    version: u8,
    super_resolution_flag: u32,
    super_resolution_range_in_width: CtlPropertyInfoUint,
    super_resolution_range_in_height: CtlPropertyInfoUint,
    reserved_fields: [u32; 16],
}

/// `ctl_video_processing_super_resolution_t` — the **get/set** payload.
///
/// This is not the 116-byte `..._info_t` capability payload: Intel's own
/// sample uses `sizeof(..._info_t)` for the capability query and
/// `sizeof(..._t)` for get/set, and passing the capability struct to the
/// get/set call is rejected with `INVALID_ARGUMENT`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlVideoProcessingSuperResolution {
    size: u32,
    version: u8,
    super_resolution_flag: u32,
    super_resolution_max_in_enabled: bool,
    super_resolution_max_in_width: u32,
    super_resolution_max_in_height: u32,
    super_resolution_reboot_reset: bool,
    reserved_fields: [u32; 15],
    reserved_bytes: [u8; 3],
}

/// `ctl_property_info_enum_t` (the 64-bit member that gives the property
/// info union its alignment).
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlPropertyInfoEnum {
    supported_types: u64,
    default_type: u32,
}

/// `ctl_property_info_t` union.
#[derive(Clone, Copy)]
#[repr(C)]
union CtlPropertyInfo {
    uint_info: CtlPropertyInfoUint,
    enum_info: CtlPropertyInfoEnum,
}

/// `ctl_video_processing_feature_details_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlVideoProcessingFeatureDetails {
    size: u32,
    version: u8,
    feature_type: u32,
    value_type: u32,
    value: CtlPropertyInfo,
    custom_value_size: i32,
    p_custom_value: *mut c_void,
    reserved_fields: [u32; 16],
}

/// `ctl_video_processing_feature_caps_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlVideoProcessingFeatureCaps {
    size: u32,
    version: u8,
    num_supported_features: u32,
    p_feature_details: *mut CtlVideoProcessingFeatureDetails,
    reserved_fields: [u32; 16],
}

/// `ctl_video_processing_feature_getset_t`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CtlVideoProcessingFeatureGetSet {
    size: u32,
    version: u8,
    feature_type: u32,
    application_name: *const u8,
    application_name_length: i8,
    b_set: bool,
    value_type: u32,
    value: [u8; 8],
    custom_value_size: i32,
    p_custom_value: *mut c_void,
    reserved_fields: [u32; 16],
}

// ---------------------------------------------------------------------------
// Transport seam: raw IGCL entry points against already-resolved handles.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct IntelCtlHandles {
    _module: DllModule,
    ctl_init: unsafe extern "C" fn(*mut CtlInitArgs, *mut *mut c_void) -> u32,
    ctl_destroy: unsafe extern "C" fn(*mut c_void) -> u32,
    ctl_enumerate_devices: unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32,
    ctl_enumerate_display_outputs:
        unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32,
    ctl_pixel_transformation_set_config:
        unsafe extern "C" fn(*mut c_void, *mut CtlPixtxPipeSetConfig) -> u32,
    ctl_pixel_transformation_get_config:
        unsafe extern "C" fn(*mut c_void, *mut CtlPixtxPipeGetConfig) -> u32,
    ctl_get_set_video_processing_feature:
        unsafe extern "C" fn(*mut c_void, *mut CtlVideoProcessingFeatureGetSet) -> u32,
    ctl_get_supported_video_processing_capabilities:
        unsafe extern "C" fn(*mut c_void, *mut CtlVideoProcessingFeatureCaps) -> u32,
}

/// Sanity bound on the block count a driver may report; a corrupt response
/// must not turn into an unbounded allocation.
const MAX_PIXTX_BLOCKS: u32 = 64;

/// One pixel-transform block as reported by the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixtxBlock {
    pub block_id: u32,
    pub block_type: u32,
    /// 1D LUT sampling geometry; only present for 1D LUT blocks, whose
    /// union member keeps these fields at their own offsets.
    pub sampling_type: Option<u32>,
    pub samples_per_channel: Option<u32>,
    pub channels: Option<u32>,
}

/// Pixel-transform pipeline capability of one display output. An empty
/// `blocks` vector is a positive answer: this output has no pipeline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PixtxCapability {
    pub blocks: Vec<PixtxBlock>,
}

/// Super-resolution capability, from the *capability* payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SuperResolutionCaps {
    pub flags: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

/// Trait seam over the IGCL transport so the feature layer is unit-testable.
pub trait IntelCtlApi: Send + Sync {
    /// First adapter's first display output handle, or `None` when the
    /// library loaded but no active output exists.
    fn primary_output(&self) -> Result<Option<*mut c_void>>;
    /// Current super-resolution flag as reported by the driver.
    fn super_resolution_flag(&self) -> Result<u32>;
    /// Request a super-resolution flag change.
    fn set_super_resolution_flag(&self, flag: u32) -> Result<()>;
    /// Pixel-transform capability of the primary display output. Read-only:
    /// the driver is asked for its capability, never for a configuration
    /// change. Implemented with the *display output* handle, which is the
    /// handle class this call requires.
    fn pixtx_capability(&self) -> Result<PixtxCapability>;
    /// Super-resolution capability as reported by the driver. Read-only.
    fn super_resolution_capability(&self) -> Result<SuperResolutionCaps>;
}

/// Dynamically loaded IntelControlLib.dll from the graphics driver store.
pub struct IntelControlLib {
    path: PathBuf,
    handles: IntelCtlHandles,
    api: *mut c_void,
    primary_adapter: *mut c_void,
    primary_output: *mut c_void,
    state: Mutex<()>,
}

// Safety: handles are raw pointers owned by the loaded library; all use is
// synchronized through `state`, and the module outlives this struct.
unsafe impl Send for IntelControlLib {}
unsafe impl Sync for IntelControlLib {}

impl IntelControlLib {
    /// Locate `IntelControlLib.dll` under the Intel graphics driver package
    /// and bind the entry points. No copying: the library stays in the
    /// driver store (system driver component).
    pub fn load() -> Result<Self> {
        let path = discover_intel_control_lib()?;
        let handle = load_library(&path)?;
        unsafe {
            let ctl_init: unsafe extern "C" fn(*mut CtlInitArgs, *mut *mut c_void) -> u32 =
                export(handle, b"ctlInit\0")?;
            let ctl_destroy: unsafe extern "C" fn(*mut c_void) -> u32 =
                export(handle, b"ctlClose\0")?;
            let ctl_enumerate_devices: unsafe extern "C" fn(
                *mut c_void,
                *mut u32,
                *mut *mut c_void,
            ) -> u32 = export(handle, b"ctlEnumerateDevices\0")?;
            let ctl_enumerate_display_outputs: unsafe extern "C" fn(
                *mut c_void,
                *mut u32,
                *mut *mut c_void,
            ) -> u32 = export(handle, b"ctlEnumerateDisplayOutputs\0")?;
            let ctl_pixel_transformation_set_config: unsafe extern "C" fn(
                *mut c_void,
                *mut CtlPixtxPipeSetConfig,
            ) -> u32 = export(handle, b"ctlPixelTransformationSetConfig\0")?;
            let ctl_pixel_transformation_get_config: unsafe extern "C" fn(
                *mut c_void,
                *mut CtlPixtxPipeGetConfig,
            ) -> u32 = export(handle, b"ctlPixelTransformationGetConfig\0")?;
            let ctl_get_set_video_processing_feature: unsafe extern "C" fn(
                *mut c_void,
                *mut CtlVideoProcessingFeatureGetSet,
            ) -> u32 = export(handle, b"ctlGetSetVideoProcessingFeature\0")?;

            let ctl_get_supported_video_processing_capabilities: unsafe extern "C" fn(
                *mut c_void,
                *mut CtlVideoProcessingFeatureCaps,
            )
                -> u32 = export(handle, b"ctlGetSupportedVideoProcessingCapabilities ")?;

            let handles = IntelCtlHandles {
                _module: DllModule { handle },
                ctl_init,
                ctl_destroy,
                ctl_enumerate_devices,
                ctl_enumerate_display_outputs,
                ctl_pixel_transformation_set_config,
                ctl_pixel_transformation_get_config,
                ctl_get_set_video_processing_feature,
                ctl_get_supported_video_processing_capabilities,
            };
            let mut api: *mut c_void = std::ptr::null_mut();
            let mut init = CtlInitArgs {
                size: std::mem::size_of::<CtlInitArgs>() as u32,
                version: 0,
                app_version: CTL_API_VERSIONS[0],
                flags: 0,
                supported_version: 0,
                application_uid: CtlApplicationId {
                    data1: 0,
                    data2: 0,
                    data3: 0,
                    data4: [0; 8],
                },
            };
            let mut result = u32::MAX;
            for candidate in CTL_API_VERSIONS {
                init.app_version = candidate;
                init.supported_version = 0;
                api = std::ptr::null_mut();
                result = (handles.ctl_init)(&mut init, &mut api);
                if result == CTL_RESULT_SUCCESS && !api.is_null() {
                    break;
                }
            }
            if result != CTL_RESULT_SUCCESS || api.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "IntelControlLib ctlInit failed (result 0x{result:08x}); pinned ABI versions {CTL_API_VERSIONS:?} rejected"
                    ),
                });
            }
            if init.supported_version >> 16 != CTL_IMPL_MAJOR_VERSION {
                let _ = (handles.ctl_destroy)(api);
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "IntelControlLib implementation major version {} is not ABI-compatible with pinned major {CTL_IMPL_MAJOR_VERSION}",
                        init.supported_version >> 16
                    ),
                });
            }

            let mut count: u32 = 0;
            let result = (handles.ctl_enumerate_devices)(api, &mut count, std::ptr::null_mut());
            if result != CTL_RESULT_SUCCESS || count == 0 {
                let _ = (handles.ctl_destroy)(api);
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("IntelControlLib ctlEnumerateDevices failed (0x{result:08x})"),
                });
            }
            let mut adapters = vec![std::ptr::null_mut::<c_void>(); count as usize];
            let result = (handles.ctl_enumerate_devices)(api, &mut count, adapters.as_mut_ptr());
            if result != CTL_RESULT_SUCCESS || adapters[0].is_null() {
                let _ = (handles.ctl_destroy)(api);
                return Err(LctrlError::ChannelUnavailable {
                    channel: "IntelControlLib returned no device adapter".to_string(),
                });
            }
            let primary_adapter = adapters[0];

            let mut outputs: u32 = 0;
            let result = (handles.ctl_enumerate_display_outputs)(
                primary_adapter,
                &mut outputs,
                std::ptr::null_mut(),
            );
            let primary_output = if result == CTL_RESULT_SUCCESS && outputs > 0 {
                let mut outs = vec![std::ptr::null_mut::<c_void>(); outputs as usize];
                let result = (handles.ctl_enumerate_display_outputs)(
                    primary_adapter,
                    &mut outputs,
                    outs.as_mut_ptr(),
                );
                if result == CTL_RESULT_SUCCESS {
                    outs[0]
                } else {
                    std::ptr::null_mut()
                }
            } else {
                std::ptr::null_mut()
            };

            Ok(Self {
                path,
                handles,
                api,
                primary_adapter,
                primary_output,
                state: Mutex::new(()),
            })
        }
    }
}

impl Drop for IntelControlLib {
    fn drop(&mut self) {
        let _guard = self.state.lock();
        unsafe {
            let _ = (self.handles.ctl_destroy)(self.api);
        }
    }
}

impl IntelCtlApi for IntelControlLib {
    fn primary_output(&self) -> Result<Option<*mut c_void>> {
        if self.primary_output.is_null() {
            Ok(None)
        } else {
            Ok(Some(self.primary_output))
        }
    }

    fn super_resolution_flag(&self) -> Result<u32> {
        let _guard = self.state.lock();
        // Get/set takes `ctl_video_processing_super_resolution_t` (92
        // bytes), not the 116-byte capability payload.
        let mut config = CtlVideoProcessingSuperResolution {
            size: std::mem::size_of::<CtlVideoProcessingSuperResolution>() as u32,
            version: 0,
            super_resolution_flag: 0,
            super_resolution_max_in_enabled: false,
            super_resolution_max_in_width: 0,
            super_resolution_max_in_height: 0,
            super_resolution_reboot_reset: false,
            reserved_fields: [0; 15],
            reserved_bytes: [0; 3],
        };
        let mut feature = CtlVideoProcessingFeatureGetSet {
            size: std::mem::size_of::<CtlVideoProcessingFeatureGetSet>() as u32,
            version: 0,
            feature_type: CTL_VIDEO_PROCESSING_FEATURE_SUPER_RESOLUTION,
            application_name: std::ptr::null(),
            application_name_length: 0,
            b_set: false,
            value_type: CTL_PROPERTY_VALUE_TYPE_CUSTOM,
            value: [0; 8],
            custom_value_size: std::mem::size_of::<CtlVideoProcessingSuperResolution>() as i32,
            p_custom_value: &mut config as *mut _ as *mut c_void,
            reserved_fields: [0; 16],
        };
        let result = unsafe {
            (self.handles.ctl_get_set_video_processing_feature)(self.primary_adapter, &mut feature)
        };
        if result == CTL_RESULT_ERROR_INVALID_ARGUMENT {
            // The driver advertises the feature (the capability query lists
            // it) but rejects this state query: the state stays unknown
            // instead of being invented. The public header also warns that
            // super-resolution needs activation by Intel, so a rejection is
            // not proof that the hardware lacks the feature.
            return Err(LctrlError::Unsupported {
                feature: "panel.super-resolution.state-query.rejected".to_string(),
            });
        }
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlGetSetVideoProcessingFeature(get SUPER_RESOLUTION) failed (result 0x{result:08x})"
                ),
            });
        }
        Ok(config.super_resolution_flag)
    }

    fn super_resolution_capability(&self) -> Result<SuperResolutionCaps> {
        let _guard = self.state.lock();
        // Capability queries use the 116-byte `..._info_t` payload.
        let mut info = CtlVideoProcessingSuperResolutionInfo {
            size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as u32,
            version: 0,
            super_resolution_flag: 0,
            super_resolution_range_in_width: CtlPropertyInfoUint {
                default_enable: false,
                range_info: CtlPropertyRangeInfoUint {
                    min_possible_value: 0,
                    max_possible_value: 0,
                    step_size: 0,
                    default_value: 0,
                },
            },
            super_resolution_range_in_height: CtlPropertyInfoUint {
                default_enable: false,
                range_info: CtlPropertyRangeInfoUint {
                    min_possible_value: 0,
                    max_possible_value: 0,
                    step_size: 0,
                    default_value: 0,
                },
            },
            reserved_fields: [0; 16],
        };
        let mut details = CtlVideoProcessingFeatureDetails {
            size: std::mem::size_of::<CtlVideoProcessingFeatureDetails>() as u32,
            version: 0,
            feature_type: CTL_VIDEO_PROCESSING_FEATURE_SUPER_RESOLUTION,
            value_type: CTL_PROPERTY_VALUE_TYPE_CUSTOM,
            value: CtlPropertyInfo {
                enum_info: CtlPropertyInfoEnum {
                    supported_types: 0,
                    default_type: 0,
                },
            },
            custom_value_size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as i32,
            p_custom_value: &mut info as *mut _ as *mut c_void,
            reserved_fields: [0; 16],
        };
        let mut caps = CtlVideoProcessingFeatureCaps {
            size: std::mem::size_of::<CtlVideoProcessingFeatureCaps>() as u32,
            version: 0,
            num_supported_features: 1,
            p_feature_details: &mut details,
            reserved_fields: [0; 16],
        };
        let result = unsafe {
            (self.handles.ctl_get_supported_video_processing_capabilities)(
                self.primary_adapter,
                &mut caps,
            )
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlGetSupportedVideoProcessingCapabilities(SUPER_RESOLUTION) failed (result 0x{result:08x})"
                ),
            });
        }
        Ok(SuperResolutionCaps {
            flags: info.super_resolution_flag,
            min_width: info
                .super_resolution_range_in_width
                .range_info
                .min_possible_value,
            max_width: info
                .super_resolution_range_in_width
                .range_info
                .max_possible_value,
            min_height: info
                .super_resolution_range_in_height
                .range_info
                .min_possible_value,
            max_height: info
                .super_resolution_range_in_height
                .range_info
                .max_possible_value,
        })
    }

    fn pixtx_capability(&self) -> Result<PixtxCapability> {
        let _guard = self.state.lock();
        if self.primary_output.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "IntelControlLib reported no display output handle".into(),
            });
        }
        let mut query = CtlPixtxPipeGetConfig {
            size: std::mem::size_of::<CtlPixtxPipeGetConfig>() as u32,
            version: 0,
            query_type: CTL_PIXTX_CONFIG_QUERY_TYPE_CAPABILITY,
            input_pixel_format: pixel_format(),
            output_pixel_format: pixel_format(),
            num_blocks: 0,
            p_block_configs: std::ptr::null_mut(),
        };
        // The call needs the display *output* handle; an adapter handle here
        // makes the driver walk unrelated internal objects.
        let result = unsafe {
            (self.handles.ctl_pixel_transformation_get_config)(self.primary_output, &mut query)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlPixelTransformationGetConfig(capability, no blocks) failed (result 0x{result:08x})"
                ),
            });
        }
        if query.num_blocks == 0 {
            // A successful query with zero blocks is a positive answer: this
            // output has no pixel-transform pipeline.
            return Ok(PixtxCapability::default());
        }
        if query.num_blocks > MAX_PIXTX_BLOCKS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "driver reported an implausible pixel-transform block count {}",
                    query.num_blocks
                ),
            });
        }
        let mut blocks = vec![empty_block_config(); query.num_blocks as usize];
        // The driver validates the supplied capacity, so the count is passed
        // back in even though it is annotated as an output field.
        query.num_blocks = blocks.len() as u32;
        query.p_block_configs = blocks.as_mut_ptr();
        let result = unsafe {
            (self.handles.ctl_pixel_transformation_get_config)(self.primary_output, &mut query)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlPixelTransformationGetConfig(capability, {} blocks) failed (result 0x{result:08x})",
                    blocks.len()
                ),
            });
        }
        let mut capability = PixtxCapability::default();
        for block in &blocks {
            let (sampling_type, samples_per_channel, channels) =
                if block.block_type == CTL_PIXTX_BLOCK_TYPE_1D_LUT {
                    // The 1D member is only meaningful for a 1D block: the
                    // union members keep their fields at different offsets.
                    let lut = unsafe { block.config.one_d_lut_config };
                    (
                        Some(lut.sampling_type),
                        Some(lut.num_samples_per_channel),
                        Some(lut.num_channels),
                    )
                } else {
                    (None, None, None)
                };
            capability.blocks.push(PixtxBlock {
                block_id: block.block_id,
                block_type: block.block_type,
                sampling_type,
                samples_per_channel,
                channels,
            });
        }
        Ok(capability)
    }

    fn set_super_resolution_flag(&self, flag: u32) -> Result<()> {
        let _guard = self.state.lock();
        // The state query for this feature is rejected by the driver, so the
        // write contract is unproven: refuse instead of writing blind.
        let _ = flag;
        Err(LctrlError::Unsupported {
            feature: "panel.super-resolution.write-unverified".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Feature layer.
// ---------------------------------------------------------------------------

pub struct WindowsPanelFeatures<T: IntelCtlApi> {
    api: T,
    #[allow(dead_code)]
    last_eye_care: StdMutex<EyeCareLevel>,
}

impl<T: IntelCtlApi> WindowsPanelFeatures<T> {
    pub fn new(api: T) -> Self {
        Self {
            api,
            last_eye_care: StdMutex::new(EyeCareLevel::Off),
        }
    }
}

/// Kelvin → per-channel gain (0..=1) using the Tanner-Helland approximation
/// (inferred; see module docs). Returns the channel intensity in 0..=255
/// normalized to 0..=1.
fn kelvin_channel_gain(kelvin: u32, channel: usize) -> f64 {
    let k = kelvin.clamp(1000, 40000) as f64;
    let value = match channel {
        0 if k < 6600.0 => 255.0,
        0 => 329.698_727_446 * (k - 60.0).powf(-0.133_204_759_2),
        1 if k < 6600.0 => 99.470_802_586_1 * k.ln() - 161.119_568_166_1,
        1 => 288.122_169_528_3 * (k - 60.0).powf(-0.075_514_849_2),
        _ if k <= 4900.0 => 0.0,
        _ if k < 6600.0 => 138.517_731_223_1 * (k - 10.0).ln() - 305.044_792_730_7,
        _ => 255.0,
    };
    (value / 255.0).clamp(0.0, 1.0)
}

/// Builds the 256-sample RGB LUT: R block then G then B, sample i scaled by
/// i/255 * channel gain.
#[allow(dead_code)]
pub fn eye_care_lut(level: EyeCareLevel) -> [f64; LUT_SAMPLES * 3] {
    let kelvin = match level {
        EyeCareLevel::Off | EyeCareLevel::Custom => return identity_lut(),
        EyeCareLevel::Mid => EYE_CARE_MID_KELVIN,
        EyeCareLevel::High => EYE_CARE_HIGH_KELVIN,
    };
    let mut lut = [0f64; LUT_SAMPLES * 3];
    for channel in 0..3 {
        let gain = kelvin_channel_gain(kelvin, channel);
        for sample in 0..LUT_SAMPLES {
            lut[channel * LUT_SAMPLES + sample] = (sample as f64 / 255.0) * gain;
        }
    }
    lut
}

#[allow(dead_code)]
pub fn identity_lut() -> [f64; LUT_SAMPLES * 3] {
    let mut lut = [0f64; LUT_SAMPLES * 3];
    for channel in 0..3 {
        for sample in 0..LUT_SAMPLES {
            lut[channel * LUT_SAMPLES + sample] = sample as f64 / 255.0;
        }
    }
    lut
}

/// Classifies an observed LUT against the known eye-care tables.
#[allow(dead_code)]
pub fn classify_lut(observed: &[f64; LUT_SAMPLES * 3]) -> Result<EyeCareLevel> {
    const EPSILON: f64 = 1.0 / 255.0;
    for (level, expected) in [
        (EyeCareLevel::Off, identity_lut()),
        (EyeCareLevel::Mid, eye_care_lut(EyeCareLevel::Mid)),
        (EyeCareLevel::High, eye_care_lut(EyeCareLevel::High)),
    ] {
        if observed
            .iter()
            .zip(expected.iter())
            .all(|(a, b)| (a - b).abs() <= EPSILON)
        {
            return Ok(level);
        }
    }
    Err(LctrlError::ChannelUnavailable {
        channel: "active 1D LUT does not match any known eye-care level".to_string(),
    })
}

impl<T: IntelCtlApi> PanelFeaturesControl for WindowsPanelFeatures<T> {
    fn eye_care(&self) -> Result<Option<EyeCareLevel>> {
        // The pixel-transformation block query is not reproducible against
        // this driver: it writes outside the published structure and
        // corrupts the caller's heap, so the channel stays closed rather than
        // risking the process (docs/interfaces.md §15.1).
        Err(LctrlError::Unsupported {
            feature: "panel.eye-care.pixel-transformation-query-unsupported".to_string(),
        })
    }

    fn set_eye_care(
        &self,
        _level: EyeCareLevel,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<Option<EyeCareLevel>>> {
        Err(LctrlError::Unsupported {
            feature: "panel.eye-care.pixel-transformation-query-unsupported".to_string(),
        })
    }

    fn super_resolution(&self) -> Result<Option<bool>> {
        match self.api.super_resolution_flag() {
            Ok(flag) => Ok(Some(
                flag & CTL_VIDEO_PROCESSING_SUPER_RESOLUTION_FLAG_ENABLE_DEFAULT_SCENARIO_MODE != 0,
            )),
            // The driver refuses the state query on this adapter: report the
            // state as unknown instead of inventing a value.
            Err(LctrlError::Unsupported { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn set_super_resolution(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<Option<bool>>> {
        let previous = self.super_resolution()?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, Some(enabled)));
        }
        let flag = if enabled {
            CTL_VIDEO_PROCESSING_SUPER_RESOLUTION_FLAG_ENABLE_DEFAULT_SCENARIO_MODE
        } else {
            CTL_VIDEO_PROCESSING_SUPER_RESOLUTION_FLAG_DISABLE
        };
        self.api.set_super_resolution_flag(flag)?;
        Ok(ChangeReport::committed(
            previous,
            Some(enabled),
            Some(enabled),
        ))
    }
}

#[allow(dead_code)]
fn pixel_format() -> CtlPixelFormat {
    CtlPixelFormat {
        size: std::mem::size_of::<CtlPixelFormat>() as u32,
        version: 0,
        bits_per_color: 0,
        is_float: false,
        encoding_type: 0,
        color_space: 0,
        color_model: 0,
        color_primaries: CtlColorPrimaries {
            size: 0,
            version: 0,
            x_r: 0.0,
            y_r: 0.0,
            x_g: 0.0,
            y_g: 0.0,
            x_b: 0.0,
            y_b: 0.0,
            x_w: 0.0,
            y_w: 0.0,
        },
        max_brightness: 0.0,
        min_brightness: 0.0,
    }
}

/// A zeroed block descriptor: no LUT sample buffers are supplied, which is
/// the documented shape for a *capability* query (the driver reports the
/// geometry instead of transferring samples).
fn empty_block_config() -> CtlPixtxBlockConfig {
    CtlPixtxBlockConfig {
        size: std::mem::size_of::<CtlPixtxBlockConfig>() as u32,
        version: 0,
        block_id: 0,
        block_type: 0,
        config: CtlPixtxConfig {
            matrix_config: CtlPixtxMatrixConfig {
                size: std::mem::size_of::<CtlPixtxMatrixConfig>() as u32,
                version: 0,
                pre_offsets: [0.0; 3],
                post_offsets: [0.0; 3],
                matrix: [[0.0; 3]; 3],
            },
        },
    }
}

impl IntelControlLib {
    /// Driver-store path of the loaded library, for diagnostics.
    #[must_use]
    pub fn library_path(&self) -> &std::path::Path {
        &self.path
    }
}

fn discover_intel_control_lib() -> Result<PathBuf> {
    let store = PathBuf::from(r"C:\Windows\System32\DriverStore\FileRepository");
    let entries = std::fs::read_dir(&store).map_err(|error| LctrlError::ChannelUnavailable {
        channel: format!(
            "cannot enumerate {} (Intel graphics driver required): {error}",
            store.display()
        ),
    })?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("iigd_dch.inf"))
        })
        .map(|path| path.join("IntelControlLib.dll"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "IntelControlLib.dll not found in any iigd_dch.inf driver package".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    /// The IGCL structures are ABI: every field offset and the struct size
    /// must match the published header, otherwise the driver rejects the
    /// call (INVALID_SIZE / INVALID_ARGUMENT) instead of describing the
    /// pipeline. These assertions pin the layout so a drift is caught here
    /// and not on the target machine.
    #[test]
    fn igcl_struct_layouts_match_the_published_header() {
        assert_eq!(size_of::<CtlColorPrimaries>(), 72);
        assert_eq!(align_of::<CtlColorPrimaries>(), 8);
        assert_eq!(offset_of!(CtlColorPrimaries, x_r), 8);

        assert_eq!(size_of::<CtlPixelFormat>(), 120);
        assert_eq!(align_of::<CtlPixelFormat>(), 8);
        assert_eq!(offset_of!(CtlPixelFormat, bits_per_color), 8);
        assert_eq!(offset_of!(CtlPixelFormat, color_primaries), 32);
        assert_eq!(offset_of!(CtlPixelFormat, max_brightness), 104);

        assert_eq!(size_of::<CtlPixtx1dLutConfig>(), 40);
        assert_eq!(offset_of!(CtlPixtx1dLutConfig, sampling_type), 8);
        assert_eq!(offset_of!(CtlPixtx1dLutConfig, p_sample_values), 24);

        // The union follows its largest member (the 3x3 matrix payload).
        assert_eq!(size_of::<CtlPixtxMatrixConfig>(), 128);
        assert_eq!(size_of::<CtlPixtxConfig>(), 128);
        assert_eq!(align_of::<CtlPixtxConfig>(), 8);

        assert_eq!(size_of::<CtlPixtxBlockConfig>(), 144);
        assert_eq!(offset_of!(CtlPixtxBlockConfig, config), 16);

        assert_eq!(size_of::<CtlPixtxPipeSetConfig>(), 32);
        assert_eq!(offset_of!(CtlPixtxPipeSetConfig, p_block_configs), 24);

        assert_eq!(size_of::<CtlPixtxPipeGetConfig>(), 272);
        assert_eq!(offset_of!(CtlPixtxPipeGetConfig, input_pixel_format), 16);
        assert_eq!(offset_of!(CtlPixtxPipeGetConfig, output_pixel_format), 136);
        assert_eq!(offset_of!(CtlPixtxPipeGetConfig, num_blocks), 256);

        assert_eq!(size_of::<CtlPropertyInfoUint>(), 20);
        assert_eq!(offset_of!(CtlPropertyInfoUint, range_info), 4);
        assert_eq!(size_of::<CtlPropertyRangeInfoUint>(), 16);
        assert_eq!(size_of::<CtlPropertyInfo>(), 24);
        assert_eq!(align_of::<CtlPropertyInfo>(), 8);
        assert_eq!(size_of::<CtlVideoProcessingFeatureDetails>(), 120);
        assert_eq!(offset_of!(CtlVideoProcessingFeatureDetails, value), 16);
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureDetails, custom_value_size),
            40
        );
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureDetails, p_custom_value),
            48
        );
        assert_eq!(size_of::<CtlVideoProcessingFeatureCaps>(), 88);
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureCaps, num_supported_features),
            8
        );
        assert_eq!(size_of::<CtlVideoProcessingSuperResolution>(), 92);
        assert_eq!(align_of::<CtlVideoProcessingSuperResolution>(), 4);
        assert_eq!(
            offset_of!(CtlVideoProcessingSuperResolution, super_resolution_flag),
            8
        );
        assert_eq!(
            offset_of!(CtlVideoProcessingSuperResolution, reserved_bytes),
            88
        );
        // Capability payload and get/set payload are different structs; the
        // sizes must never become interchangeable again.
        assert_ne!(
            size_of::<CtlVideoProcessingSuperResolutionInfo>(),
            size_of::<CtlVideoProcessingSuperResolution>()
        );
        // 1D and 3D LUT union members are not offset-compatible.
        assert_ne!(
            offset_of!(CtlPixtx1dLutConfig, p_sample_values),
            offset_of!(CtlPixtx3dLutConfig, p_sample_values)
        );
        assert_eq!(size_of::<CtlVideoProcessingSuperResolutionInfo>(), 116);
        assert_eq!(
            offset_of!(
                CtlVideoProcessingSuperResolutionInfo,
                super_resolution_range_in_width
            ),
            12
        );
        assert_eq!(
            offset_of!(
                CtlVideoProcessingSuperResolutionInfo,
                super_resolution_range_in_height
            ),
            32
        );

        assert_eq!(size_of::<CtlVideoProcessingFeatureGetSet>(), 120);
        assert_eq!(offset_of!(CtlVideoProcessingFeatureGetSet, feature_type), 8);
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureGetSet, application_name),
            16
        );
        assert_eq!(offset_of!(CtlVideoProcessingFeatureGetSet, value_type), 28);
        assert_eq!(offset_of!(CtlVideoProcessingFeatureGetSet, value), 32);
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureGetSet, custom_value_size),
            40
        );
        assert_eq!(
            offset_of!(CtlVideoProcessingFeatureGetSet, p_custom_value),
            48
        );
    }
}
