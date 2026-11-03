//! Read-only diagnostic: dump the raw GetAllSetting array + capability so the
//! field-index projection can be validated against live service state and the
//! HKLM SmartSense registry mirror. No writes.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{HpdRpc, NativeHpdRpc};

#[cfg(windows)]
fn main() {
    let rpc = NativeHpdRpc::new();
    match rpc.capability() {
        Ok(cap) => println!("capability: 0x{cap:04x}"),
        Err(e) => eprintln!("capability failed: {e}"),
    }
    match rpc.raw_settings() {
        Ok(values) => {
            for (index, value) in values.iter().enumerate() {
                println!("values[{index:2}] = {value}");
            }
        }
        Err(e) => eprintln!("raw_settings failed: {e}"),
    }
    match rpc.read_state() {
        Ok(state) => println!("{state:?}"),
        Err(e) => eprintln!("read_state failed: {e}"),
    }
}
