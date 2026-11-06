//! Windows Bluetooth adapter and device-list observation: enumeration only.
//!
//! `BluetoothFindFirstDevice` is called with `fIssueInquiry = FALSE`, so the
//! call reads the local device cache and never starts an inquiry. Nothing here
//! pairs, connects, removes, or reads pairing material.
//!
//! See `docs/evidence/re-network-20261105.md` for the recovered contract, the
//! live receipts, and the PnP/WinRT cross-checks.

use std::{
    cmp::Ordering,
    io,
    mem::size_of,
    ptr,
    time::{SystemTime, UNIX_EPOCH},
};

use lctrl_core::{LctrlError, Result};
use lctrl_hal::{
    BluetoothAdapter, BluetoothControl, BluetoothDeviceRecord, BluetoothInventory,
    BluetoothMajorClass, BluetoothTime,
};
use windows_sys::Win32::{
    Devices::Bluetooth::{
        BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS, BLUETOOTH_FIND_RADIO_PARAMS,
        BLUETOOTH_RADIO_INFO, BluetoothFindDeviceClose, BluetoothFindFirstDevice,
        BluetoothFindFirstRadio, BluetoothFindNextDevice, BluetoothFindNextRadio,
        BluetoothFindRadioClose, BluetoothGetRadioInfo, HBLUETOOTH_DEVICE_FIND,
        HBLUETOOTH_RADIO_FIND,
    },
    Foundation::{
        CloseHandle, ERROR_NO_MORE_ITEMS, FALSE, GetLastError, HANDLE, SYSTEMTIME, TRUE,
    },
};

/// Sizes the structures have on the verified contract
/// (`docs/evidence/re-network-20261105.md` §3.3). A different `windows-sys`
/// layout would silently change which field the service fills, so it is a
/// compile-time error here instead.
const _: () = assert!(size_of::<BLUETOOTH_RADIO_INFO>() == 520);
const _: () = assert!(size_of::<BLUETOOTH_DEVICE_INFO>() == 560);
const _: () = assert!(size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() == 40);

/// Bounds for a machine whose enumerator keeps returning items.
const MAX_ADAPTERS: usize = 16;
const MAX_DEVICES: usize = 512;

/// Read-only Bluetooth inventory service.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsBluetooth;

impl BluetoothControl for WindowsBluetooth {
    fn bluetooth_devices(&self) -> Result<BluetoothInventory> {
        let observed_at_unix_ms = now_unix_ms()?;
        Ok(BluetoothInventory {
            observed_at_unix_ms,
            adapters: adapters()?,
            devices: devices()?,
        })
    }
}

fn adapters() -> Result<Vec<BluetoothAdapter>> {
    let params = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32,
    };
    let mut radio: HANDLE = ptr::null_mut();
    let find = unsafe { BluetoothFindFirstRadio(&params, &mut radio) };
    if find.is_null() {
        let error = unsafe { GetLastError() };
        // No radio on this machine is an observation, not a failure.
        if error == ERROR_NO_MORE_ITEMS {
            return Ok(Vec::new());
        }
        return Err(crate::map_win_error(error, "BluetoothFindFirstRadio"));
    }
    let _find = RadioFind(find);
    let mut adapters: Vec<BluetoothAdapter> = Vec::new();
    loop {
        if adapters.len() >= MAX_ADAPTERS {
            return Err(LctrlError::Unsupported {
                feature: format!("Bluetooth reports more than {MAX_ADAPTERS} radios"),
            });
        }
        // The handle is closed even when the info read fails.
        let adapter = {
            let handle = RadioHandle(radio);
            radio_info(handle.0)?
        };
        adapters.push(adapter);
        radio = ptr::null_mut();
        if unsafe { BluetoothFindNextRadio(find, &mut radio) } != 0 {
            continue;
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_NO_MORE_ITEMS {
            break;
        }
        return Err(crate::map_win_error(error, "BluetoothFindNextRadio"));
    }
    adapters.sort_by(|left, right| left.address.cmp(&right.address));
    Ok(adapters)
}

fn radio_info(handle: HANDLE) -> Result<BluetoothAdapter> {
    let mut info = BLUETOOTH_RADIO_INFO {
        dwSize: size_of::<BLUETOOTH_RADIO_INFO>() as u32,
        ..Default::default()
    };
    let rc = unsafe { BluetoothGetRadioInfo(handle, &mut info) };
    if rc != 0 {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("BluetoothGetRadioInfo failed with Win32 error {rc} (0x{rc:08x})"),
        });
    }
    let class_of_device = present_class_of_device(info.ulClassofDevice);
    Ok(BluetoothAdapter {
        // SAFETY: the union's only members are integer/byte views of the same
        // 64-bit value, so every bit pattern is valid.
        address: format_address(unsafe { info.address.Anonymous.ullLong }),
        name: wide_name(&info.szName),
        class_of_device,
        major_class: class_of_device.map(BluetoothMajorClass::from_class_of_device),
        lmp_subversion: present_u16(info.lmpSubversion),
        manufacturer: present_u16(info.manufacturer),
    })
}

