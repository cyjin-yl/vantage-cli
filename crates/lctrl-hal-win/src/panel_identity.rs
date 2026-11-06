//! Read-only identity of the embedded panel, decoded from its own EDID.
//!
//! Two channels are joined, and both must agree:
//!
//! 1. `root\wmi` `WmiMonitorConnectionParams` classifies every connected
//!    monitor's output technology. The embedded panel is the instance whose
//!    `VideoOutputTechnology` is `D3DKMDT_VOT_INTERNAL` (`0x80000000`). Exactly
//!    one such instance is admitted; zero means this machine exposes no
//!    embedded panel, several mean the topology is ambiguous, and both fail
//!    closed. This class is readable without an interactive desktop session,
//!    whereas display-path APIs describe the calling session's desktop: the
//!    SSH logon on the reference machine enumerates a virtual `WinDisc`
//!    1024x768 screen rather than the internal panel.
//! 2. `HKLM\SYSTEM\CurrentControlSet\Enum\<monitor instance>\Device Parameters\EDID`
//!    holds the panel's EDID bytes. The WMI instance name carries a trailing
//!    display index (`..._0`) that is stripped before the registry join, so the
//!    key is the full device instance path.
//!
//! The join must be exact for a reason: monitor EDID values are stored per PnP
//! instance, a disconnected monitor keeps its record, and one instance can carry
//! more than one class record (the reference machine has both `LEN8AD7` and
//! `CSW146D` for the same instance suffix). Selecting "the first DISPLAY record"
//! would read a panel that is not the active one, so the decoded manufacturer
//! and product code are also checked against the instance's PnP class.

use std::ptr;

use lctrl_core::{LctrlError, PanelIdentity, Result, decode_panel_edid};
use lctrl_hal::PanelIdentityControl;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_BINARY, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};

use crate::map_win_error;
use crate::wmi_contract::{ROOT_WMI, WmiObject, WmiTransport, WmiValue};

/// `D3DKMDT_VIDEO_OUTPUT_TECHNOLOGY` for an embedded panel. The WMI property is
/// an unsigned 32-bit value; the same constant read as a signed 32-bit one is
/// `i32::MIN`, so both encodings are accepted.
const VOT_INTERNAL_UNSIGNED: i64 = 0x8000_0000;
const VOT_INTERNAL_SIGNED: i64 = -2_147_483_648;

/// An EDID is 128 bytes per block and the base block declares the extension
/// count in one byte; 32 KiB bounds every legal value with room to spare.
const MAX_EDID_BYTES: u32 = 32 * 1024;

const CONNECTION_QUERY: &str =
    "SELECT InstanceName, VideoOutputTechnology FROM WmiMonitorConnectionParams";

/// The raw EDID value of one monitor device instance.
pub trait PanelRegistry: Send + Sync {
    fn edid(&self, device_instance: &str) -> Result<Vec<u8>>;
}

/// The panel's EDID as the PnP monitor driver stored it.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativePanelRegistry;

/// Identity of the embedded panel from `WmiMonitorConnectionParams` plus the
/// per-instance EDID registry value. Read-only: no display mode, topology, or
/// EDID value is written.
pub struct WindowsPanelIdentity<W, R> {
    wmi: W,
    registry: R,
}

impl<W, R> WindowsPanelIdentity<W, R> {
    #[must_use]
    pub const fn new(wmi: W, registry: R) -> Self {
        Self { wmi, registry }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }

    #[must_use]
    pub const fn registry(&self) -> &R {
        &self.registry
    }
}

impl<W, R> PanelIdentityControl for WindowsPanelIdentity<W, R>
where
    W: WmiTransport,
    R: PanelRegistry,
{
    fn identity(&self) -> Result<PanelIdentity> {
        let device_instance = internal_panel_device(&self.wmi)?;
        let bytes = self.registry.edid(&device_instance)?;
        let identity = decode_panel_edid(&bytes, true)?;
        if pnp_class_matches(
            &device_instance,
            &identity.manufacturer,
            identity.product_code,
        ) == Some(false)
        {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "EDID of {device_instance} describes {}{:04X}, which is a different monitor class",
                    identity.manufacturer, identity.product_code
                ),
            });
        }
        Ok(identity)
    }
}

impl PanelRegistry for NativePanelRegistry {
    fn edid(&self, device_instance: &str) -> Result<Vec<u8>> {
        let path =
            format!("SYSTEM\\CurrentControlSet\\Enum\\{device_instance}\\Device Parameters");
        let path = wide(&path);
        let mut key: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) };
        if status != 0 {
            return Err(map_win_error(
                status,
                "EDID registry key for the internal panel",
            ));
        }
        let key = RegistryKey(key);
        key.binary("EDID")
    }
}

struct RegistryKey(HKEY);

impl RegistryKey {
    fn binary(&self, name: &str) -> Result<Vec<u8>> {
        let name = wide(name);
        let mut kind = 0u32;
        let mut length = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut length,
            )
        };
        if status != 0 {
            return Err(map_win_error(status, "EDID registry value size"));
        }
        if kind != REG_BINARY || length == 0 || length > MAX_EDID_BYTES {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "EDID registry value is of type {kind} and {length} bytes, not a bounded REG_BINARY value"
                ),
            });
        }
        let mut buffer = vec![0u8; length as usize];
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if status != 0 {
            return Err(map_win_error(status, "EDID registry value"));
        }
        if kind != REG_BINARY || length as usize > buffer.len() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "EDID registry value changed type or length between reads".into(),
            });
        }
        buffer.truncate(length as usize);
        Ok(buffer)
    }
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

