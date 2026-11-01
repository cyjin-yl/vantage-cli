//! Read-only charge-domain probe: reports the GBMD charge-mode decode, USB
//! charging flags, night-charge probe, and temporary charge mode from the
//! live EnergyDrv channel. Never writes. Used for C-acceptance evidence.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{EnergyDriver, NativeIoctl};

#[cfg(windows)]
fn main() {
    let driver = EnergyDriver::new(&NativeIoctl);

    match driver.gbmd_status() {
        Ok(raw) => println!(
            "gbmd_status: 0x{raw:08x} -> charge mode {:?} (decode: bit5 storage, bit2 express)",
            lctrl_core::decode_gbmd_charge_mode(raw)
        ),
        Err(error) => println!("gbmd_status: ERROR {error}"),
    }

    match driver.usb_charger_flags() {
        Ok(raw) => println!(
            "usb_charger: 0x{raw:08x} -> {}",
            lctrl_core::UsbChargingState::from_flags(raw)
        ),
        Err(error) => println!("usb_charger: ERROR {error}"),
    }

    match driver.night_charge_probe() {
        Ok(raw) => println!(
            "night_charge probe: 0x{raw:08x} -> supported={} (probe bit4 is not the live state; see status)",
            raw & 1 != 0
        ),
        Err(error) => println!("night_charge probe: ERROR {error}"),
    }

    match driver.night_charge_status() {
        Ok(raw) => println!(
            "night_charge status: 0x{raw:08x} -> enabled={}",
            lctrl_core::NightChargeState::enabled_from_status(raw)
        ),
        Err(error) => println!("night_charge status: ERROR {error}"),
    }

    match driver.temporary_charge_status() {
        Ok(raw) => println!(
            "temporary_charge: 0x{raw:08x} -> {:?}",
            lctrl_core::TemporaryChargeState::from_raw(raw)
        ),
        Err(error) => println!("temporary_charge: ERROR {error}"),
    }
}
