//! Windows Wi-Fi link state: query-only `wlanapi`.
//!
//! Every call in this module is a query. `WlanScan`, `WlanSetInterface`,
//! `WlanConnect` and `WlanDisconnect` are deliberately not bound at all: an
//! active scan can disturb the association that carries a remote session, and
//! this verb has no write path.
//!
//! See `docs/evidence/re-network-20261105.md` for the recovered contract, the
//! live receipts, and why the band comes from the center frequency instead of
//! the channel number.

use std::{
    ffi::c_void,
    io,
    mem::size_of,
    ptr,
    time::{SystemTime, UNIX_EPOCH},
};

use lctrl_core::{LctrlError, Result};
use lctrl_hal::{
    WifiBand, WifiLinkStatus, WifiStatusControl, WifiStatusReport, assess_wifi_security,
    classify_wlan_interface_state, decode_ssid, dot11_authentication_name, dot11_cipher_name,
    dot11_phy_type_name, wlan_interface_state_name,
};
use windows_sys::{
    Win32::{
        Foundation::HANDLE,
        NetworkManagement::WiFi::{
            DOT11_SSID, WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO,
            WLAN_INTERFACE_INFO_LIST, WLAN_INTF_OPCODE, WlanCloseHandle, WlanEnumInterfaces,
            WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle, WlanQueryInterface,
            dot11_BSS_type_infrastructure, wlan_interface_state_connected,
            wlan_intf_opcode_channel_number, wlan_intf_opcode_current_connection,
        },
    },
    core::GUID,
};

/// Client version 2 is the documented surviving version for `WlanOpenHandle`.
const WLAN_CLIENT_VERSION: u32 = 2;
const DOT11_SSID_MAX_LENGTH: usize = 32;
const ERROR_SUCCESS: u32 = 0;
const WLAN_MAX_NAME_LENGTH: usize = 256;
/// `sizeof(WLAN_CONNECTION_ATTRIBUTES)` — the size the service returned live, and
/// the guard for every field offset read out of that buffer.
const CONNECTION_ATTRIBUTES_LEN: usize = 604;
const CONNECTION_PROFILE_NAME_FIELD: usize = 8;
const CONNECTION_SSID_LENGTH_FIELD: usize = 520;
const CONNECTION_SSID_FIELD: usize = 524;
const CONNECTION_BSSID_FIELD: usize = 560;
const CONNECTION_PHY_TYPE_FIELD: usize = 568;
const CONNECTION_SIGNAL_FIELD: usize = 576;
const CONNECTION_RX_RATE_FIELD: usize = 580;
const CONNECTION_TX_RATE_FIELD: usize = 584;
const CONNECTION_SECURITY_ENABLED_FIELD: usize = 588;
const CONNECTION_ONE_X_FIELD: usize = 592;
const CONNECTION_AUTHENTICATION_FIELD: usize = 596;
const CONNECTION_CIPHER_FIELD: usize = 600;
/// Fixed part of `WLAN_BSS_ENTRY` on the 10.0.26100 layout: the value the entry
/// itself reports in `ulIeOffset`.
const BSS_ENTRY_FIXED_LEN: u32 = 360;
const BSS_LIST_HEADER_LEN: usize = 8;
const BSS_ENTRY_BSSID_FIELD: usize = 40;
/// `lRssi` (dBm) on the 10.0.26100 layout.
const BSS_ENTRY_RSSI_FIELD: usize = 56;
/// `ulChCenterFrequency` (kHz) on the 10.0.26100 layout.
const BSS_ENTRY_CENTER_FREQUENCY_FIELD: usize = 92;
const BSS_ENTRY_IE_OFFSET_FIELD: usize = 352;
const BSS_ENTRY_IE_SIZE_FIELD: usize = 356;
/// A hostile or buggy service must not make this verb walk an unbounded list.
const MAX_BSS_ENTRIES: u32 = 4096;

// The explicit field offsets above are only meaningful for the layout this
// build was verified against (docs/evidence/re-network-20261105.md §2).
const _: () = assert!(size_of::<WLAN_CONNECTION_ATTRIBUTES>() == CONNECTION_ATTRIBUTES_LEN);
const _: () = assert!(size_of::<WLAN_INTERFACE_INFO>() == 532);
const _: () = assert!(size_of::<DOT11_SSID>() == 36);

/// Read-only Wi-Fi link-state service.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsWifiStatus;

