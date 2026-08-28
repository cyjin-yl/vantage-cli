use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use vantage_daemon::DaemonEvent;

fn main() {
    let (events, receiver) = mpsc::channel();
    spawn_event_source(events);
    if let Err(error) = vantage_daemon::run(receiver) {
        eprintln!("{error}");
        std::process::exit(i32::from(error.exit_code()));
    }
}

#[cfg(target_os = "linux")]
fn spawn_event_source(events: mpsc::Sender<DaemonEvent>) {
    use lctrl_hal::{
        BatteryControl, FanControl, Hal, MagicBayControl, PerformanceControl, PowerControl,
    };

    thread::spawn(move || {
        let hal = lctrl_hal_linux::LinuxHal::new();
        let mut previous = Value::Null;
        loop {
            let snapshot = json!({
                "hardware": value_or_error(hal.hardware_info()),
                "capabilities": value_or_error(hal.capabilities()),
                "adapter": value_or_error(hal.adapter_info()),
                "charge_mode": value_or_error(hal.charge_mode()),
                "performance": value_or_error(hal.performance_state()),
                "fan_mode": value_or_error(hal.fan_mode()),
                "power_scheme": value_or_error(hal.active_power_scheme()),
                "magicbay": value_or_error(hal.detect_magicbay()),
            });
            if snapshot != previous {
                if events
                    .send(DaemonEvent::now("hardware_state_changed", snapshot.clone()))
                    .is_err()
                {
                    break;
                }
                previous = snapshot;
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(windows)]
fn spawn_event_source(events: mpsc::Sender<DaemonEvent>) {
    use lctrl_hal::{Hal, MagicBayControl, PerformanceControl, PowerControl};

    thread::spawn(move || {
        let hal = lctrl_hal_win::NativeWindowsHal::new(
            lctrl_hal_win::NativeWmi,
            lctrl_hal_win::NativeIoctl,
        );
        let performance =
            lctrl_hal_win::WindowsPerformanceP0::new(lctrl_hal_win::NativePerformanceRegistry);
        let power = lctrl_hal_win::WindowsPowerP0::new(lctrl_hal_win::NativePowerApi);
        let magicbay = lctrl_hal_win::NativeMagicBay;
        let mut previous = Value::Null;
        loop {
            let snapshot = json!({
                "hardware": value_or_error(hal.hardware_info()),
                "capabilities": value_or_error(hal.capabilities()),
                "performance": value_or_error(performance.performance_state()),
                "power_scheme": value_or_error(power.active_power_scheme()),
                "magicbay": value_or_error(magicbay.detect_magicbay()),
            });
            if snapshot != previous {
                if events
                    .send(DaemonEvent::now("hardware_state_changed", snapshot.clone()))
                    .is_err()
                {
                    break;
                }
                previous = snapshot;
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(not(any(target_os = "linux", windows)))]
fn spawn_event_source(_events: mpsc::Sender<DaemonEvent>) {}

fn value_or_error<T: serde::Serialize>(result: lctrl_core::Result<T>) -> Value {
    match result {
        Ok(value) => serde_json::to_value(value).unwrap_or(Value::Null),
        Err(error) => serde_json::to_value(error.report()).unwrap_or_else(
            |_| json!({"error": {"kind": "serialization", "message": error.to_string()}}),
        ),
    }
}
