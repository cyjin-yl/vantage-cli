use lctrl_core::{LctrlError, Result};

pub const IOCTL_GENERIC_SET: u32 = 0x8310_20c0;
pub const IOCTL_GENERIC_GET: u32 = 0x8310_20c4;
pub const IOCTL_GENERIC_GET_VARIANT: u32 = 0x8310_20e8;
pub const IOCTL_GBMD: u32 = 0x8310_20f8;
pub const IOCTL_BATTERY_CONFIG: u32 = 0x8310_2120;
pub const IOCTL_BATTERY_DETAIL: u32 = 0x8310_2138;
pub const IOCTL_GAPD: u32 = 0x8310_215c;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GbmdCommand(u8);

impl GbmdCommand {
    pub const STATUS: Self = Self(0xff);
    pub const CONSERVATION_ON_GEN1: Self = Self(0x03);
    pub const CONSERVATION_OFF_GEN1: Self = Self(0x05);
    pub const CONSERVATION_ON_GEN2: Self = Self(0x0d);
    pub const CONSERVATION_OFF_GEN2: Self = Self(0x0f);
    pub const RAPID_ON: Self = Self(0x07);
    pub const RAPID_OFF: Self = Self(0x08);

    #[must_use]
    pub const fn encode(self) -> [u8; 1] {
        [self.0]
    }

    pub fn decode_status(output: &[u8]) -> Result<u32> {
        decode_u32_exact(output, "GBMD")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenericGet {
    cmd: u32,
}

impl GenericGet {
    #[must_use]
    pub const fn new(cmd: u32) -> Self {
        Self { cmd }
    }

    #[must_use]
    pub const fn encode(self) -> [u8; 4] {
        self.cmd.to_le_bytes()
    }

    pub fn decode(output: &[u8]) -> Result<u32> {
        decode_u32_exact(output, "generic GET")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenericSet {
    cmd: u32,
    p1: u32,
    p2: u32,
}

impl GenericSet {
    #[must_use]
    pub const fn new(cmd: u32, p1: u32, p2: u32) -> Self {
        Self { cmd, p1, p2 }
    }

    #[must_use]
    pub fn encode(self) -> [u8; 12] {
        let mut output = [0; 12];
        output[0..4].copy_from_slice(&self.cmd.to_le_bytes());
        output[4..8].copy_from_slice(&self.p1.to_le_bytes());
        output[8..12].copy_from_slice(&self.p2.to_le_bytes());
        output
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterDetail {
    pub pid: u16,
    pub vid: u16,
    pub system_power_w: u16,
    pub current_power_w: u16,
    pub reserved: [u8; 2],
}

impl AdapterDetail {
    pub fn decode(output: &[u8]) -> Result<Self> {
        require_len(output, 10, "GAPD")?;
        Ok(Self {
            pid: u16::from_le_bytes([output[0], output[1]]),
            vid: u16::from_le_bytes([output[2], output[3]]),
            system_power_w: u16::from_le_bytes([output[4], output[5]]),
            current_power_w: u16::from_le_bytes([output[6], output[7]]),
            reserved: [output[8], output[9]],
        })
    }

    #[must_use]
    pub const fn is_underpowered(&self) -> bool {
        self.current_power_w < self.system_power_w
    }
}

/// Lossless wrapper for the verified 83-byte EnergyDrv battery response.
///
/// The clean-room documents describe two partially conflicting semantic
/// layouts for this buffer. The transport layer therefore preserves every
/// byte and exposes bounds-checked scalar access; the battery domain parser
/// owns model/version-specific interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatteryDetail83 {
    raw: [u8; 83],
}

impl BatteryDetail83 {
    pub fn decode(output: &[u8]) -> Result<Self> {
        require_len(output, 83, "battery detail")?;
        let mut raw = [0; 83];
        raw.copy_from_slice(output);
        Ok(Self { raw })
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 83] {
        &self.raw
    }

    pub fn read_u16(&self, offset: usize) -> Result<u16> {
        let bytes = self.slice(offset, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u32(&self, offset: usize) -> Result<u32> {
        let bytes = self.slice(offset, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn slice(&self, offset: usize, len: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(len)
            .filter(|end| *end <= self.raw.len())
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: format!(
                    "battery detail field at offset {offset} with length {len} exceeds 83 bytes"
                ),
            })?;
        Ok(&self.raw[offset..end])
    }
}

fn decode_u32_exact(output: &[u8], operation: &str) -> Result<u32> {
    require_len(output, 4, operation)?;
    Ok(u32::from_le_bytes([
        output[0], output[1], output[2], output[3],
    ]))
}

fn require_len(output: &[u8], expected: usize, operation: &str) -> Result<()> {
    if output.len() == expected {
        return Ok(());
    }
    Err(LctrlError::FirmwareRejected {
        detail: format!(
            "EnergyDrv {operation} returned {} bytes; expected {expected}",
            output.len()
        ),
    })
}
