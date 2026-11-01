//! Panel image-quality features (eye-care, super-resolution) via the Intel
//! Graphics Control Library (`IntelControlLib.dll`, IGCL).
//!
//! IGCL is a public Intel SDK shipped with the graphics driver (non-Lenovo
//! driver component, allowed dependency). ABI stability argument
//! (docs/RE-interfaces.md §15): every call takes caller-sized,
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

/// Pinned IGCL API versions, tried in order. The public IGCL header ships
/// 1.1; the Intel driver packages observed on the reference target implement
/// 1.2. The negotiated version is verified against `SupportedVersion` after
/// init and only the implementation *major* version is required to match, so
/// a driver update cannot silently change the struct layouts we marshal
/// (every struct carries `Size`/`Version` fields).
const CTL_API_VERSIONS: [u32; 2] = [ctl_make_version(1, 2), ctl_make_version(1, 1)];
const CTL_IMPL_MAJOR_VERSION: u32 = 1;

const CTL_RESULT_SUCCESS: u32 = 0;

const CTL_PIXTX_BLOCK_TYPE_1D_LUT: u32 = 1;
const CTL_PIXTX_CONFIG_OPERTAION_TYPE_RESTORE_DEFAULT: u32 = 1;
const CTL_PIXTX_CONFIG_OPERTAION_TYPE_SET_CUSTOM: u32 = 2;
const CTL_PIXTX_LUT_SAMPLING_TYPE_UNIFORM: u32 = 0;
const CTL_PIXTX_CONFIG_QUERY_TYPE_CAPABILITY: u32 = 0;
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
/// read-only LUT comparison acceptance (docs/RE-interfaces.md §15).
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

#[derive(Clone, Copy)]
#[repr(C)]
struct CtlPixtx1dLutConfig {
    size: u32,
    version: u8,
    sampling_type: u32,
    num_samples_per_channel: u32,
    num_channels: u32,
    p_sample_values: *mut f64,
    p_sample_positions: *mut f64,
}

#[repr(C)]
union CtlPixtxConfig {
    one_d_lut_config: CtlPixtx1dLutConfig,
}

#[repr(C)]
struct CtlPixtxBlockConfig {
    size: u32,
    version: u8,
    block_id: u32,
    block_type: u32,
    config: CtlPixtxConfig,
}

#[repr(C)]
struct CtlPixtxPipeSetConfig {
    size: u32,
    version: u8,
    opertaion_type: u32,
    flags: u32,
    num_blocks: u32,
    p_block_configs: *mut CtlPixtxBlockConfig,
}

#[repr(C)]
struct CtlPixelFormat {
    size: u32,
    version: u8,
    bits_per_color: u32,
    is_float: bool,
    encoding_type: u32,
    color_space: u32,
    color_model: u32,
    color_primaries: u32,
    max_brightness: f64,
    min_brightness: f64,
}

#[repr(C)]
struct CtlPixtxPipeGetConfig {
    size: u32,
    version: u8,
    query_type: u32,
    input_pixel_format: CtlPixelFormat,
    output_pixel_format: CtlPixelFormat,
    num_blocks: u32,
    p_block_configs: *mut CtlPixtxBlockConfig,
}

#[repr(C)]
struct CtlVideoProcessingSuperResolutionInfo {
    size: u32,
    version: u8,
    super_resolution_flag: u32,
    reserved: [u32; 36],
}

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

pub struct IntelCtlHandles {
    _module: DllModule,
    pub(crate) ctl_init: unsafe extern "C" fn(*mut CtlInitArgs, *mut *mut c_void) -> u32,
    pub(crate) ctl_destroy: unsafe extern "C" fn(*mut c_void) -> u32,
    pub(crate) ctl_enumerate_devices:
        unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32,
    pub(crate) ctl_enumerate_display_outputs:
        unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32,
    pub(crate) ctl_pixel_transformation_set_config:
        unsafe extern "C" fn(*mut c_void, *mut CtlPixtxPipeSetConfig) -> u32,
    pub(crate) ctl_pixel_transformation_get_config:
        unsafe extern "C" fn(*mut c_void, *mut CtlPixtxPipeGetConfig) -> u32,
    pub(crate) ctl_get_set_video_processing_feature:
        unsafe extern "C" fn(*mut c_void, *mut CtlVideoProcessingFeatureGetSet) -> u32,
}

