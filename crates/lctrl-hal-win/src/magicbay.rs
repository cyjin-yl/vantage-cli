//! Windows MagicBay detect path.
//!
//! The accessory identity table, the device-id parser and the record
//! coalescing are shared with the platform-independent core; this module only
//! adds the SetupAPI enumeration that produces the records.

pub use lctrl_core::{
    coalesce_magicbay_devices, parse_magicbay_device_id as parse_magicbay_instance_id,
};

#[cfg(windows)]
mod native {
    use std::{mem, ptr};

    use lctrl_core::{
        LctrlError, MagicBayDevice, MagicBayInventory, Result, merge_magicbay_interfaces,
    };
    use lctrl_hal::MagicBayControl;
    use windows_sys::{
        Win32::{
            Devices::{
                DeviceAndDriverInstallation::{
                    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA,
                    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
                },
                Properties::{DEVPKEY_Device_ContainerId, DEVPROP_TYPE_GUID},
            },
            Foundation::GetLastError,
        },
        core::GUID,
    };

    use super::{coalesce_magicbay_devices, parse_magicbay_instance_id};
    use crate::map_win_error;

    const ERROR_NO_MORE_ITEMS: u32 = 259;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeMagicBay;

    impl MagicBayControl for NativeMagicBay {
        fn detect_magicbay(&self) -> Result<MagicBayInventory> {
            let set = DeviceInfoSet::open()?;
            let mut inventory = MagicBayInventory::default();
            let mut usb_devices = Vec::new();
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
                    if device.bus == "acpi" {
                        inventory.acpi_devices.push(device);
                    } else {
                        usb_devices.push((device, container_id(set.0, &info)));
                    }
                }
            }
            inventory.devices = coalesce_native_devices(usb_devices);
            inventory
                .acpi_devices
                .sort_by(|left, right| left.path.cmp(&right.path));
            inventory
                .acpi_devices
                .dedup_by(|left, right| left.path == right.path);
            Ok(inventory)
        }
    }

    fn coalesce_native_devices(
        devices: Vec<(MagicBayDevice, Option<String>)>,
    ) -> Vec<MagicBayDevice> {
        let mut grouped: Vec<(MagicBayDevice, Option<String>)> = Vec::with_capacity(devices.len());
        for (device, container) in devices {
            if let Some(container) = container.as_ref() {
                if let Some((existing, _)) = grouped.iter_mut().find(|(existing, known)| {
                    known.as_ref() == Some(container)
                        && existing.bus == "usb"
                        && existing.vid == device.vid
                        && existing.pid == device.pid
                        && existing.kind == device.kind
                }) {
                    merge_magicbay_interfaces(existing, &device);
                    continue;
                }
            }
            grouped.push((device, container));
        }
        coalesce_magicbay_devices(grouped.into_iter().map(|(device, _)| device).collect())
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

    fn container_id(set: HDEVINFO, info: &SP_DEVINFO_DATA) -> Option<String> {
        let mut property_type = 0u32;
        let mut guid = GUID::default();
        let mut required = 0u32;
        let success = unsafe {
            SetupDiGetDevicePropertyW(
                set,
                info,
                &DEVPKEY_Device_ContainerId,
                &mut property_type,
                (&mut guid as *mut GUID).cast::<u8>(),
                u32::try_from(mem::size_of::<GUID>()).expect("GUID size fits u32"),
                &mut required,
                0,
            )
        };
        if success == 0
            || property_type != DEVPROP_TYPE_GUID
            || required < u32::try_from(mem::size_of::<GUID>()).ok()?
        {
            return None;
        }
        Some(format_guid(&guid))
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
            guid.data4[7],
        )
    }
}

#[cfg(windows)]
pub use native::NativeMagicBay;
