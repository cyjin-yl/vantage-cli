use std::{ffi::c_void, ptr};

use lctrl_core::{
    LctrlError, PowerScheme, PowerSchemeId, PowerSettingKey, PowerSource, PowerValueRange, Result,
};
use windows_sys::{
    Win32::{
        Foundation::LocalFree,
        System::{
            Power::{
                ACCESS_SCHEME, PowerEnumerate, PowerGetActiveScheme, PowerReadACValueIndex,
                PowerReadDCValueIndex, PowerReadValueIncrement, PowerReadValueMax,
                PowerReadValueMin, PowerSetActiveScheme, PowerWriteACValueIndex,
                PowerWriteDCValueIndex,
            },
            Registry::HKEY,
        },
    },
    core::GUID,
};

use crate::{PowerApi, map_win_error};

const ERROR_NO_MORE_ITEMS: u32 = 259;

#[derive(Clone, Copy, Debug, Default)]
pub struct NativePowerApi;

impl PowerApi for NativePowerApi {
    fn schemes(&self) -> Result<Vec<PowerScheme>> {
        let active = active_guid()?;
        let mut schemes = Vec::new();
        for index in 0.. {
            let mut guid = GUID::default();
            let mut size = u32::try_from(std::mem::size_of::<GUID>()).expect("GUID size fits u32");
            let status = unsafe {
                PowerEnumerate(
                    null_hkey(),
                    ptr::null(),
                    ptr::null(),
                    ACCESS_SCHEME,
                    index,
                    (&mut guid as *mut GUID).cast::<u8>(),
                    &mut size,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != 0 {
                return Err(map_win_error(status, "PowerEnumerate schemes"));
            }
            if size != std::mem::size_of::<GUID>() as u32 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("PowerEnumerate returned {size} bytes; expected GUID"),
                });
            }
            let id = PowerSchemeId::new(format_guid(&guid))?;
            schemes.push(PowerScheme::new(
                id.clone(),
                id.as_str(),
                guid_eq(&guid, &active),
            ));
        }
        Ok(schemes)
    }

    fn active_scheme(&self) -> Result<PowerScheme> {
        let guid = active_guid()?;
        let id = PowerSchemeId::new(format_guid(&guid))?;
        Ok(PowerScheme::new(id.clone(), id.as_str(), true))
    }

    fn activate(&self, id: &PowerSchemeId) -> Result<()> {
        let guid = parse_guid(id.as_str())?;
        let status = unsafe { PowerSetActiveScheme(null_hkey(), &guid) };
        if status == 0 {
            Ok(())
        } else {
            Err(map_win_error(status, "PowerSetActiveScheme"))
        }
    }

    fn read_value(&self, key: &PowerSettingKey, source: PowerSource) -> Result<u32> {
        let scheme = active_guid()?;
        let subgroup = parse_guid(key.subgroup.as_str())?;
        let setting = parse_guid(key.setting.as_str())?;
        let mut value = 0u32;
        let status = unsafe {
            match source {
                PowerSource::Ac => {
                    PowerReadACValueIndex(null_hkey(), &scheme, &subgroup, &setting, &mut value)
                }
                PowerSource::Dc => {
                    PowerReadDCValueIndex(null_hkey(), &scheme, &subgroup, &setting, &mut value)
                }
            }
        };
        if status == 0 {
            Ok(value)
        } else {
            Err(map_win_error(status, "PowerRead*ValueIndex"))
        }
    }

    fn range(&self, key: &PowerSettingKey) -> Result<PowerValueRange> {
        let subgroup = parse_guid(key.subgroup.as_str())?;
        let setting = parse_guid(key.setting.as_str())?;
        let min = read_range_value(PowerReadValueMin, &subgroup, &setting, "PowerReadValueMin")?;
        let max = read_range_value(PowerReadValueMax, &subgroup, &setting, "PowerReadValueMax")?;
        let increment = read_range_value(
            PowerReadValueIncrement,
            &subgroup,
            &setting,
            "PowerReadValueIncrement",
        )?;
        PowerValueRange::new(min, max, increment)
    }

    fn write_value(&self, key: &PowerSettingKey, source: PowerSource, value: u32) -> Result<()> {
        let scheme = active_guid()?;
        let subgroup = parse_guid(key.subgroup.as_str())?;
        let setting = parse_guid(key.setting.as_str())?;
        let status = unsafe {
            match source {
                PowerSource::Ac => {
                    PowerWriteACValueIndex(null_hkey(), &scheme, &subgroup, &setting, value)
                }
                PowerSource::Dc => {
                    PowerWriteDCValueIndex(null_hkey(), &scheme, &subgroup, &setting, value)
                }
            }
        };
        if status == 0 {
            Ok(())
        } else {
            Err(map_win_error(status, "PowerWrite*ValueIndex"))
        }
    }
}

type ReadRange = unsafe extern "system" fn(HKEY, *const GUID, *const GUID, *mut u32) -> u32;

fn read_range_value(
    reader: ReadRange,
    subgroup: &GUID,
    setting: &GUID,
    context: &str,
) -> Result<u32> {
    let mut value = 0u32;
    let status = unsafe { reader(null_hkey(), subgroup, setting, &mut value) };
    if status == 0 {
        Ok(value)
    } else {
        Err(map_win_error(status, context))
    }
}

fn active_guid() -> Result<GUID> {
    let mut pointer: *mut GUID = ptr::null_mut();
    let status = unsafe { PowerGetActiveScheme(null_hkey(), &mut pointer) };
    if status != 0 {
        return Err(map_win_error(status, "PowerGetActiveScheme"));
    }
    if pointer.is_null() {
        return Err(LctrlError::ChannelUnavailable {
            channel: "PowerGetActiveScheme returned a null GUID".into(),
        });
    }
    let guid = unsafe { *pointer };
    unsafe {
        LocalFree(pointer.cast::<c_void>());
    }
    Ok(guid)
}

fn null_hkey() -> HKEY {
    ptr::null_mut()
}

fn parse_guid(text: &str) -> Result<GUID> {
    let text = text.trim().trim_start_matches('{').trim_end_matches('}');
    let compact: String = text.chars().filter(|character| *character != '-').collect();
    if compact.len() != 32 || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(LctrlError::InvalidArgument {
            detail: format!("power identifier is not a GUID: {text:?}"),
        });
    }
    let raw = u128::from_str_radix(&compact, 16).map_err(|_| LctrlError::InvalidArgument {
        detail: format!("power identifier is not a GUID: {text:?}"),
    })?;
    Ok(GUID::from_u128(raw))
}

fn format_guid(guid: &GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

fn guid_eq(left: &GUID, right: &GUID) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}
