//! Read-only diagnostic: dump the raw GetAllSetting array + capability so the
//! field-index projection can be validated against live service state and the
//! HKLM SmartSense registry mirror. No writes.
//!
//! Optional write probe: `hpd_probe <op> <arg>` performs one
//! `NdrClientCall3` for ops 7–17 (op 7 ignores the arg) and then dumps the
//! settings array again, so the persistence encoding of each setter can be
//! observed on target. Ops outside the recovered table are refused.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{HpdRpc, NativeHpdRpc};

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rpc = NativeHpdRpc::new();
    if args.len() >= 3 {
        let op: u32 = args[1].parse().expect("op must be a decimal opnum 7..=17");
        let arg: u32 = args[2].parse().expect("arg must be a decimal u32");
        match rpc.raw_call(op, Some(arg)) {
            Ok(()) => println!("op {op} arg {arg}: accepted"),
            Err(e) => println!("op {op} arg {arg}: {e}"),
        }
    } else if args.len() == 2 {
        let op: u32 = args[1].parse().expect("op must be a decimal opnum 7..=17");
        match rpc.raw_call(op, None) {
            Ok(()) => println!("op {op} (context only): accepted"),
            Err(e) => println!("op {op} (context only): {e}"),
        }
    }
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
