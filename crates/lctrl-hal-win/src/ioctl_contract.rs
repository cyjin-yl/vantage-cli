use lctrl_core::{LctrlError, Result};

use crate::{
    AdapterDetail, BatteryDetail83, GbmdCommand, GenericGet, GenericSet, IOCTL_BATTERY_CONFIG,
    IOCTL_BATTERY_DETAIL, IOCTL_GAPD, IOCTL_GBMD, IOCTL_GENERIC_GET, IOCTL_GENERIC_SET,
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

    pub fn write_gbmd(&self, command: GbmdCommand) -> Result<()> {
        let output = self.transport.call(IOCTL_GBMD, &command.encode(), 4)?;
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

    pub fn generic_set(&self, cmd: u32, p1: u32, p2: u32) -> Result<()> {
        let output =
            self.transport
                .call(IOCTL_GENERIC_SET, &GenericSet::new(cmd, p1, p2).encode(), 0)?;
        if output.is_empty() {
            return Ok(());
        }
        Err(LctrlError::FirmwareRejected {
            detail: format!(
                "EnergyDrv generic SET returned {} bytes; expected 0",
                output.len()
            ),
        })
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
}

fn require_array<const N: usize>(output: Vec<u8>, operation: &str) -> Result<[u8; N]> {
    let actual = output.len();
    output.try_into().map_err(|_| LctrlError::FirmwareRejected {
        detail: format!("EnergyDrv {operation} returned {actual} bytes; expected {N}"),
    })
}
