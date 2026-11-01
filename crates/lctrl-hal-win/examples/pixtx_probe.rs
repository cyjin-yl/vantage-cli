//! Read-only probe for the Intel graphics pixel-transformation pipeline.
//!
//! Prints the current 1D LUT configuration of the first display output so the
//! eye-care readback path can be implemented against observed behaviour
//! instead of guesses. Usage: `cargo run --example pixtx_probe`.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

const CTL_RESULT_SUCCESS: u32 = 0;

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
#[derive(Clone, Copy)]
struct Ctl1dLutConfig {
    size: u32,
    version: u8,
    sampling_type: u32,
    num_samples_per_channel: u32,
    num_channels: u32,
    p_sample_values: *mut f64,
    p_sample_positions: *mut f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
union CtlPixtxConfig {
    one_d_lut: Ctl1dLutConfig,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CtlBlockConfig {
    size: u32,
    version: u8,
    block_id: u32,
    block_type: u32,
    config: CtlPixtxConfig,
}

#[repr(C)]
struct CtlPipeGetConfig {
    size: u32,
    version: u8,
    query_type: u32,
    input_pixel_format: CtlPixelFormat,
    output_pixel_format: CtlPixelFormat,
    num_blocks: u32,
    p_block_configs: *mut CtlBlockConfig,
}

fn main() {
    let path = std::path::PathBuf::from(
        r"C:\Windows\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_e55e939f4d7c3060\IntelControlLib.dll",
    );
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryW(wide.as_ptr()) };
    assert!(!module.is_null(), "load failed");
    let ctl_init: unsafe extern "C" fn(*mut CtlInitArgs, *mut *mut c_void) -> u32 = unsafe {
        std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            module,
            b"ctlInit\0".as_ptr().cast(),
        ))
    };
    let ctl_enumerate_devices: unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32 =
        unsafe {
            std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                module,
                b"ctlEnumerateDevices\0".as_ptr().cast(),
            ))
        };
    let ctl_enumerate_outputs: unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> u32 =
        unsafe {
            std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                module,
                b"ctlEnumerateDisplayOutputs\0".as_ptr().cast(),
            ))
        };
    let ctl_get_config: unsafe extern "C" fn(*mut c_void, *mut CtlPipeGetConfig) -> u32 = unsafe {
        std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            module,
            b"ctlPixelTransformationGetConfig\0".as_ptr().cast(),
        ))
    };

    let mut api: *mut c_void = std::ptr::null_mut();
    let mut init = CtlInitArgs {
        size: std::mem::size_of::<CtlInitArgs>() as u32,
        version: 0,
        app_version: (1 << 16) | 2,
        flags: 0,
        supported_version: 0,
        application_uid: CtlApplicationId {
            data1: 0,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        },
    };
    let result = unsafe { ctl_init(&mut init, &mut api) };
    println!("ctlInit result=0x{result:08x} supported=0x{:08x}", init.supported_version);
    assert_eq!(result, CTL_RESULT_SUCCESS);

    let mut count: u32 = 0;
    let result = unsafe { ctl_enumerate_devices(api, &mut count, std::ptr::null_mut()) };
    println!("enumerate devices result=0x{result:08x} count={count}");
    let mut adapters = vec![std::ptr::null_mut::<c_void>(); count as usize];
    let result = unsafe { ctl_enumerate_devices(api, &mut count, adapters.as_mut_ptr()) };
    println!("enumerate devices(2) result=0x{result:08x} count={count}");
    let adapter = adapters[0];

    let mut outputs: u32 = 0;
    let result = unsafe { ctl_enumerate_outputs(adapter, &mut outputs, std::ptr::null_mut()) };
    println!("enumerate outputs result=0x{result:08x} count={outputs}");
    let mut outs = vec![std::ptr::null_mut::<c_void>(); outputs as usize];
    let result = unsafe { ctl_enumerate_outputs(adapter, &mut outputs, outs.as_mut_ptr()) };
    println!("enumerate outputs(2) result=0x{result:08x} count={outputs}");
    let output = outs[0];

    // Pass 1: capability query, ask for the block count.
    let mut samples = vec![0f64; 4096];
    let mut blocks = vec![
        CtlBlockConfig {
            size: std::mem::size_of::<CtlBlockConfig>() as u32,
            version: 0,
            block_id: 0,
            block_type: 1,
            config: CtlPixtxConfig {
                one_d_lut: Ctl1dLutConfig {
                    size: std::mem::size_of::<Ctl1dLutConfig>() as u32,
                    version: 0,
                    sampling_type: 0,
                    num_samples_per_channel: 0,
                    num_channels: 3,
                    p_sample_values: samples.as_mut_ptr(),
                    p_sample_positions: std::ptr::null_mut(),
                },
            },
        };
        8
    ];
    // Pass 0: no block array at all - the driver should report the block
    // count and whether a pixel-transformation pipeline exists.
    // Sweep the block count to learn the pipeline shape.
    for num_blocks in 1u32..=8 {
        let mut samples = vec![0f64; 4096];
        let mut blocks = vec![
            CtlBlockConfig {
                size: std::mem::size_of::<CtlBlockConfig>() as u32,
                version: 0,
                block_id: 0,
                block_type: 1,
                config: CtlPixtxConfig {
                    one_d_lut: Ctl1dLutConfig {
                        size: std::mem::size_of::<Ctl1dLutConfig>() as u32,
                        version: 0,
                        sampling_type: 0,
                        num_samples_per_channel: 0,
                        num_channels: 3,
                        p_sample_values: samples.as_mut_ptr(),
                        p_sample_positions: std::ptr::null_mut(),
                    },
                },
            };
            num_blocks as usize
        ];
        for query in [0u32, 1u32] {
            let mut config = CtlPipeGetConfig {
                size: std::mem::size_of::<CtlPipeGetConfig>() as u32,
                version: 0,
                query_type: query,
                input_pixel_format: CtlPixelFormat {
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
                },
                output_pixel_format: CtlPixelFormat {
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
                },
                num_blocks,
                p_block_configs: blocks.as_mut_ptr(),
            };
            let result = unsafe { ctl_get_config(output, &mut config) };
            println!(
                "sweep blocks={num_blocks} query={query} result=0x{result:08x} reported={} ids={:?} samples={:?}",
                config.num_blocks,
                blocks.iter().map(|b| b.block_id).collect::<Vec<_>>(),
                blocks
                    .iter()
                    .map(|b| unsafe { b.config.one_d_lut }.num_samples_per_channel)
                    .collect::<Vec<_>>()
            );
        }
    }
    for query in [0u32, 1u32] {
        let mut config = CtlPipeGetConfig {
            size: std::mem::size_of::<CtlPipeGetConfig>() as u32,
            version: 0,
            query_type: query,
            input_pixel_format: CtlPixelFormat {
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
            },
            output_pixel_format: CtlPixelFormat {
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
            },
            num_blocks: blocks.len() as u32,
            p_block_configs: blocks.as_mut_ptr(),
        };
        let result = unsafe { ctl_get_config(output, &mut config) };
        println!(
            "getConfig query={query} result=0x{result:08x} num_blocks={} in_bits={} out_bits={}",
            config.num_blocks, config.input_pixel_format.bits_per_color, config.output_pixel_format.bits_per_color
        );
        for (index, block) in blocks.iter().enumerate() {
            let lut = unsafe { block.config.one_d_lut };
            println!(
                "  block[{index}] id={} type={} samples={} channels={} first={:?}",
                block.block_id,
                block.block_type,
                lut.num_samples_per_channel,
                lut.num_channels,
                &samples[..4]
            );
        }
    }

    // Video-processing capabilities (read-only): which features does this
    // adapter advertise?
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CtlPropertyRangeInfoUint {
        min_possible_value: u32,
        max_possible_value: u32,
        step_size: u32,
        default_value: u32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CtlPropertyInfoUint {
        default_enable: bool,
        range_info: CtlPropertyRangeInfoUint,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CtlVideoProcessingSuperResolutionInfo {
        size: u32,
        version: u8,
        super_resolution_flag: u32,
        super_resolution_range_in_width: CtlPropertyInfoUint,
        super_resolution_range_in_height: CtlPropertyInfoUint,
        reserved: [u32; 16],
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
    let ctl_get_sr: unsafe extern "C" fn(*mut c_void, *mut CtlVideoProcessingFeatureGetSet) -> u32 =
        unsafe {
            std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                module,
                b"ctlGetSetVideoProcessingFeature\0".as_ptr().cast(),
            ))
        };
    let mut sr_info = CtlVideoProcessingSuperResolutionInfo {
        size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as u32,
        version: 1,
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
        reserved: [0; 16],
    };
    let mut sr = CtlVideoProcessingFeatureGetSet {
        size: std::mem::size_of::<CtlVideoProcessingFeatureGetSet>() as u32,
        version: 1,
        feature_type: 4,
        application_name: b"\0".as_ptr().cast(),
        application_name_length: 0,
        b_set: false,
        value_type: 5,
        value: [0; 8],
        custom_value_size: std::mem::size_of::<CtlVideoProcessingSuperResolutionInfo>() as i32,
        p_custom_value: &mut sr_info as *mut _ as *mut c_void,
        reserved_fields: [0; 16],
    };
    for version in 0u8..=2 {
        for size in (96u32..=136).step_by(4) {
            sr.version = version;
            sr.size = size;
            sr_info.version = version;
            sr_info.super_resolution_flag = 0;
            let result = unsafe { ctl_get_sr(adapter, &mut sr) };
            if result == 0 {
                println!(
                    "SR get version={version} size={size} result=SUCCESS flag=0x{:08x}",
                    sr_info.super_resolution_flag
                );
            }
        }
    }

    #[repr(C)]
    struct CtlVideoProcessingFeatureCaps {
        size: u32,
        version: u8,
        num_supported_features: u32,
        p_feature_details: *mut c_void,
        reserved_fields: [u32; 16],
    }
    let ctl_get_caps: unsafe extern "C" fn(*mut c_void, *mut CtlVideoProcessingFeatureCaps) -> u32 = unsafe {
        std::mem::transmute_copy(&windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            module,
            b"ctlGetSupportedVideoProcessingCapabilities\0".as_ptr().cast(),
        ))
    };
    let mut caps = CtlVideoProcessingFeatureCaps {
        size: std::mem::size_of::<CtlVideoProcessingFeatureCaps>() as u32,
        version: 0,
        num_supported_features: 0,
        p_feature_details: std::ptr::null_mut(),
        reserved_fields: [0; 16],
    };
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CtlVideoProcessingFeatureDetails {
        size: u32,
        version: u8,
        feature_type: u32,
        value_type: u32,
        value: [u8; 20],
        custom_value_size: i32,
        p_custom_value: *mut c_void,
        reserved_fields: [u32; 16],
    }
    let make_detail = || CtlVideoProcessingFeatureDetails {
        size: std::mem::size_of::<CtlVideoProcessingFeatureDetails>() as u32,
        version: 0,
        feature_type: 0,
        value_type: 0,
        value: [0; 20],
        custom_value_size: 0,
        p_custom_value: std::ptr::null_mut(),
        reserved_fields: [0; 16],
    };
    let count = caps.num_supported_features;
    let mut details: Vec<CtlVideoProcessingFeatureDetails> = (0..count.max(1)).map(|_| make_detail()).collect();
    caps.p_feature_details = details.as_mut_ptr().cast();
    caps.num_supported_features = count;
    let result = unsafe { ctl_get_caps(adapter, &mut caps) };
    println!(
        "SR caps result=0x{result:08x} num_features={}",
        caps.num_supported_features
    );
    for detail in details.iter().take(caps.num_supported_features as usize) {
        println!(
            "  feature type={} value_type={} custom_size={}",
            detail.feature_type, detail.value_type, detail.custom_value_size
        );
    }
}
