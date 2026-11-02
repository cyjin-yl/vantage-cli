use lctrl_core::{LctrlError, Result};

pub const IOCTL_GENERIC_GET: u32 = 0x8310_20c4;
/// USB charging control (`PowerBattery.dll` `CUSBCharger` / `CUSBBatteryCharger`).
///
/// Previously allowlisted as a "generic GET variant" from docs/01 behavior
/// analysis; RE of the exact target binaries (PowerBattery.dll 1.0.13.114,
/// AcpiVpc.sys 15.11.30.14) proved `in[0] == 0x02` is the flags query and
/// `0x0a/0x0b/0x12/0x13` are the setters (docs/interfaces.md §11).
pub const IOCTL_USB_CHARGER: u32 = 0x8310_20e8;
pub const IOCTL_GBMD: u32 = 0x8310_20f8;
pub const IOCTL_BATTERY_CONFIG: u32 = 0x8310_2120;
pub const IOCTL_BATTERY_DETAIL: u32 = 0x8310_2138;
pub const IOCTL_GAPD: u32 = 0x8310_215c;
/// Night-charge mode (`PowerBattery.dll` `CChargingMode`, RE-proven;
/// 4-byte DWORD selectors).
pub const IOCTL_NIGHT_CHARGE: u32 = 0x8310_2150;
/// Temporary charge-mode probe (`PowerBattery.dll`, support `0x13`,
/// status `0x11`; docs/interfaces.md §11).
pub const IOCTL_TEMP_CHARGE: u32 = 0x8310_2148;
/// EM Spec version (`PowerBattery.dll` `GetEmSpecVersion`, ACPI `BSIF`;
/// docs/02 §9). Read-only; gates the `ChargeMode-Storage-80` mode string in
/// the vendor decode.
pub const IOCTL_EM_SPEC_VERSION: u32 = 0x8310_214c;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GbmdCommand(u8);

impl GbmdCommand {
    pub const STATUS: Self = Self(0xff);
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

    /// Storage80 support bit in the GBMD status word
    /// (`PowerBattery.dll!DoesSupportStorage80`, docs/interfaces.md §11.1).
    /// Live on 21VG: `0x00860004` has bit23 set.
    #[must_use]
    pub const fn storage80_supported(status: u32) -> bool {
        status & (1 << 23) != 0
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
pub struct UsbChargerSelector(u8);

impl UsbChargerSelector {
    pub const QUERY: Self = Self(0x02);
    pub const AC_ON: Self = Self(0x0a);
    pub const AC_OFF: Self = Self(0x0b);
    pub const BATTERY_ON: Self = Self(0x13);
    pub const BATTERY_OFF: Self = Self(0x12);

    #[must_use]
    pub const fn encode(self) -> [u8; 1] {
        [self.0]
    }

    pub fn decode_flags(output: &[u8]) -> Result<u32> {
        decode_u32_exact(output, "USB charger")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NightChargeSelector(u32);

impl NightChargeSelector {
    pub const SUPPORT_PROBE: Self = Self(0x13);
    pub const STATUS: Self = Self(0x11);
    pub const SET_ON: Self = Self(0x8000_0012);
    pub const SET_OFF: Self = Self(0x12);

    #[must_use]
    pub const fn encode(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    pub fn decode_word(output: &[u8]) -> Result<u32> {
        decode_u32_exact(output, "night charge")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TempChargeSelector(u32);

impl TempChargeSelector {
    pub const SUPPORT_PROBE: Self = Self(0x13);
    pub const STATUS: Self = Self(0x11);

    #[must_use]
    pub const fn encode(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    pub fn decode_word(output: &[u8]) -> Result<u32> {
        decode_u32_exact(output, "temporary charge mode")
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

pub(crate) fn decode_u32_exact(output: &[u8], operation: &str) -> Result<u32> {
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

#[cfg(test)]
mod tests {
    use super::{GbmdCommand, IOCTL_EM_SPEC_VERSION};

    #[test]
    fn storage80_bit23_matches_live_gbmd_status() {
        // Live 21VG GBMD status word (docs/interfaces.md §11.1).
        assert!(GbmdCommand::storage80_supported(0x0086_0004));
        assert!(!GbmdCommand::storage80_supported(0x0006_0004));
        assert!(!GbmdCommand::storage80_supported(0));
    }

    #[test]
    fn em_spec_version_ioctl_is_the_bsif_code() {
        assert_eq!(IOCTL_EM_SPEC_VERSION, 0x8310_214c);
    }
}
