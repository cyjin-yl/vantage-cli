//! Read-only MagicBay LTE status over documented public CIM classes.
//!
//! Three property-limited `SELECT`s, no method invocation, no vendor payload:
//!
//! * `root\CIMV2: Win32_PnPEntity` — accessory presence. The query is the one
//!   recovered from Lenovo's own presence watcher
//!   (`LenovoMagiCenterDetector.exe` 2.0.2.1), including its `[_]` LIKE escapes
//!   (`docs/evidence/re-magicbay-20261105.md` §2.2).
//! * `root\StandardCimv2: MSFT_NetAdapter` — WWAN interfaces
//!   (`IF_TYPE_WWANPP` / `IF_TYPE_WWANPP2`) with media and operational state.
//! * `root\StandardCimv2: MSFT_NetIPAddress` — the unicast addresses those
//!   interfaces own, i.e. the PDP-context addresses a data session would have.
//!
//! Modem control (signal, registration, APN, connect/disconnect) is not part of
//! this contract: no bindable modem API exists on the target
//! (`docs/evidence/re-magicbay-20261105.md` §3.2).

use lctrl_core::{
    LctrlError, MagicBayDevice, MagicBayInventory, MagicBayLteInterface, MagicBayLteMediaState,
    MagicBayLteOperational, MagicBayLteStatus, Result, coalesce_magicbay_devices,
    parse_magicbay_device_id,
};
use lctrl_hal::MobileBroadbandControl;

use crate::{WmiObject, WmiTransport, WmiValue};

pub const ROOT_CIMV2: &str = "ROOT\\CIMV2";
pub const ROOT_STANDARD_CIMV2: &str = "ROOT\\StandardCimv2";

/// Accessory presence query, byte-for-byte the Lenovo detector's WQL.
pub const MODULE_QUERY: &str = "SELECT DeviceID FROM Win32_PnPEntity WHERE (DeviceID LIKE '%VID[_]17EF%') AND DeviceID LIKE '%PID[_]%'";

/// WWAN interface query. `OperStatus` is not a property of this class; the
/// NDIS operational state is `InterfaceOperationalStatus`.
pub const ADAPTER_QUERY: &str = "SELECT InterfaceIndex, InterfaceType, InterfaceDescription, MediaConnectState, InterfaceOperationalStatus FROM MSFT_NetAdapter";

/// Unicast address query (`AddressFamily` 2 = IPv4, 23 = IPv6).
pub const ADDRESS_QUERY: &str =
    "SELECT InterfaceIndex, IPAddress, AddressFamily FROM MSFT_NetIPAddress";

/// `IF_TYPE_WWANPP` — Mobile Broadband interface.
pub const IF_TYPE_WWANPP: u32 = 243;
/// `IF_TYPE_WWANPP2` — second Mobile Broadband interface instance.
pub const IF_TYPE_WWANPP2: u32 = 244;

const NO_MODULE_CHANNEL: &str = "magicbay.lte: no MagicBay LTE accessory is enumerated (USB VID_17EF with PID_7005 or PID_62A5); the WWAN interface state is unavailable (docs/evidence/re-magicbay-20261105.md)";

/// Read-only probe used by the capability matrix.
///
/// It runs the accessory query and the WWAN interface query so that a recorded
/// capability proves both CIM namespaces answer, not merely that WMI exists.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MobileBroadbandProbe {
    pub inventory: MagicBayInventory,
    pub wwan_interfaces: usize,
}

pub fn probe_mobile_broadband<W: WmiTransport>(wmi: &W) -> Result<MobileBroadbandProbe> {
    let inventory = MagicBayInventory {
        devices: magicbay_modules(&wmi.query(ROOT_CIMV2, MODULE_QUERY)?),
        ..MagicBayInventory::default()
    };
    let interfaces = wwan_interfaces(&wmi.query(ROOT_STANDARD_CIMV2, ADAPTER_QUERY)?);
    Ok(MobileBroadbandProbe {
        inventory,
        wwan_interfaces: interfaces.len(),
    })
}

/// Read-only WWAN status service.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsMobileBroadband<W> {
    wmi: W,
}

