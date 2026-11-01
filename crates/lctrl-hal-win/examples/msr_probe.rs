//! Read-only WinMsrDev probe: runs the documented NotifyPlatformInfo
//! handshake for the 21VG platform ID and reads all six selectors.
//! Never writes. Intended to run as LocalSystem (device ACL); any other
//! caller gets an honest access-denied error.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{MsrSelector, PLATFORM_ID_21VG, WIN_MSR_TRANSPORT, WinMsrDev};

#[cfg(windows)]
fn main() {
    let client = WinMsrDev::new(&WIN_MSR_TRANSPORT);
    match client.notify_platform_info(PLATFORM_ID_21VG, 0) {
        Ok(()) => println!("notify_platform_info(0x36): OK"),
        Err(error) => {
            println!("notify_platform_info(0x36): ERR {error}");
            return;
        }
    }
    for selector in MsrSelector::all() {
        match client.read_selector(selector) {
            Ok(value) => println!(
                "selector {} (MSR 0x{:03X}): 0x{value:016X}",
                selector.raw(),
                selector.msr_address()
            ),
            Err(error) => println!(
                "selector {} (MSR 0x{:03X}): ERR {error}",
                selector.raw(),
                selector.msr_address()
            ),
        }
    }
}
