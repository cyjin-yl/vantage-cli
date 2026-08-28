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
    let (kind, interface) = if upper.contains("QCOM2488") {
        (MagicBayKind::DisplayBridge, "display")
    } else if upper.contains("QCOM24B7") {
        (MagicBayKind::UsbRoleSwitch, "usb_role_switch")
    } else {
        return None;
    };
    Some(MagicBayDevice {
        path: instance_id.into(),
        bus: "acpi".into(),
        vid: None,
        pid: None,
        kind,
        interfaces: vec![interface.into()],
        attached: true,
    })
}

/// Merge SetupAPI's composite-parent and interface records into one accessory.
#[must_use]
pub fn coalesce_magicbay_devices(mut devices: Vec<MagicBayDevice>) -> Vec<MagicBayDevice> {
    devices.sort_by(|left, right| {
        is_usb_interface_instance(&left.path)
            .cmp(&is_usb_interface_instance(&right.path))
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut merged: Vec<MagicBayDevice> = Vec::with_capacity(devices.len());
    for device in devices {
        let device_is_interface = is_usb_interface_instance(&device.path);
        if device.bus == "usb" {
            if let Some(existing) = merged.iter_mut().find(|existing| {
                existing.bus == "usb"
                    && existing.vid == device.vid
                    && existing.pid == device.pid
                    && existing.kind == device.kind
                    && is_usb_merge_pair(existing, &device, device_is_interface)
            }) {
                merge_usb_interfaces(existing, &device);
                continue;
            }
        }
        merged.push(device);
    }
    let mut used = vec![false; merged.len()];
    let mut associated = Vec::with_capacity(merged.len());
    for index in 0..merged.len() {
        if used[index] {
            continue;
        }
        if !is_usb_interface_instance(&merged[index].path) && merged[index].bus == "usb" {
            let candidates: Vec<usize> = (0..merged.len())
                .filter(|candidate| {
                    !used[*candidate]
                        && is_usb_interface_instance(&merged[*candidate].path)
                        && merged[*candidate].bus == "usb"
                        && merged[*candidate].vid == merged[index].vid
                        && merged[*candidate].pid == merged[index].pid
                        && merged[*candidate].kind == merged[index].kind
                })
                .collect();
            let parent_count = (0..merged.len())
                .filter(|parent| {
                    !used[*parent]
                        && !is_usb_interface_instance(&merged[*parent].path)
                        && merged[*parent].bus == "usb"
                        && merged[*parent].vid == merged[index].vid
                        && merged[*parent].pid == merged[index].pid
                        && merged[*parent].kind == merged[index].kind
                })
                .count();
            if parent_count == 1 && candidates.len() == 1 {
                let mut parent = merged[index].clone();
                merge_usb_interfaces(&mut parent, &merged[candidates[0]]);
                used[index] = true;
                used[candidates[0]] = true;
                associated.push(parent);
                continue;
            }
        }
        used[index] = true;
        associated.push(merged[index].clone());
    }
    associated.sort_by(|left, right| left.path.cmp(&right.path));
    associated
}

fn merge_usb_interfaces(existing: &mut MagicBayDevice, candidate: &MagicBayDevice) {
    if is_usb_interface_instance(&existing.path) && !is_usb_interface_instance(&candidate.path) {
        existing.path = candidate.path.clone();
    }
    for interface in &candidate.interfaces {
        if !existing.interfaces.contains(interface) {
            existing.interfaces.push(interface.clone());
        }
    }
    existing.attached |= candidate.attached;
}

fn is_usb_merge_pair(
    existing: &MagicBayDevice,
    candidate: &MagicBayDevice,
    candidate_is_interface: bool,
) -> bool {
    let existing_is_interface = is_usb_interface_instance(&existing.path);
    let same_instance = match (
        usb_instance_key(&existing.path),
        usb_instance_key(&candidate.path),
    ) {
        (Some(existing), Some(candidate)) => existing == candidate,
        _ => false,
    };
    same_instance
        && (existing_is_interface || candidate_is_interface || existing.path == candidate.path)
}

fn usb_instance_key(path: &str) -> Option<String> {
    let upper = path.to_ascii_uppercase();
    let mut segments = upper.split('\\');
    let bus = segments.next()?;
    let hardware_id = segments.next()?.split("&MI_").next().unwrap_or_default();
    let instance = segments.next()?.split('&').next().unwrap_or_default();
    if hardware_id.is_empty() || instance.is_empty() {
        return None;
    }
    Some(format!("{bus}\\{hardware_id}\\{instance}"))
}

fn is_usb_interface_instance(path: &str) -> bool {
    path.as_bytes()
        .windows(4)
        .any(|window| window.eq_ignore_ascii_case(b"&MI_"))
}

#[cfg(windows)]
mod native {
    use std::{mem, ptr};

    use lctrl_core::{LctrlError, MagicBayDevice, MagicBayInventory, Result};
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

    use super::{coalesce_magicbay_devices, merge_usb_interfaces, parse_magicbay_instance_id};
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
                    merge_usb_interfaces(existing, &device);
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