impl WifiStatusControl for WindowsWifiStatus {
    fn wifi_status(&self) -> Result<WifiStatusReport> {
        let observed_at_unix_ms = now_unix_ms()?;
        let client = WlanClient::open()?;
        let interfaces = client.interfaces()?;
        let mut statuses = Vec::with_capacity(interfaces.len());
        for interface in &interfaces {
            statuses.push(client.link_status(interface)?);
        }
        Ok(WifiStatusReport {
            observed_at_unix_ms,
            interfaces: statuses,
        })
    }
}

/// An open WLAN client handle. Closed on drop, so no early return leaks it.
struct WlanClient {
    handle: HANDLE,
}

/// What a `WlanQueryInterface` call produced.
enum QueryOutcome {
    Data(Vec<u8>),
    /// The service declined the opcode; the status is retained so a caller can
    /// report it instead of pretending the field does not exist.
    Declined(u32),
}

impl WlanClient {
    fn open() -> Result<Self> {
        let mut negotiated = 0u32;
        let mut handle: HANDLE = ptr::null_mut();
        let rc = unsafe {
            WlanOpenHandle(
                WLAN_CLIENT_VERSION,
                ptr::null(),
                &mut negotiated,
                &mut handle,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(wlan_error(rc, "WlanOpenHandle"));
        }
        if handle.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "WlanOpenHandle reported success without a client handle".into(),
            });
        }
        Ok(Self { handle })
    }

    fn interfaces(&self) -> Result<Vec<WlanInterface>> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        let rc = unsafe { WlanEnumInterfaces(self.handle, ptr::null(), &mut list) };
        if rc != ERROR_SUCCESS {
            return Err(wlan_error(rc, "WlanEnumInterfaces"));
        }
        if list.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "WlanEnumInterfaces reported success without a list".into(),
            });
        }
        let _owned = WlanMemory(list as *const c_void);
        let count = unsafe { (*list).dwNumberOfItems };
        let entries = unsafe { ptr::addr_of!((*list).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>() };
        let mut interfaces = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            // A copy, so no reference is held into service-owned memory and no
            // alignment assumption is made about it beyond its own malloc.
            let entry = unsafe { ptr::read_unaligned(entries.add(index)) };
            interfaces.push(WlanInterface {
                guid: entry.InterfaceGuid,
                description: wide_field(&entry.strInterfaceDescription),
                state: entry.isState,
            });
        }
        Ok(interfaces)
    }

    fn link_status(&self, interface: &WlanInterface) -> Result<WifiLinkStatus> {
        let mut status = WifiLinkStatus {
            interface_guid: format_guid(&interface.guid),
            interface_description: interface.description.clone(),
            interface_state_raw: interface.state,
            interface_state: wlan_interface_state_name(interface.state).map(str::to_owned),
            connection_state: classify_wlan_interface_state(interface.state),
            ssid: None,
            bssid: None,
            profile_name: None,
            phy_type: None,
            phy_type_raw: None,
            channel: None,
            band: None,
            center_frequency_khz: None,
            signal_percent: None,
            rssi_dbm: None,
            rx_rate_kbps: None,
            tx_rate_kbps: None,
            authentication: None,
            authentication_raw: None,
            cipher: None,
            cipher_raw: None,
            security_enabled: None,
            one_x_enabled: None,
            assessment: None,
        };
        if interface.state != wlan_interface_state_connected {
            return Ok(status);
        }
        // The channel number is only defined for a live association, so a
        // declined opcode leaves the field absent rather than failing the verb.
        if let QueryOutcome::Data(buffer) =
            self.query(&interface.guid, wlan_intf_opcode_channel_number)?
        {
            status.channel = read_u32(&buffer, 0);
        }
        let association = self.current_connection(&interface.guid)?;
        status.ssid = association.ssid.clone();
        status.bssid = association.bssid.clone();
        status.profile_name = association.profile_name.clone();
        status.phy_type_raw = association.phy_type;
        status.phy_type = association
            .phy_type
            .and_then(dot11_phy_type_name)
            .map(str::to_owned);
        status.signal_percent = association
            .signal_percent
            .and_then(|percent| u8::try_from(percent).ok())
            .filter(|percent| *percent <= 100);
        status.rx_rate_kbps = association.rx_rate_kbps;
        status.tx_rate_kbps = association.tx_rate_kbps;
        status.authentication_raw = association.authentication;
        status.authentication = association
            .authentication
            .and_then(dot11_authentication_name)
            .map(str::to_owned);
        status.cipher_raw = association.cipher;
        status.cipher = association
            .cipher
            .and_then(dot11_cipher_name)
            .map(str::to_owned);
        status.security_enabled = association.security_enabled;
        status.one_x_enabled = association.one_x_enabled;
        status.assessment = Some(assess_wifi_security(
            status.authentication.as_deref(),
            status.cipher.as_deref(),
        ));
        if let Some(bss) = self.associated_bss(interface, &association)? {
            status.rssi_dbm = Some(bss.rssi_dbm);
            status.center_frequency_khz = Some(bss.center_frequency_khz);
            status.band = Some(WifiBand::from_center_frequency_khz(
                bss.center_frequency_khz,
            ));
        }
        Ok(status)
    }

    fn current_connection(&self, guid: &GUID) -> Result<ConnectionAttributes> {
        let buffer = match self.query(guid, wlan_intf_opcode_current_connection)? {
            QueryOutcome::Data(buffer) => buffer,
            QueryOutcome::Declined(rc) => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "wlan_intf_opcode_current_connection was declined with WLAN status {rc} \
                         (0x{rc:08x}) for an interface the service reports as connected"
                    ),
                });
            }
        };
        if buffer.len() != CONNECTION_ATTRIBUTES_LEN {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "wlan_intf_opcode_current_connection returned {} bytes; this build decodes the \
                     {CONNECTION_ATTRIBUTES_LEN}-byte WLAN_CONNECTION_ATTRIBUTES layout",
                    buffer.len()
                ),
            });
        }
        let ssid_length = read_u32(&buffer, CONNECTION_SSID_LENGTH_FIELD).ok_or_else(|| {
            LctrlError::Unsupported {
                feature: "WLAN_CONNECTION_ATTRIBUTES has no SSID length field".into(),
            }
        })?;
        let ssid_length = usize::try_from(ssid_length).map_err(|_| LctrlError::Unsupported {
            feature: "WLAN_ASSOCIATION_ATTRIBUTES reports an unusable SSID length".into(),
        })?;
        if ssid_length > DOT11_SSID_MAX_LENGTH {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "WLAN_ASSOCIATION_ATTRIBUTES reports an SSID length of {ssid_length}, above \
                     DOT11_SSID_MAX_LENGTH"
                ),
            });
        }
        let raw_ssid = buffer
            .get(CONNECTION_SSID_FIELD..CONNECTION_SSID_FIELD + ssid_length)
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "WLAN_CONNECTION_ATTRIBUTES is truncated before the SSID".into(),
            })?
            .to_vec();
        let ssid = decode_ssid(&raw_ssid);
        Ok(ConnectionAttributes {
            ssid,
            raw_ssid: if raw_ssid.is_empty() {
                None
            } else {
                Some(raw_ssid)
            },
            bssid: read_address(&buffer, CONNECTION_BSSID_FIELD)
                .map(|bssid| format_address(&bssid)),
            profile_name: read_wide(&buffer, CONNECTION_PROFILE_NAME_FIELD, WLAN_MAX_NAME_LENGTH),
            phy_type: read_i32(&buffer, CONNECTION_PHY_TYPE_FIELD),
            signal_percent: read_u32(&buffer, CONNECTION_SIGNAL_FIELD),
            rx_rate_kbps: read_u32(&buffer, CONNECTION_RX_RATE_FIELD),
            tx_rate_kbps: read_u32(&buffer, CONNECTION_TX_RATE_FIELD),
            authentication: read_i32(&buffer, CONNECTION_AUTHENTICATION_FIELD),
            cipher: read_i32(&buffer, CONNECTION_CIPHER_FIELD),
            security_enabled: read_i32(&buffer, CONNECTION_SECURITY_ENABLED_FIELD)
                .map(|value| value != 0),
            one_x_enabled: read_i32(&buffer, CONNECTION_ONE_X_FIELD).map(|value| value != 0),
        })
    }

    /// Reads the BSS entry of the associated AP.
    ///
    /// `WlanGetNetworkBssList` returns the service's cached list for the filter
    /// it is given; it does not scan. An empty list is "no cached entry", which
    /// leaves band and RSSI absent instead of inventing them.
    fn associated_bss(
        &self,
        interface: &WlanInterface,
        association: &ConnectionAttributes,
    ) -> Result<Option<BssFacts>> {
        let mut filter = DOT11_SSID {
            uSSIDLength: 0,
            ucSSID: [0; DOT11_SSID_MAX_LENGTH],
        };
        let filter_pointer = match association.raw_ssid.as_deref() {
            Some(bytes) => {
                let length = bytes.len().min(DOT11_SSID_MAX_LENGTH);
                filter.uSSIDLength = u32::try_from(length).map_err(|_| {
                    LctrlError::Unsupported {
                        feature: "association SSID does not fit DOT11_SSID".into(),
                    }
                })?;
                filter.ucSSID[..length].copy_from_slice(&bytes[..length]);
                &filter as *const DOT11_SSID
            }
            None => ptr::null(),
        };
        let mut list: *mut WLAN_BSS_LIST = ptr::null_mut();
        let rc = unsafe {
            WlanGetNetworkBssList(
                self.handle,
                &interface.guid,
                filter_pointer,
                dot11_BSS_type_infrastructure,
                0,
                ptr::null(),
                &mut list,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(wlan_error(rc, "WlanGetNetworkBssList"));
        }
        if list.is_null() {
            return Ok(None);
        }
        let _owned = WlanMemory(list as *const c_void);
        let base = list.cast::<u8>();
        let total = unsafe { ptr::read_unaligned(base.cast::<u32>()) };
        let items = unsafe { ptr::read_unaligned(base.cast::<u32>().add(1)) };
        decode_bss_list(base, total, items, association.bssid_bytes())
    }

    fn query(&self, guid: &GUID, opcode: WLAN_INTF_OPCODE) -> Result<QueryOutcome> {
        let mut size = 0u32;
        let mut data: *mut c_void = ptr::null_mut();
        let mut value_type = 0;
        let rc = unsafe {
            WlanQueryInterface(
                self.handle,
                guid,
                opcode,
                ptr::null(),
                &mut size,
                &mut data,
                &mut value_type,
            )
        };
        if rc != ERROR_SUCCESS {
            return Ok(QueryOutcome::Declined(rc));
        }
        if data.is_null() || size == 0 {
            return Ok(QueryOutcome::Declined(ERROR_SUCCESS));
        }
        // Copy out before releasing: the copy outlives the service buffer.
        let owned =
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) }.to_vec();
        unsafe { WlanFreeMemory(data) };
        Ok(QueryOutcome::Data(owned))
    }
}

