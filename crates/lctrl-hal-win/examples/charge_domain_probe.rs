//! Read-only charge-domain probe: reports the GBMD charge-mode decode, USB
//! charging flags, night-charge probe, temporary charge mode, the advertised
//! Storage80 bit (bit23 only, not the full vendor predicate), the vendor
//! storage subtype bit (bit14), and the decoded EM Spec version from the live
//! EnergyDrv channel.
//! Never writes. Used for C-acceptance evidence.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{EnergyDriver, NativeIoctl};

#[cfg(windows)]
fn main() {
    let driver = EnergyDriver::new(&NativeIoctl);

    match driver.gbmd_status() {
        Ok(raw) => println!(
            "gbmd_status: 0x{raw:08x} -> charge mode {:?} (decode: bit5 storage, bit2 express) storage80_bit23={} storage_subtype2_bit14={}",
            lctrl_core::decode_gbmd_charge_mode(raw),
            lctrl_hal_win::GbmdCommand::storage80_advertised(raw),
            raw & (1 << 14) != 0,
        ),
        Err(error) => println!("gbmd_status: ERROR {error}"),
    }

    match driver.em_spec_version() {
        Ok(version) => println!(
            "em_spec_version: {version} (request DWORD 0x11, reply bit0 valid, version = raw >> 4) storage80_em_min={} em_gate_met={}",
            lctrl_hal_win::EM_SPEC_VERSION_STORAGE_80_MIN,
            version >= lctrl_hal_win::EM_SPEC_VERSION_STORAGE_80_MIN,
        ),
        Err(error) => println!("em_spec_version: ERROR {error}"),
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
