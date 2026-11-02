//! Read-only IGCL probe: prints the loaded library, the negotiated ABI
//! version, the pixel-transform capability and the super-resolution state.
//! It never submits a configuration change, and it runs as its own process
//! so a driver-side fault cannot take the CLI down with it.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use lctrl_hal_win::{IntelControlLib, IntelCtlApi};

    match IntelControlLib::load() {
        Ok(api) => {
            println!("library: {}", api.library_path().display());
            match api.primary_output() {
                Ok(Some(handle)) => println!("display output handle: {handle:p}"),
                Ok(None) => println!("display output handle: none"),
                Err(error) => println!("display output handle: error {error}"),
            }
            match api.pixtx_capability() {
                Ok(capability) if capability.blocks.is_empty() => {
                    println!("pixtx: no pixel-transform blocks on this output (pipeline absent)");
                }
                Ok(capability) => {
                    println!("pixtx: {} block(s)", capability.blocks.len());
                    for block in capability.blocks {
                        println!(
                            "  id=0x{:08x} type={} sampling={:?} samples_per_channel={:?} channels={:?}",
                            block.block_id,
                            block.block_type,
                            block.sampling_type,
                            block.samples_per_channel,
                            block.channels
                        );
                    }
                }
                Err(error) => println!("pixtx capability: {error}"),
            }
            match api.super_resolution_capability() {
                Ok(caps) => println!(
                    "super-resolution caps: flags=0x{:x} width {}..{} height {}..{}",
                    caps.flags, caps.min_width, caps.max_width, caps.min_height, caps.max_height
                ),
                Err(error) => println!("super-resolution caps: {error}"),
            }
            match api.super_resolution_flag() {
                Ok(flag) => println!("super-resolution flag: 0x{flag:x}"),
                Err(error) => println!("super-resolution flag: {error}"),
            }
        }
        Err(error) => println!("IntelControlLib load failed: {error}"),
    }
}