/// Pick the single monitor connection the OS classifies as internal and return
/// its device instance path.
///
/// This is also the availability probe for the `panel.identity` capability: it
/// only reads `WmiMonitorConnectionParams`, so it proves the selection channel
/// without touching the registry.
pub(crate) fn internal_panel_device<W: WmiTransport>(wmi: &W) -> Result<String> {
    let connections = wmi.query(ROOT_WMI, CONNECTION_QUERY)?;
    select_internal_instance(&connections)
}

fn select_internal_instance(connections: &[WmiObject]) -> Result<String> {
    let mut internal = Vec::new();
    for connection in connections {
        let Some(instance) = connection.get("InstanceName") else {
            return Err(LctrlError::ChannelUnavailable {
                channel: "monitor connection record has no InstanceName".into(),
            });
        };
        let WmiValue::String(instance) = instance else {
            return Err(LctrlError::ChannelUnavailable {
                channel: "monitor connection InstanceName is not a string".into(),
            });
        };
        let technology = connection
            .get("VideoOutputTechnology")
            .and_then(numeric)
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!(
                    "monitor connection {instance} has no numeric VideoOutputTechnology"
                ),
            })?;
        if technology == VOT_INTERNAL_UNSIGNED || technology == VOT_INTERNAL_SIGNED {
            internal.push(instance.clone());
        }
    }
    match internal.len() {
        0 => Err(LctrlError::Unsupported {
            feature: "panel.identity".into(),
        }),
        1 => device_instance_of(&internal[0]),
        count => Err(LctrlError::ChannelUnavailable {
            channel: format!(
                "{count} monitor connections report an internal panel; the embedded panel is ambiguous"
            ),
        }),
    }
}

/// Strip the WMI display index (`..._0`) from a monitor instance name.
fn device_instance_of(instance_name: &str) -> Result<String> {
    let (device, index) = instance_name
        .rsplit_once('_')
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("monitor instance {instance_name} carries no display index"),
        })?;
    if device.is_empty() || index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("monitor instance {instance_name} does not end in a display index"),
        });
    }
    Ok(device.to_owned())
}

/// `Some(true)`/`Some(false)` when the instance's class component is a monitor
/// PnP id, `None` when the generic class (for example `Default_Monitor`) is in
/// use and no cross-check is possible.
fn pnp_class_matches(
    device_instance: &str,
    manufacturer: &str,
    product_code: u16,
) -> Option<bool> {
    let class = device_instance.split('\\').nth(1)?;
    let class_manufacturer = class.get(..3)?;
    let product = class.get(3..)?;
    if product.len() != 4 || !class_manufacturer.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    let product = u16::from_str_radix(product, 16).ok()?;
    Some(class_manufacturer == manufacturer && product == product_code)
}

fn numeric(value: &WmiValue) -> Option<i64> {
    match value {
        WmiValue::I8(value) => Some(i64::from(*value)),
        WmiValue::I16(value) => Some(i64::from(*value)),
        WmiValue::I32(value) => Some(i64::from(*value)),
        WmiValue::I64(value) => Some(*value),
        WmiValue::U8(value) => Some(i64::from(*value)),
        WmiValue::U16(value) => Some(i64::from(*value)),
        WmiValue::U32(value) => Some(i64::from(*value)),
        WmiValue::U64(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(instance: &str, technology: WmiValue) -> WmiObject {
        [
            ("InstanceName".to_owned(), WmiValue::String(instance.to_owned())),
            ("VideoOutputTechnology".to_owned(), technology),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn selects_the_single_internal_connection() {
        let connections = vec![
            connection("DISPLAY\\DELD0E6\\1&28a6823a&2&UID256_0", WmiValue::U32(10)),
            connection(
                "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688_0",
                WmiValue::U32(0x8000_0000),
            ),
        ];
        assert_eq!(
            select_internal_instance(&connections).expect("one internal panel"),
            "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688"
        );
    }

    #[test]
    fn accepts_the_signed_encoding_of_internal() {
        let connections = vec![connection(
            "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688_0",
            WmiValue::I32(i32::MIN),
        )];
        assert_eq!(
            select_internal_instance(&connections).expect("signed internal"),
            "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688"
        );
    }

    #[test]
    fn refuses_when_no_connection_is_internal() {
        let connections = vec![connection(
            "DISPLAY\\DELD0E6\\1&28a6823a&2&UID256_0",
            WmiValue::U32(10),
        )];
        assert!(matches!(
            select_internal_instance(&connections),
            Err(LctrlError::Unsupported { .. })
        ));
    }

    #[test]
    fn refuses_an_ambiguous_internal_topology() {
        let connections = vec![
            connection(
                "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688_0",
                WmiValue::U32(0x8000_0000),
            ),
            connection(
                "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688_1",
                WmiValue::U32(0x8000_0000),
            ),
        ];
        assert!(matches!(
            select_internal_instance(&connections),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }

    #[test]
    fn refuses_an_instance_name_without_a_display_index() {
        let connections = vec![connection(
            "DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688",
            WmiValue::U32(0x8000_0000),
        )];
        assert!(matches!(
            select_internal_instance(&connections),
            Err(LctrlError::ChannelUnavailable { .. })
        ));
    }

    #[test]
    fn cross_checks_the_instance_class_against_the_edid() {
        assert_eq!(
            pnp_class_matches("DISPLAY\\LEN8AD7\\4&3ab355bd&1&UID8388688", "LEN", 0x8ad7),
            Some(true)
        );
        assert_eq!(
            pnp_class_matches("DISPLAY\\CSW146D\\4&3ab355bd&1&UID8388688", "LEN", 0x8ad7),
            Some(false)
        );
        assert_eq!(
            pnp_class_matches("DISPLAY\\Default_Monitor\\1&8713bca&0&UID0", "LEN", 0x8ad7),
            None
        );
    }
}