fn devices() -> Result<Vec<BluetoothDeviceRecord>> {
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: TRUE,
        fReturnRemembered: TRUE,
        // Devices the platform has merely seen in an inquiry are neither paired
        // nor in use; they are excluded deliberately (evidence §3.3/§3.4).
        fReturnUnknown: FALSE,
        fReturnConnected: TRUE,
        // The only parameter that starts an inquiry. Never set for a read-only verb.
        fIssueInquiry: FALSE,
        cTimeoutMultiplier: 0,
        // NULL searches every radio, which is what the service documents and
        // what keeps a device from being reported once per radio.
        hRadio: ptr::null_mut(),
    };
    let mut info = fresh_device_info();
    let find = unsafe { BluetoothFindFirstDevice(&params, &mut info) };
    if find.is_null() {
        let error = unsafe { GetLastError() };
        if error == ERROR_NO_MORE_ITEMS {
            return Ok(Vec::new());
        }
        return Err(crate::map_win_error(error, "BluetoothFindFirstDevice"));
    }
    let _find = DeviceFind(find);
    let mut devices: Vec<BluetoothDeviceRecord> = Vec::new();
    loop {
        if devices.len() >= MAX_DEVICES {
            return Err(LctrlError::Unsupported {
                feature: format!("Bluetooth reports more than {MAX_DEVICES} devices"),
            });
        }
        let record = device_record(&info);
        if !devices
            .iter()
            .any(|known| known.address == record.address)
        {
            devices.push(record);
        }
        info = fresh_device_info();
        if unsafe { BluetoothFindNextDevice(find, &mut info) } != 0 {
            continue;
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_NO_MORE_ITEMS {
            break;
        }
        return Err(crate::map_win_error(error, "BluetoothFindNextDevice"));
    }
    devices.sort_by(|left, right| {
        compare_names(left.name.as_deref(), right.name.as_deref())
            .then_with(|| left.address.cmp(&right.address))
    });
    Ok(devices)
}

fn fresh_device_info() -> BLUETOOTH_DEVICE_INFO {
    BLUETOOTH_DEVICE_INFO {
        dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    }
}

fn device_record(info: &BLUETOOTH_DEVICE_INFO) -> BluetoothDeviceRecord {
    let class_of_device = present_class_of_device(info.ulClassofDevice);
    BluetoothDeviceRecord {
        // SAFETY: as in `radio_info`, the union is a 64-bit value with no
        // invalid bit patterns.
        address: format_address(unsafe { info.Address.Anonymous.ullLong }),
        name: wide_name(&info.szName),
        class_of_device,
        major_class: class_of_device.map(BluetoothMajorClass::from_class_of_device),
        // Win32 BOOL semantics: zero is false and any non-zero value is true.
        // The target returned 0 / 16 / 8 for these fields (evidence §3.3), so an
        // equality test against 1 would have reported `false` for two fields
        // that are in fact set.
        connected: info.fConnected != 0,
        remembered: info.fRemembered != 0,
        authenticated: info.fAuthenticated != 0,
        last_seen: bluetooth_time(&info.stLastSeen),
        last_used: bluetooth_time(&info.stLastUsed),
    }
}

/// A class-of-device value of zero means "not set", not "miscellaneous".
fn present_class_of_device(value: u32) -> Option<u32> {
    if value == 0 { None } else { Some(value) }
}

fn present_u16(value: u16) -> Option<u16> {
    if value == 0 { None } else { Some(value) }
}

fn bluetooth_time(value: &SYSTEMTIME) -> Option<BluetoothTime> {
    BluetoothTime::new(
        value.wYear,
        value.wMonth,
        value.wDay,
        value.wHour,
        value.wMinute,
        value.wSecond,
    )
}

fn wide_name(value: &[u16]) -> Option<String> {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    let text = String::from_utf16_lossy(&value[..length]);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Formats the packed 48-bit address the API returns as `ullLong`.
fn format_address(value: u64) -> String {
    let mut text = String::with_capacity(17);
    for index in (0..6).rev() {
        if index < 5 {
            text.push(':');
        }
        text.push_str(&format!("{:02x}", (value >> (8 * index)) as u8));
    }
    text
}

fn compare_names(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase())),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

struct RadioFind(HBLUETOOTH_RADIO_FIND);

impl Drop for RadioFind {
    fn drop(&mut self) {
        unsafe {
            BluetoothFindRadioClose(self.0);
        }
    }
}

struct DeviceFind(HBLUETOOTH_DEVICE_FIND);

impl Drop for DeviceFind {
    fn drop(&mut self) {
        unsafe {
            BluetoothFindDeviceClose(self.0);
        }
    }
}

/// A radio handle the service opened for us; `CloseHandle` is the documented
/// release (there is no Bluetooth-specific radio close).
struct RadioHandle(HANDLE);

impl Drop for RadioHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn now_unix_ms() -> Result<u64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock is before the Unix epoch; cannot timestamp a Bluetooth observation",
        ))
    })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock timestamp exceeds the Bluetooth observation representation",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{compare_names, format_address};
    use std::cmp::Ordering;

    #[test]
    fn addresses_use_the_live_packed_byte_order() {
        // The radio reported ullLong 0x34fd70177491 and the local address
        // 34:fd:70:17:74:91 (evidence §3.3).
        assert_eq!(format_address(0x34fd_7017_7491), "34:fd:70:17:74:91");
        assert_eq!(format_address(0x0cbd_75ab_f23a), "0c:bd:75:ab:f2:3a");
        assert_eq!(format_address(0), "00:00:00:00:00:00");
    }

    #[test]
    fn device_names_sort_case_insensitively_before_unnamed_ones() {
        assert_eq!(
            compare_names(Some("oppo"), Some("ThinkBook")),
            Ordering::Greater
        );
        assert_eq!(compare_names(Some("abc"), Some("ABD")), Ordering::Less);
        assert_eq!(compare_names(Some("abc"), None), Ordering::Less);
        assert_eq!(compare_names(None, None), Ordering::Equal);
    }
}
