//! Read-only DAX RPC probe: opens the `ncalrpc:[DaxRpcEndpoint]` session
//! through the pure-Rust rpcrt4 transport and reports every read opnum.
//! Never writes. Used to prove the pure-Rust client against live DAX3API.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{DolbyRpc, NativeDolbyRpc};

#[cfg(windows)]
fn main() {
    let rpc = match NativeDolbyRpc::load() {
        Ok(rpc) => rpc,
        Err(error) => {
            println!("connect: ERROR {error}");
            return;
        }
    };
    println!("connect: ok");

    match rpc.dolby_enabled() {
        Ok(v) => println!("GetDolbyEnabled (op5): {v}"),
        Err(e) => println!("GetDolbyEnabled (op5): ERROR {e}"),
    }
    match rpc.active_profile() {
        Ok(v) => println!("GetActiveProfile (op7): {v}"),
        Err(e) => println!("GetActiveProfile (op7): ERROR {e}"),
    }
    match rpc.spatial_supported() {
        Ok(v) => println!("IsSpatialAudioSupported (op2): {v}"),
        Err(e) => println!("IsSpatialAudioSupported (op2): ERROR {e}"),
    }
    match rpc.sku() {
        Ok(v) => println!("GetSKU (op37): {v}"),
        Err(e) => println!("GetSKU (op37): ERROR {e}"),
    }
}