impl Drop for WlanClient {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.handle, ptr::null());
        }
    }
}

/// Owns a buffer the WLAN service allocated; released exactly once.
struct WlanMemory(*const c_void);

impl Drop for WlanMemory {
    fn drop(&mut self) {
        unsafe { WlanFreeMemory(self.0) };
    }
}

struct WlanInterface {
    guid: GUID,
    description: Option<String>,
    state: i32,
}

struct ConnectionAttributes {
    ssid: Option<String>,
    raw_ssid: Option<Vec<u8>>,
    bssid: Option<String>,
    profile_name: Option<String>,
    phy_type: Option<i32>,
    signal_percent: Option<u32>,
    rx_rate_kbps: Option<u32>,
    tx_rate_kbps: Option<u32>,
    authentication: Option<i32>,
    cipher: Option<i32>,
    security_enabled: Option<bool>,
    one_x_enabled: Option<bool>,
}

impl ConnectionAttributes {
    fn bssid_bytes(&self) -> Option<[u8; 6]> {
        parse_address(self.bssid.as_deref()?)
    }
}

struct BssFacts {
    rssi_dbm: i32,
    center_frequency_khz: u32,
}

/// Walks a `WLAN_BSS_LIST` and returns the entry of the associated AP.
///
/// `WLAN_BSS_ENTRY` is variable-size: each entry declares its own IE offset and
/// size, so the stride is `align8(ulIeOffset + ulIeSize)` and never
/// `(dwTotalSize - 8) / dwNumberOfItems`. The walk is accepted only when every
/// entry declares the fixed part this build was verified against **and** the
/// strides land exactly on `dwTotalSize`.
fn decode_bss_list(
    base: *const u8,
    total: u32,
    items: u32,
    wanted_bssid: Option<[u8; 6]>,
) -> Result<Option<BssFacts>> {
    let total = total as usize;
    if items > MAX_BSS_ENTRIES {
        return Err(LctrlError::Unsupported {
            feature: format!(
                "WLAN_BSS_LIST reports {items} entries, above the {MAX_BSS_ENTRIES} bound"
            ),
        });
    }
    if total < BSS_LIST_HEADER_LEN {
        return Err(LctrlError::Unsupported {
            feature: format!("WLAN_BSS_LIST reports a total size of {total} bytes"),
        });
    }
    let mut offset = BSS_LIST_HEADER_LEN;
    let mut matched: Option<BssFacts> = None;
    for _ in 0..items {
        let remaining = total - offset;
        if remaining < BSS_ENTRY_FIXED_LEN as usize {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "WLAN_BSS_ENTRY at offset {offset} has only {remaining} bytes left"
                ),
            });
        }
        // SAFETY: `offset + BSS_ENTRY_FIXED_LEN <= total`, and the service
        // buffer holds at least `dwTotalSize` bytes by contract.
        let entry = unsafe { base.add(offset) };
        let ie_offset = read_u32_at(entry, BSS_ENTRY_IE_OFFSET_FIELD);
        let ie_size = read_u32_at(entry, BSS_ENTRY_IE_SIZE_FIELD);
        if ie_offset != BSS_ENTRY_FIXED_LEN {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "WLAN_BSS_ENTRY declares an IE offset of {ie_offset}; this build decodes the \
                     {BSS_ENTRY_FIXED_LEN}-byte layout"
                ),
            });
        }
        let entry_length = (ie_offset as usize)
            .checked_add(ie_size as usize)
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "WLAN_BSS_ENTRY length overflows".into(),
            })?;
        let stride = entry_length.checked_add(7).map(|value| value & !7).ok_or_else(|| {
            LctrlError::Unsupported {
                feature: "WLAN_BSS_ENTRY stride overflows".into(),
            }
        })?;
        if stride < BSS_ENTRY_FIXED_LEN as usize || stride > total - offset {
            return Err(LctrlError::Unsupported {
                feature: format!("WLAN_BSS_ENTRY at offset {offset} declares length {stride}"),
            });
        }
        if let Some(wanted) = wanted_bssid {
            if matched.is_none() && read_address_at(entry, BSS_ENTRY_BSSID_FIELD) == wanted {
                matched = Some(BssFacts {
                    rssi_dbm: read_i32_at(entry, BSS_ENTRY_RSSI_FIELD),
                    center_frequency_khz: read_u32_at(entry, BSS_ENTRY_CENTER_FREQUENCY_FIELD),
                });
            }
        }
        offset += stride;
    }
    if offset != total {
        return Err(LctrlError::Unsupported {
            feature: format!(
                "WLAN_BSS_LIST strides end at {offset} but the list declares {total} bytes"
            ),
        });
    }
    Ok(matched)
}

