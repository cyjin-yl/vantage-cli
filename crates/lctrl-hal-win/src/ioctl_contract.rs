use lctrl_core::{LctrlError, Result};

use crate::{
    AdapterDetail, BatteryDetail83, EM_SPEC_VERSION_REQUEST, GbmdCommand, GenericGet,
    IOCTL_BATTERY_CONFIG, IOCTL_BATTERY_DETAIL, IOCTL_EM_SPEC_VERSION, IOCTL_GAPD, IOCTL_GBMD,
    IOCTL_GENERIC_GET, IOCTL_NIGHT_CHARGE, IOCTL_TEMP_CHARGE, IOCTL_USB_CHARGER,
    NightChargeSelector, TempChargeSelector, UsbChargerSelector, codec::decode_u32_exact,
};

pub trait IoctlTransport: Send + Sync {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> Result<Vec<u8>>;
}

#[derive(Clone, Copy, Debug)]
pub struct EnergyDriver<'a, T: ?Sized> {
    transport: &'a T,
}

impl<'a, T> EnergyDriver<'a, T>
where
    T: IoctlTransport + ?Sized,
{
    #[must_use]
    pub const fn new(transport: &'a T) -> Self {
        Self { transport }
    }

    pub fn gbmd_status(&self) -> Result<u32> {
        let output = self
            .transport
            .call(IOCTL_GBMD, &GbmdCommand::STATUS.encode(), 4)?;
        GbmdCommand::decode_status(&output)
    }

    pub(crate) fn write_gbmd(&self, command: GbmdCommand) -> Result<()> {
        let output = self.transport.call(IOCTL_GBMD, &command.encode(), 4)?;
        if output.is_empty() {
            // The write slot answers with no data; the caller verifies the
            // transition by reading the GBMD status back.
            return Ok(());
        }
        let status = GbmdCommand::decode_status(&output)?;
        if status == 0 {
            return Ok(());
        }
        Err(LctrlError::FirmwareRejected {
            detail: format!("EnergyDrv GBMD write returned status {status}"),
        })
    }

    pub fn generic_get(&self, cmd: u32) -> Result<u32> {
        let output = self
            .transport
            .call(IOCTL_GENERIC_GET, &GenericGet::new(cmd).encode(), 4)?;
        GenericGet::decode(&output)
    }

    pub fn battery_detail(&self, index: u32) -> Result<BatteryDetail83> {
        let output = self
            .transport
            .call(IOCTL_BATTERY_DETAIL, &index.to_le_bytes(), 83)?;
        BatteryDetail83::decode(&output)
    }

    pub fn adapter_detail(&self) -> Result<AdapterDetail> {
        let output = self.transport.call(IOCTL_GAPD, &[0; 4], 10)?;
        AdapterDetail::decode(&output)
    }

    pub fn battery_config(&self) -> Result<[u8; 20]> {
        let output = self.transport.call(IOCTL_BATTERY_CONFIG, &[0; 4], 20)?;
        require_array::<20>(output, "battery config")
    }

    /// USB charging flags word (`0x831020e8`/`0x02`): bits 6/7 AC
    /// supported/enabled, bits 14/15 battery-mode supported/enabled.
    pub fn usb_charger_flags(&self) -> Result<u32> {
        let output =
            self.transport
                .call(IOCTL_USB_CHARGER, &UsbChargerSelector::QUERY.encode(), 4)?;
        UsbChargerSelector::decode_flags(&output)
    }

    /// Drive one USB charging setter. The driver completes the EC transaction
    /// and reports `Information=4` without writing an output buffer, so a
    /// zero-length output is the success shape; ioctl failure still maps to an
    /// error (a rejected request does not prove the device ignored it, but no
    /// post-write verification happens here — callers re-query flags).
    pub(crate) fn write_usb_charger(&self, selector: UsbChargerSelector) -> Result<()> {
        // The write slots answer with a four-byte word (Information = 4); a
        // zero-length output buffer is refused with ERROR_INSUFFICIENT_BUFFER.
        self.transport
            .call(IOCTL_USB_CHARGER, &selector.encode(), 4)?;
        Ok(())
    }

    /// Night-charge support probe + current state in one word.
    pub fn night_charge_probe(&self) -> Result<u32> {
        let output = self.transport.call(
            IOCTL_NIGHT_CHARGE,
            &NightChargeSelector::SUPPORT_PROBE.encode(),
            4,
        )?;
        NightChargeSelector::decode_word(&output)
    }

    /// Night-charge status word (bit4 = on).
    pub fn night_charge_status(&self) -> Result<u32> {
        let output =
            self.transport
                .call(IOCTL_NIGHT_CHARGE, &NightChargeSelector::STATUS.encode(), 4)?;
        NightChargeSelector::decode_word(&output)
    }

    pub(crate) fn write_night_charge(&self, on: bool) -> Result<()> {
        let selector = if on {
            NightChargeSelector::SET_ON
        } else {
            NightChargeSelector::SET_OFF
        };
        self.transport
            .call(IOCTL_NIGHT_CHARGE, &selector.encode(), 4)?;
        Ok(())
    }

    /// Temporary charge-mode support probe (`0x83102148`/`0x13`).
    ///
    /// `CChargingMode::DoesSupportTemporaryMode` (RVA `0xa250`) treats the
    /// feature as present/active only when the reply has bit0 set and any of
    /// bits 1..2 set; the native getter returns "no mode" without a status
    /// query otherwise.
    pub fn temporary_charge_probe(&self) -> Result<u32> {
        let output = self.transport.call(
            IOCTL_TEMP_CHARGE,
            &TempChargeSelector::SUPPORT_PROBE.encode(),
            4,
        )?;
        TempChargeSelector::decode_word(&output)
    }

    /// Temporary charge-mode status word (`0x83102148`/`0x11`). Bit0 is the
    /// validity flag; the mode is `(reply >> 4) & 0x07ff_ffff`.
    pub fn temporary_charge_status(&self) -> Result<u32> {
        let output =
            self.transport
                .call(IOCTL_TEMP_CHARGE, &TempChargeSelector::STATUS.encode(), 4)?;
        TempChargeSelector::decode_word(&output)
    }

    /// EM Spec version (`PowerBattery.dll` `GetEmSpecVersion` RVA 0x22d0,
    /// ACPI `BSIF`; docs/02 section 9). Read-only.
    ///
    /// The native request is input DWORD `0x11`, not four zero bytes, and the
    /// raw output word is not itself the version: bit0 clear means the
    /// response carries no valid version (native returns -1), bit0 set means
    /// the version is `raw >> 4`. A failed IOCTL is a transport error, not
    /// version zero.
    ///
    /// This is only one half of the vendor `DoesSupportStorage80` predicate;
    /// GBMD bit23 alone is not that predicate (see
    /// `GbmdCommand::storage80_advertised`).
    pub fn em_spec_version(&self) -> Result<u32> {
        let output = self.transport.call(
            IOCTL_EM_SPEC_VERSION,
            &EM_SPEC_VERSION_REQUEST.to_le_bytes(),
            4,
        )?;
        let raw = decode_u32_exact(&output, "EM spec version")?;
        if raw & 1 == 0 {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "EM spec version: BSIF response 0x{raw:08x} has bit0 clear (native GetEmSpecVersion returns -1)"
                ),
            });
        }
        Ok(raw >> 4)
    }
}

fn require_array<const N: usize>(output: Vec<u8>, operation: &str) -> Result<[u8; N]> {
    let actual = output.len();
    output.try_into().map_err(|_| LctrlError::FirmwareRejected {
        detail: format!("EnergyDrv {operation} returned {actual} bytes; expected {N}"),
    })
}
