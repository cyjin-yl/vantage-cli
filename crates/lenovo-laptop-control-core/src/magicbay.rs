use serde::Serialize;

pub const MAGICBAY_VENDOR_ID: u16 = 0x17ef;

/// PIDs of the two Mobile Broadband accessories Lenovo ever shipped.
///
/// Recovered from `LenovoMagiCenterDetector.exe` 2.0.2.1 (the accessories'
/// presence watcher): `LTEDevicePidLists = { "PID_7005" }`,
/// `TIKODevicePidLists = { "PID_62A5" }` (`docs/evidence/re-magicbay-20261105.md`).
pub const MAGICBAY_LTE_PIDS: [u16; 2] = [0x7005, 0x62a5];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MagicBayKind {
    TikoLte,
    Lte2,
    Hud,
    DisplayBridge,
    UsbRoleSwitch,
    Unknown,
}

impl MagicBayKind {
    /// True for the accessory kinds that expose a standard MBIM function.
    #[must_use]
    pub const fn is_lte(self) -> bool {
        matches!(self, Self::TikoLte | Self::Lte2)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KnownMagicBayDevice {
    pub vid: u16,
    pub pid: u16,
    pub kind: MagicBayKind,
    pub description: &'static str,
}

pub const KNOWN_MAGICBAY_DEVICES: [KnownMagicBayDevice; 3] = [
    KnownMagicBayDevice {
        vid: MAGICBAY_VENDOR_ID,
        pid: 0x62a5,
        kind: MagicBayKind::TikoLte,
        description: "MagicBay Tiko LTE",
    },
    KnownMagicBayDevice {
        vid: MAGICBAY_VENDOR_ID,
        pid: 0x7005,
        kind: MagicBayKind::Lte2,
        description: "MagicBay LTE 2",
    },
    KnownMagicBayDevice {
        vid: MAGICBAY_VENDOR_ID,
        pid: 0x1117,
        kind: MagicBayKind::Hud,
        description: "MagicBay HUD",
    },
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MagicBayDevice {
    pub path: String,
    pub bus: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub kind: MagicBayKind,
    pub interfaces: Vec<String>,
    pub attached: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MagicBayInventory {
    pub devices: Vec<MagicBayDevice>,
    pub acpi_devices: Vec<MagicBayDevice>,
}

#[must_use]
pub fn identify_magicbay(vid: u16, pid: u16) -> Option<&'static KnownMagicBayDevice> {
    KNOWN_MAGICBAY_DEVICES
        .iter()
        .find(|device| device.vid == vid && device.pid == pid)
}

/// Classify one Windows device instance id as a MagicBay accessory record.
///
/// This is the platform-independent half of the Windows detect path: the
/// instance id comes from SetupAPI (`SetupDiGetDeviceInstanceIdW`) or from the
/// recovered detector query's `Win32_PnPEntity.DeviceID`, and both forms carry
/// the `VID_17EF&PID_xxxx` hardware id.
#[must_use]
pub fn parse_magicbay_device_id(instance_id: &str) -> Option<MagicBayDevice> {
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

/// Merge a composite parent and its interface records into one accessory.
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
                merge_magicbay_interfaces(existing, &device);
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
                merge_magicbay_interfaces(&mut parent, &merged[candidates[0]]);
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

/// Fold one interface record's facts into an accessory record.
pub fn merge_magicbay_interfaces(existing: &mut MagicBayDevice, candidate: &MagicBayDevice) {
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

/// Media/link state of one WWAN interface (`NET_IF_MEDIA_CONNECT_STATE`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MagicBayLteMediaState {
    Connected,
    Disconnected,
    #[default]
    Unknown,
}

impl MagicBayLteMediaState {
    /// Map the documented `NET_IF_MEDIA_CONNECT_STATE` value.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Connected,
            2 => Self::Disconnected,
            _ => Self::Unknown,
        }
    }
}

/// `IF_OPER_STATUS` of one WWAN interface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MagicBayLteOperational {
    Up,
    Down,
    Testing,
    #[default]
    Unknown,
    Dormant,
    NotPresent,
    LowerLayerDown,
}

impl MagicBayLteOperational {
    /// Map the documented `IF_OPER_STATUS` value.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::Testing,
            5 => Self::Dormant,
            6 => Self::NotPresent,
            7 => Self::LowerLayerDown,
            _ => Self::Unknown,
        }
    }
}

/// One WWAN network interface as published by the OS.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MagicBayLteInterface {
    pub interface_index: u32,
    /// `IF_TYPE_WWANPP` (243) or `IF_TYPE_WWANPP2` (244).
    pub if_type: u32,
    pub description: String,
    pub media_state: MagicBayLteMediaState,
    pub operational: MagicBayLteOperational,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
}

impl MagicBayLteInterface {
    /// True when this interface owns a unicast address that is neither
    /// link-local nor loopback — i.e. a PDP context handed one out.
    #[must_use]
    pub fn has_global_address(&self) -> bool {
        self.ipv4
            .iter()
            .chain(self.ipv6.iter())
            .any(|address| is_global_address(address))
    }
}

fn is_global_address(address: &str) -> bool {
    let lower = address.to_ascii_lowercase();
    !(lower == "::" || lower == "::1"
        || lower.starts_with("fe80:")
        || lower.starts_with("127.")
        || lower.starts_with("169.254."))
}

/// Read-only MagicBay LTE state.
///
/// `modules` are the accessory records whose PID belongs to the Mobile
/// Broadband modules; `interfaces` are the WWAN interfaces the OS exposes
/// (empty when the module is attached but no MBIM function is bound).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MagicBayLteStatus {
    pub modules: Vec<MagicBayDevice>,
    pub interfaces: Vec<MagicBayLteInterface>,
    /// True when a WWAN interface carries a non-link-local unicast address.
    pub data_session: bool,
}