fn read_u32_at(base: *const u8, offset: usize) -> u32 {
    // SAFETY: every caller checks `offset + 4` against the list's total size.
    unsafe { ptr::read_unaligned(base.add(offset).cast::<u32>()) }
}

fn read_i32_at(base: *const u8, offset: usize) -> i32 {
    // SAFETY: every caller checks `offset + 4` against the list's total size.
    unsafe { ptr::read_unaligned(base.add(offset).cast::<i32>()) }
}

fn read_address_at(base: *const u8, offset: usize) -> [u8; 6] {
    let mut address = [0u8; 6];
    // SAFETY: callers keep `offset + 6` inside the validated entry.
    address.copy_from_slice(unsafe { std::slice::from_raw_parts(base.add(offset), 6) });
    address
}

fn read_u32(buffer: &[u8], offset: usize) -> Option<u32> {
    let bytes = buffer.get(offset..offset + 4)?;
    Some(u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(buffer: &[u8], offset: usize) -> Option<i32> {
    let bytes = buffer.get(offset..offset + 4)?;
    Some(i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_address(buffer: &[u8], offset: usize) -> Option<[u8; 6]> {
    let bytes = buffer.get(offset..offset + 6)?;
    let mut address = [0u8; 6];
    address.copy_from_slice(bytes);
    Some(address)
}

fn read_wide(buffer: &[u8], offset: usize, units: usize) -> Option<String> {
    let bytes = buffer.get(offset..offset + units * 2)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect();
    wide_field(&units)
}

fn wide_field(value: &[u16]) -> Option<String> {
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

fn format_address(bytes: &[u8; 6]) -> String {
    let mut text = String::with_capacity(17);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            text.push(':');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn parse_address(text: &str) -> Option<[u8; 6]> {
    let mut address = [0u8; 6];
    let mut parts = text.split(':');
    for slot in address.iter_mut() {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(address)
}

fn wlan_error(rc: u32, operation: &str) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("{operation} failed with WLAN status {rc} (0x{rc:08x})"),
    }
}

fn now_unix_ms() -> Result<u64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock is before the Unix epoch; cannot timestamp a Wi-Fi observation",
        ))
    })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock timestamp exceeds the Wi-Fi observation representation",
        ))
    })
}