impl<W> WindowsMobileBroadband<W> {
    #[must_use]
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

impl<W> MobileBroadbandControl for WindowsMobileBroadband<W>
where
    W: WmiTransport,
{
    fn lte_status(&self) -> Result<MagicBayLteStatus> {
        let modules = magicbay_modules(&self.wmi.query(ROOT_CIMV2, MODULE_QUERY)?);
        if modules.is_empty() {
            return Err(LctrlError::ChannelUnavailable {
                channel: NO_MODULE_CHANNEL.into(),
            });
        }
        let mut interfaces = wwan_interfaces(&self.wmi.query(ROOT_STANDARD_CIMV2, ADAPTER_QUERY)?);
        if !interfaces.is_empty() {
            attach_addresses(
                &mut interfaces,
                &self.wmi.query(ROOT_STANDARD_CIMV2, ADDRESS_QUERY)?,
            );
        }
        let data_session = interfaces
            .iter()
            .any(MagicBayLteInterface::has_global_address);
        Ok(MagicBayLteStatus {
            modules,
            interfaces,
            data_session,
        })
    }
}

/// Coalesced Mobile Broadband accessory records from `Win32_PnPEntity` rows.
fn magicbay_modules(objects: &[WmiObject]) -> Vec<MagicBayDevice> {
    let mut devices: Vec<MagicBayDevice> = objects
        .iter()
        .filter_map(|object| match object.get("DeviceID") {
            Some(WmiValue::String(device_id)) => parse_magicbay_device_id(device_id),
            _ => None,
        })
        .filter(|device| device.bus == "usb" && device.kind.is_lte())
        .collect();
    devices.sort_by(|left, right| left.path.cmp(&right.path));
    devices.dedup_by(|left, right| left.path == right.path);
    coalesce_magicbay_devices(devices)
}

/// WWAN interfaces from `MSFT_NetAdapter` rows, ordered by interface index.
fn wwan_interfaces(objects: &[WmiObject]) -> Vec<MagicBayLteInterface> {
    let mut interfaces: Vec<MagicBayLteInterface> = objects
        .iter()
        .filter_map(|object| {
            let if_type = u32_field(object, "InterfaceType")?;
            if !is_wwan_if_type(if_type) {
                return None;
            }
            Some(MagicBayLteInterface {
                interface_index: u32_field(object, "InterfaceIndex")?,
                if_type,
                description: string_field(object, "InterfaceDescription")
                    .unwrap_or_else(|| "unknown Mobile Broadband adapter".into()),
                media_state: MagicBayLteMediaState::from_raw(
                    u32_field(object, "MediaConnectState").unwrap_or_default(),
                ),
                operational: MagicBayLteOperational::from_raw(
                    u32_field(object, "InterfaceOperationalStatus").unwrap_or_default(),
                ),
                ipv4: Vec::new(),
                ipv6: Vec::new(),
            })
        })
        .collect();
    interfaces.sort_by_key(|interface| interface.interface_index);
    interfaces
}

fn is_wwan_if_type(if_type: u32) -> bool {
    if_type == IF_TYPE_WWANPP || if_type == IF_TYPE_WWANPP2
}

/// Attach `MSFT_NetIPAddress` rows to the interfaces that own them.
fn attach_addresses(interfaces: &mut [MagicBayLteInterface], objects: &[WmiObject]) {
    for object in objects {
        let (Some(index), Some(WmiValue::String(address))) = (
            u32_field(object, "InterfaceIndex"),
            object.get("IPAddress"),
        ) else {
            continue;
        };
        if address.is_empty() {
            continue;
        }
        let Some(interface) = interfaces
            .iter_mut()
            .find(|interface| interface.interface_index == index)
        else {
            continue;
        };
        let bucket = match u32_field(object, "AddressFamily") {
            Some(2) => &mut interface.ipv4,
            Some(23) => &mut interface.ipv6,
            _ => continue,
        };
        if !bucket.contains(address) {
            bucket.push(address.clone());
        }
    }
}

fn string_field(object: &WmiObject, name: &str) -> Option<String> {
    match object.get(name) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    match object.get(name)? {
        WmiValue::U8(value) => Some(u32::from(*value)),
        WmiValue::U16(value) => Some(u32::from(*value)),
        WmiValue::U32(value) => Some(*value),
        WmiValue::I8(value) => u32::try_from(*value).ok(),
        WmiValue::I16(value) => u32::try_from(*value).ok(),
        WmiValue::I32(value) => u32::try_from(*value).ok(),
        WmiValue::U64(value) => u32::try_from(*value).ok(),
        WmiValue::I64(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

#[cfg(test)]
#[path = "mobile_broadband_tests.rs"]
mod tests;
