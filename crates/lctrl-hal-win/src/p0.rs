use std::time::Duration;

use lctrl_core::{
    AdapterDetailValues, AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode,
    ChargeModeActual, ChargePrimitive, LctrlError, Result, decode_gbmd_charge_mode,
    plan_charge_mode,
};
use lctrl_hal::{BatteryControl, poll_readback};

use crate::{EnergyDriver, GbmdCommand, IoctlTransport};

/// A readback source for the effective charge mode.
///
/// The production implementation reads the GBMD status bitmask; the controller
/// refuses writes until a reader is attached.
pub trait ChargeModeReader: Send + Sync {
    fn read_charge_mode(&self) -> Result<ChargeModeActual>;
}

/// Explicitly denies charge-mode writes until a firmware readback transport is
/// independently verified. This prevents a successful IOCTL from being
/// mistaken for semantic success on the target machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnverifiedChargeModeReader;

impl ChargeModeReader for UnverifiedChargeModeReader {
    fn read_charge_mode(&self) -> Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}

/// Reads the charge mode through the GBMD status bitmask (`0x831020f8`/`0xFF`).
///
/// The bit decode mirrors `PowerBattery.dll!CChargingMode::GetChargingMode`
/// and its post-write poll loop (RE-proven against the exact target binaries,
/// docs/RE-interfaces.md §10.3), so the same value the vendor waits on is the
/// value this controller verifies against.
#[derive(Clone, Copy, Debug, Default)]
pub struct GbmdChargeModeReader<I>(I);

impl<I> GbmdChargeModeReader<I> {
    #[must_use]
    pub const fn new(ioctl: I) -> Self {
        Self(ioctl)
    }
}

impl<I> ChargeModeReader for GbmdChargeModeReader<I>
where
    I: IoctlTransport,
{
    fn read_charge_mode(&self) -> Result<ChargeModeActual> {
        Ok(decode_gbmd_charge_mode(EnergyDriver::new(&self.0).gbmd_status()?))
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
        match self.reader.read_charge_mode()? {
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

    fn commit_charge_mode(&self, mode: ChargeMode) -> Result<ChargeMode> {
        for primitive in plan_charge_mode(mode) {
            self.write_primitive(primitive)?;
        }
        poll_readback(&mode, 10, Duration::from_millis(50), || {
            self.mode_for_write()
        })
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
                pid: (detail.pid != u16::MAX).then_some(detail.pid),
                vid: (detail.vid != u16::MAX).then_some(detail.vid),
                system_power_w: detail.system_power_w,
                current_power_w: detail.current_power_w,
            })
        } else {
            None
        };
        Ok(AdapterInfo::from_gbmd(status, detail))
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        self.reader.read_charge_mode()
    }

    fn night_charge(&self) -> Result<lctrl_core::NightChargeState> {
        let driver = EnergyDriver::new(&self.ioctl);
        let mut state =
            lctrl_core::NightChargeState::from_probe(driver.night_charge_probe()?);
        state.enabled =
            lctrl_core::NightChargeState::enabled_from_status(driver.night_charge_status()?);
        Ok(state)
    }

    fn set_night_charge(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<bool>> {
        let previous = lctrl_core::NightChargeState::enabled_from_status(
            EnergyDriver::new(&self.ioctl).night_charge_status()?,
        );
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, enabled));
        }
        let result = (|| {
            EnergyDriver::new(&self.ioctl).write_night_charge(enabled)?;
            poll_readback(&enabled, 10, Duration::from_millis(50), || {
                Ok(lctrl_core::NightChargeState::enabled_from_status(
                    EnergyDriver::new(&self.ioctl).night_charge_status()?,
                ))
            })
        })();
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, enabled, actual)),
            Err(error) => match self.set_night_charge(previous, ApplyMode::Commit) {
                Ok(_) => Err(error),
                Err(rollback) => Err(LctrlError::FirmwareRejected {
                    detail: format!(
                        "night-charge transition failed ({error}); restoring {previous} also failed ({rollback}); device state is unverified"
                    ),
                }),
            },
        }
    }

    fn temporary_charge(&self) -> Result<lctrl_core::TemporaryChargeState> {
        Ok(lctrl_core::TemporaryChargeState::from_raw(
            EnergyDriver::new(&self.ioctl).temporary_charge_status()?,
        ))
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

        let result = (|| {
            for primitive in plan_charge_mode(mode) {
                // An error or malformed reply does not establish that the
                // device ignored the request. Recovery is required even if
                // the first attempted primitive reports failure.
                self.write_primitive(primitive)?;
            }
            poll_readback(&mode, 10, Duration::from_millis(50), || {
                self.mode_for_write()
            })
        })();
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, mode, actual)),
            Err(error) => match self.commit_charge_mode(previous) {
                Ok(_) => Err(error),
                Err(rollback) => Err(LctrlError::FirmwareRejected {
                    detail: format!(
                        "charge-mode transition failed ({error}); restoring {previous} also failed ({rollback}); device state is unverified"
                    ),
                }),
            },
        }
    }
}
