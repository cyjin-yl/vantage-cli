use lctrl_core::{MagicBayDevice, MagicBayKind, identify_magicbay};

#[must_use]
pub fn parse_magicbay_instance_id(instance_id: &str) -> Option<MagicBayDevice> {
    let upper = instance_id.to_ascii_uppercase();
    if let Some(vid_at) = upper.find("VID_17EF&PID_") {
        let pid_at = vid_at + "VID_17EF&PID_".len();
        let pid_text = upper.get(pid_at..pid_at + 4)?;
        let pid = u16::from_str_radix(pid_text, 16).ok()?;
        let kind = identify_magicbay(0x17ef, pid).map_or(MagicBayKind::Unknown, |known| known.kind);
        return Some(MagicBayDevice {
            path: instance_id.into(),
            bus: "usb".into(),
            vid: Some(0x17ef),
            pid: Some(pid),
            kind,
            interfaces: if pid == 0x7005 && upper.contains("MI_00") {
                vec!["mbim".into()]
            } else {
                Vec::new()
            },
            attached: true,
        });
    }
    if upper.contains("QCOM2488") || upper.contains("QCOM24B7") {
        return Some(MagicBayDevice {
            path: instance_id.into(),
            bus: "acpi".into(),
            vid: None,
            pid: None,
            kind: MagicBayKind::Hud,
            interfaces: vec!["display".into()],
            attached: true,
        });
    }
    None
}

#[cfg(windows)]
mod native {
    use std::{mem, ptr};

    use lctrl_core::{LctrlError, MagicBayDevice, Result};
    use lctrl_hal::MagicBayControl;
    use windows_sys::Win32::{
        Devices::DeviceAndDriverInstallation::{
            DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA,
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
            SetupDiGetDeviceInstanceIdW,
        },
        Foundation::GetLastError,
    };

    use super::parse_magicbay_instance_id;
    use crate::map_win_error;

    const ERROR_NO_MORE_ITEMS: u32 = 259;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeMagicBay;

    impl MagicBayControl for NativeMagicBay {
        fn detect_magicbay(&self) -> Result<Vec<MagicBayDevice>> {
            let set = DeviceInfoSet::open()?;
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
                let instance = instance_id(set.0, &info)?;
                if let Some(device) = parse_magicbay_instance_id(&instance) {
                    devices.push(device);
                }
            }
            devices.sort_by(|left, right| left.path.cmp(&right.path));
            devices.dedup_by(|left, right| left.path == right.path);
            Ok(devices)
        }
    }

    struct DeviceInfoSet(HDEVINFO);

    impl DeviceInfoSet {
        fn open() -> Result<Self> {
            let set = unsafe {
                SetupDiGetClassDevsW(
                    ptr::null(),
                    ptr::null(),
                    ptr::null_mut(),
                    DIGCF_PRESENT | DIGCF_ALLCLASSES,
                )
            };
            if set == -1isize {
                return Err(map_win_error(
                    unsafe { GetLastError() },
                    "SetupDiGetClassDevsW",
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
}

#[cfg(windows)]
pub use native::NativeMagicBay;
