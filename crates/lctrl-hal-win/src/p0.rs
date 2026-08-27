use lctrl_core::{
    AdapterDetailValues, AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode,
    ChargeModeActual, ChargePrimitive, LctrlError, Result, decode_charge_mode, plan_charge_mode,
};
use lctrl_hal::BatteryControl;

use crate::{EnergyDriver, GbmdCommand, IoctlTransport};

/// A readback source for the firmware charge-mode bitmask.
///
/// The target firmware's WMI battery class is read-only identity data, so a
/// production writer is only constructed after a separately verified source
/// is available. The controller refuses writes before this first read.
pub trait ChargeModeReader: Send + Sync {
    fn read_charge_mode_raw(&self) -> Result<u32>;
}

/// Explicitly denies charge-mode writes until a firmware readback transport is
/// independently verified. This prevents a successful IOCTL from being
/// mistaken for semantic success on the target machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnverifiedChargeModeReader;

impl ChargeModeReader for UnverifiedChargeModeReader {
    fn read_charge_mode_raw(&self) -> Result<u32> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}

#[derive(Debug)]
pub struct WindowsBatteryP0<I, R> {
    ioctl: I,
    reader: R,
}

impl<I, R> WindowsBatteryP0<I, R> {
    #[must_use]
    pub const fn new(ioctl: I, reader: R) -> Self {
        Self { ioctl, reader }
    }

    #[must_use]
    pub const fn ioctl(&self) -> &I {
        &self.ioctl
    }

    #[must_use]
    pub const fn reader(&self) -> &R {
        &self.reader
    }
}

impl<I, R> WindowsBatteryP0<I, R>
where
    I: IoctlTransport,
    R: ChargeModeReader,
{
    fn mode_for_write(&self) -> Result<ChargeMode> {
        match decode_charge_mode(self.reader.read_charge_mode_raw()?)? {
            ChargeModeActual::Normal => Ok(ChargeMode::Normal),
            ChargeModeActual::Conservation => Ok(ChargeMode::Conservation),
            ChargeModeActual::Rapid => Ok(ChargeMode::Rapid),
            ChargeModeActual::Conflict => Err(LctrlError::FirmwareRejected {
                detail: "battery charge mode has both conservation and rapid bits set".into(),
            }),
            ChargeModeActual::Unknown(raw) => Err(LctrlError::FirmwareRejected {
                detail: format!("battery charge mode has unknown bitmask {raw}"),
            }),
        }
    }

    fn write_primitive(&self, primitive: ChargePrimitive) -> Result<()> {
        let command = match primitive {
            // Target 21VG safety rule: never send legacy gen1 0x03 because
            // later evidence identifies it as transport mode. Only verified
            // target-safe gen2 conservation commands are used here.
            ChargePrimitive::Conservation(false) => GbmdCommand::CONSERVATION_OFF_GEN2,
            ChargePrimitive::Conservation(true) => GbmdCommand::CONSERVATION_ON_GEN2,
            ChargePrimitive::Rapid(false) => GbmdCommand::RAPID_OFF,
            ChargePrimitive::Rapid(true) => GbmdCommand::RAPID_ON,
        };
        EnergyDriver::new(&self.ioctl).write_gbmd(command)
    }
}

impl<I, R> BatteryControl for WindowsBatteryP0<I, R>
where
    I: IoctlTransport,
    R: ChargeModeReader,
{
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        let raw = EnergyDriver::new(&self.ioctl).battery_detail(index)?;
        BatteryTelemetry::parse(raw.as_bytes())
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        let driver = EnergyDriver::new(&self.ioctl);
        let status = driver.gbmd_status()?;
        let detail = if (status >> 24) & 1 != 0 {
            let detail = driver.adapter_detail()?;
            Some(AdapterDetailValues {
                pid: detail.pid,
                vid: detail.vid,
                system_power_w: detail.system_power_w,
                current_power_w: detail.current_power_w,
            })
        } else {
            None
        };
        Ok(AdapterInfo::from_gbmd(status, detail))
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        decode_charge_mode(self.reader.read_charge_mode_raw()?)
    }

    fn set_charge_mode(
        &self,
        mode: ChargeMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<ChargeMode>> {
        let previous = self.mode_for_write()?;
        if mode == ChargeMode::Rapid && !self.battery_telemetry(0)?.rapid_charge_allowed() {
            return Err(LctrlError::Unsupported {
                feature: "battery.rapid_charge".into(),
            });
        }
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, mode));
        }

        for primitive in plan_charge_mode(mode) {
            self.write_primitive(primitive)?;
        }

        let actual = self.mode_for_write()?;
        if actual != mode {
            return Err(LctrlError::VerifyMismatch {
                requested: mode.to_string(),
                actual: actual.to_string(),
            });
        }
        Ok(ChangeReport::committed(previous, mode, actual))
    }
}
