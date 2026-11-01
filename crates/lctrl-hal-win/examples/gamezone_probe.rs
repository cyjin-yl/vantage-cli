//! Read-only GAMEZONE probe: invokes getters and prints each observed outcome.
//! Never calls a setter. Used for on-target acceptance evidence.

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
use lctrl_hal_win::{GameZone, NativeWmi};

#[cfg(windows)]
fn main() {
    let zone = GameZone::new(NativeWmi);
    let probes: &[(&str, fn(&GameZone<NativeWmi>) -> lctrl_core::Result<u32>)] = &[
        ("thermal_mode", |z| z.thermal_mode()),
        ("power_charge_mode", |z| z.power_charge_mode()),
        ("fan_cooling_status", |z| z.fan_cooling_status()),
        ("smart_fan_mode", |z| z.smart_fan_mode()),
        ("keyboard_light", |z| z.keyboard_light()),
        ("winkey_status", |z| z.winkey_status()),
        ("touchpad_status", |z| z.touchpad_status()),
        ("od_status", |z| z.od_status()),
        ("cpu_temp", |z| z.cpu_temp()),
        ("gpu_temp", |z| z.gpu_temp()),
        ("cpu_frequency", |z| z.cpu_frequency()),
    ];
    for (name, probe) in probes {
        match probe(&zone) {
            Ok(value) => println!("{name}: OK value={value}"),
            Err(error) => println!("{name}: ERR {error}"),
        }
    }
}
