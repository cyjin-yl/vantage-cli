use serde::Serialize;

pub const MAGICBAY_VENDOR_ID: u16 = 0x17ef;

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
        pid: 0x62b5,
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
