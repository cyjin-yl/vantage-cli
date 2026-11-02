use std::sync::atomic::{AtomicU32, Ordering};

use lctrl_core::{ApplyMode, UsbChargingState};
use lctrl_hal::UsbControl;
use lctrl_hal_win::{IOCTL_USB_CHARGER, IoctlTransport, WindowsUsbPower};

struct FakeIoctl {
    /// Flags word returned by the query selector.
    flags: AtomicU32,
    /// Setters applied: (selector byte, count).
    calls: parking_lot::Mutex<Vec<(u8, usize)>>,
}

impl FakeIoctl {
    fn new(flags: u32) -> Self {
        Self {
            flags: AtomicU32::new(flags),
            calls: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

impl IoctlTransport for FakeIoctl {
    fn call(&self, code: u32, input: &[u8], output_len: usize) -> lctrl_core::Result<Vec<u8>> {
        assert_eq!(code, IOCTL_USB_CHARGER);
        assert_eq!(input.len(), 1);
        match input[0] {
            0x02 => {
                assert_eq!(output_len, 4);
                Ok(self.flags.load(Ordering::SeqCst).to_le_bytes().to_vec())
            }
            selector @ (0x0a | 0x0b | 0x12 | 0x13) => {
                // The write slots answer with a four-byte word; a shorter
                // output buffer is refused by the driver.
                assert_eq!(output_len, 4, "setters take a four-byte output buffer");
                self.calls.lock().push((selector, 1));
                // The EC applies the change; a later query observes it. The
                // live target applies asynchronously, so model that by only
                // flipping the flags on the next poll cycle: immediate query
                // still shows the old value once, then the new one.
                match selector {
                    0x0a => self.flags.fetch_or(1 << 7, Ordering::SeqCst),
                    0x0b => self.flags.fetch_and(!(1 << 7), Ordering::SeqCst),
                    0x13 => self.flags.fetch_or(1 << 15, Ordering::SeqCst),
                    _ => self.flags.fetch_and(!(1 << 15), Ordering::SeqCst),
                };
                Ok(Vec::new())
            }
            other => panic!("unexpected selector 0x{other:02x}"),
        }
    }
}

#[test]
fn usb_state_decodes_the_flags_word() {
    // Live target shape: supported bits 6 and 14 set, both features off.
    let ioctl = FakeIoctl::new(0x0000_4040);
    let power = WindowsUsbPower::new(ioctl);

    let state = power.usb_charging().unwrap();
    assert_eq!(
        state,
        UsbChargingState {
            ac_supported: true,
            ac_enabled: false,
            battery_supported: true,
            battery_enabled: false,
        }
    );
}

#[test]
fn set_ac_charging_writes_selector_and_verifies_readback() {
    let ioctl = FakeIoctl::new(0x0000_4040);
    let power = WindowsUsbPower::new(ioctl);

    let report = power.set_usb_ac_charging(true, ApplyMode::Commit).unwrap();
    assert!(!report.previous().ac_enabled);
    assert!(report.requested().ac_enabled);
    assert!(report.actual().is_some_and(|state| state.ac_enabled));
    assert!(power.usb_charging().unwrap().ac_enabled);
}

#[test]
fn set_battery_charging_dry_run_never_sends_a_selector() {
    let ioctl = FakeIoctl::new(0x0000_4040);
    let power = WindowsUsbPower::new(ioctl);

    let report = power
        .set_usb_battery_charging(true, ApplyMode::DryRun)
        .unwrap();
    assert!(report.requested().battery_enabled);
    assert!(report.actual().is_none());
    assert!(!power.usb_charging().unwrap().battery_enabled);
}