/// Trait seam over the IGCL transport so the feature layer is unit-testable.
pub trait IntelCtlApi: Send + Sync {
    /// First adapter's first display output handle, or `None` when the
    /// library loaded but no active output exists.
    fn primary_output(&self) -> Result<Option<*mut c_void>>;
    /// `None` restores the pipeline default; `Some(values)` installs a
    /// 256-sample RGB LUT (R then G then B, 768 doubles).
    fn set_lut(&self, values: Option<&[f64; LUT_SAMPLES * 3]>) -> Result<()>;
    /// Reads the current 1D LUT block configuration back.
    fn current_lut(&self) -> Result<[f64; LUT_SAMPLES * 3]>;
    fn super_resolution_flag(&self) -> Result<u32>;
    fn set_super_resolution_flag(&self, flag: u32) -> Result<()>;
    /// Whether the panel exposes a pixel-transformation (1D LUT) pipeline.
    fn has_pixel_transformation(&self) -> Result<bool>;
}

/// Dynamically loaded IntelControlLib.dll from the graphics driver store.
pub struct IntelControlLib {
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

            let handles = IntelCtlHandles {
                _module: DllModule { handle },
                ctl_init,
                ctl_destroy,
                ctl_enumerate_devices,
                ctl_enumerate_display_outputs,
                ctl_pixel_transformation_set_config,
                ctl_pixel_transformation_get_config,
                ctl_get_set_video_processing_feature,
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

    fn set_lut(&self, values: Option<&[f64; LUT_SAMPLES * 3]>) -> Result<()> {
        let _guard = self.state.lock();
        if self.primary_output.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "IntelControlLib has no active display output".to_string(),
            });
        }
        let (operation, num_blocks) = match values {
            Some(_) => (CTL_PIXTX_CONFIG_OPERTAION_TYPE_SET_CUSTOM, 1u32),
            None => (CTL_PIXTX_CONFIG_OPERTAION_TYPE_RESTORE_DEFAULT, 0u32),
        };
        let mut lut_storage = [0f64; LUT_SAMPLES * 3];
        let mut block = CtlPixtxBlockConfig {
            size: std::mem::size_of::<CtlPixtxBlockConfig>() as u32,
            version: 0,
            block_id: 0,
            block_type: CTL_PIXTX_BLOCK_TYPE_1D_LUT,
            config: CtlPixtxConfig {
                one_d_lut_config: CtlPixtx1dLutConfig {
                    size: std::mem::size_of::<CtlPixtx1dLutConfig>() as u32,
                    version: 0,
                    sampling_type: CTL_PIXTX_LUT_SAMPLING_TYPE_UNIFORM,
                    num_samples_per_channel: LUT_SAMPLES as u32,
                    num_channels: 3,
                    p_sample_values: if let Some(values) = values {
                        lut_storage.copy_from_slice(values);
                        lut_storage.as_mut_ptr()
                    } else {
                        std::ptr::null_mut()
                    },
                    p_sample_positions: std::ptr::null_mut(),
                },
            },
        };
        let mut pipe = CtlPixtxPipeSetConfig {
            size: std::mem::size_of::<CtlPixtxPipeSetConfig>() as u32,
            version: 0,
            opertaion_type: operation,
            flags: 0,
            num_blocks,
            p_block_configs: if values.is_some() {
                &mut block
            } else {
                std::ptr::null_mut()
            },
        };
        let result = unsafe {
            (self.handles.ctl_pixel_transformation_set_config)(self.primary_output, &mut pipe)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("ctlPixelTransformationSetConfig failed (result 0x{result:08x})"),
            });
        }
        Ok(())
    }

    fn current_lut(&self) -> Result<[f64; LUT_SAMPLES * 3]> {
        let _guard = self.state.lock();
        if self.primary_output.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "IntelControlLib has no active display output".to_string(),
            });
        }
        let mut samples = vec![0f64; LUT_SAMPLES * 3];
        let mut blocks = vec![CtlPixtxBlockConfig {
            size: std::mem::size_of::<CtlPixtxBlockConfig>() as u32,
            version: 0,
            block_id: 0,
            block_type: CTL_PIXTX_BLOCK_TYPE_1D_LUT,
            config: CtlPixtxConfig {
                one_d_lut_config: CtlPixtx1dLutConfig {
                    size: std::mem::size_of::<CtlPixtx1dLutConfig>() as u32,
                    version: 0,
                    sampling_type: CTL_PIXTX_LUT_SAMPLING_TYPE_UNIFORM,
                    num_samples_per_channel: LUT_SAMPLES as u32,
                    num_channels: 3,
                    p_sample_values: samples.as_mut_ptr(),
                    p_sample_positions: std::ptr::null_mut(),
                },
            },
        }];
        let mut config = CtlPixtxPipeGetConfig {
            size: std::mem::size_of::<CtlPixtxPipeGetConfig>() as u32,
            version: 0,
            query_type: CTL_PIXTX_CONFIG_QUERY_TYPE_CURRENT,
            input_pixel_format: pixel_format(),
            output_pixel_format: pixel_format(),
            num_blocks: blocks.len() as u32,
            p_block_configs: blocks.as_mut_ptr(),
        };
        let result = unsafe {
            (self.handles.ctl_pixel_transformation_get_config)(self.primary_output, &mut config)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "panel.eye-care.readback.ctlPixelTransformationGetConfig(0x{result:08x})"
                ),
            });
        }
        let mut lut = [0f64; LUT_SAMPLES * 3];
        lut.copy_from_slice(&samples[..LUT_SAMPLES * 3]);
        Ok(lut)
    }

    fn has_pixel_transformation(&self) -> Result<bool> {
        let _guard = self.state.lock();
        if self.primary_output.is_null() {
            return Ok(false);
        }
        let mut samples = vec![0f64; LUT_SAMPLES * 3];
        let mut blocks = vec![CtlPixtxBlockConfig {
            size: std::mem::size_of::<CtlPixtxBlockConfig>() as u32,
            version: 0,
            block_id: 0,
            block_type: CTL_PIXTX_BLOCK_TYPE_1D_LUT,
            config: CtlPixtxConfig {
                one_d_lut_config: CtlPixtx1dLutConfig {
                    size: std::mem::size_of::<CtlPixtx1dLutConfig>() as u32,
                    version: 0,
                    sampling_type: CTL_PIXTX_LUT_SAMPLING_TYPE_UNIFORM,
                    num_samples_per_channel: LUT_SAMPLES as u32,
                    num_channels: 3,
                    p_sample_values: samples.as_mut_ptr(),
                    p_sample_positions: std::ptr::null_mut(),
                },
            },
        }];
        let mut config = CtlPixtxPipeGetConfig {
            size: std::mem::size_of::<CtlPixtxPipeGetConfig>() as u32,
            version: 0,
            query_type: CTL_PIXTX_CONFIG_QUERY_TYPE_CAPABILITY,
            input_pixel_format: pixel_format(),
            output_pixel_format: pixel_format(),
            num_blocks: blocks.len() as u32,
            p_block_configs: blocks.as_mut_ptr(),
        };
        let result = unsafe {
            (self.handles.ctl_pixel_transformation_get_config)(self.primary_output, &mut config)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlPixelTransformationGetConfig(capability) failed (result 0x{result:08x})"
                ),
            });
        }
        Ok(config.num_blocks > 0)
    }

    fn super_resolution_flag(&self) -> Result<u32> {
        let _guard = self.state.lock();
        let mut info = CtlVideoProcessingSuperResolutionInfo {
            size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as u32,
            version: 0,
            super_resolution_flag: 0,
            reserved: [0; 36],
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
            custom_value_size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as i32,
            p_custom_value: &mut info as *mut _ as *mut c_void,
            reserved_fields: [0; 16],
        };
        let result = unsafe {
            (self.handles.ctl_get_set_video_processing_feature)(self.primary_adapter, &mut feature)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlGetSetVideoProcessingFeature(get SUPER_RESOLUTION) failed (result 0x{result:08x})"
                ),
            });
        }
        Ok(info.super_resolution_flag)
    }

    fn set_super_resolution_flag(&self, flag: u32) -> Result<()> {
        let _guard = self.state.lock();
        let mut info = CtlVideoProcessingSuperResolutionInfo {
            size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as u32,
            version: 0,
            super_resolution_flag: flag,
            reserved: [0; 36],
        };
        let mut feature = CtlVideoProcessingFeatureGetSet {
            size: std::mem::size_of::<CtlVideoProcessingFeatureGetSet>() as u32,
            version: 0,
            feature_type: CTL_VIDEO_PROCESSING_FEATURE_SUPER_RESOLUTION,
            application_name: std::ptr::null(),
            application_name_length: 0,
            b_set: true,
            value_type: CTL_PROPERTY_VALUE_TYPE_CUSTOM,
            value: [0; 8],
            custom_value_size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as i32,
            p_custom_value: &mut info as *mut _ as *mut c_void,
            reserved_fields: [0; 16],
        };
        let result = unsafe {
            (self.handles.ctl_get_set_video_processing_feature)(self.primary_adapter, &mut feature)
        };
        if result != CTL_RESULT_SUCCESS {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "ctlGetSetVideoProcessingFeature(set SUPER_RESOLUTION) failed (result 0x{result:08x})"
                ),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Feature layer.
// ---------------------------------------------------------------------------

pub struct WindowsPanelFeatures<T: IntelCtlApi> {
    api: T,
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
        if !self.api.has_pixel_transformation()? {
            return Err(LctrlError::Unsupported {
                feature: "panel.eye-care.no-pixel-transformation-pipeline".to_string(),
            });
        }
        match self.api.current_lut() {
            Ok(observed) => Ok(Some(classify_lut(&observed)?)),
            Err(error) => Err(error),
        }
    }

    fn set_eye_care(
        &self,
        level: EyeCareLevel,
        apply: ApplyMode,
    ) -> Result<ChangeReport<Option<EyeCareLevel>>> {
        let previous = self.eye_care().ok().flatten();
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, Some(level)));
        }
        match level {
            EyeCareLevel::Off => self.api.set_lut(None)?,
            EyeCareLevel::Mid | EyeCareLevel::High => {
                let lut = eye_care_lut(level);
                self.api.set_lut(Some(&lut))?;
            }
            EyeCareLevel::Custom => {
                return Err(LctrlError::InvalidArgument {
                    detail: "custom eye-care LUTs are not supported by the CLI".to_string(),
                });
            }
        }
        *self.last_eye_care.lock().unwrap_or_else(|e| e.into_inner()) = level;
        Ok(ChangeReport::committed(previous, Some(level), Some(level)))
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

fn pixel_format() -> CtlPixelFormat {
    CtlPixelFormat {
        size: std::mem::size_of::<CtlPixelFormat>() as u32,
        version: 0,
        bits_per_color: 0,
        is_float: false,
        encoding_type: 0,
        color_space: 0,
        color_model: 0,
        color_primaries: 0,
        max_brightness: 0.0,
        min_brightness: 0.0,
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
