use std::collections::BTreeMap;

use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryTelemetry, ChargeMode, ChargeModeActual, LctrlError,
    OsBatterySnapshot, Result, SystemPowerStatus,
};
use lctrl_hal::BatteryControl;
use lctrl_hal_win::{
    OsBatteryChannel, SystemPowerStatusReader, WindowsBatteryFallback, WindowsOsBattery, WmiObject,
    WmiTransport, WmiValue,
};

struct StubChannel {
    snapshot: Option<OsBatterySnapshot>,
    power: SystemPowerStatus,
}

impl OsBatteryChannel for StubChannel {
    fn snapshot(&self) -> Result<OsBatterySnapshot> {
        self.snapshot.ok_or(LctrlError::ChannelUnavailable {
            channel: "stub battery class".into(),
        })
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        Ok(self.power)
    }
}

struct FailingChannel;

impl OsBatteryChannel for FailingChannel {
    fn snapshot(&self) -> Result<OsBatterySnapshot> {
        Err(LctrlError::ChannelUnavailable {
            channel: "stub battery class".into(),
        })
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        Err(LctrlError::ChannelUnavailable {
            channel: "stub battery class".into(),
        })
    }
}

/// A vendor channel that is unavailable, like a machine without EnergyDrv.
struct UnavailableVendor;

impl BatteryControl for UnavailableVendor {
    fn battery_telemetry(&self, _index: u32) -> Result<BatteryTelemetry> {
        Err(LctrlError::ChannelUnavailable {
            channel: "stub energy driver".into(),
        })
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        Err(LctrlError::ChannelUnavailable {
            channel: "stub energy driver".into(),
        })
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }

    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> Result<lctrl_core::ChangeReport<ChargeMode>> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}

struct VendorTelemetry(BatteryTelemetry);

impl BatteryControl for VendorTelemetry {
    fn battery_telemetry(&self, _index: u32) -> Result<BatteryTelemetry> {
        Ok(self.0.clone())
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        Ok(AdapterInfo {
            ac_connected: Some(true),
            status: Some(lctrl_core::AdapterStatus::Full),
            connector_type: None,
            wattage_w: Some(65),
            authentication: lctrl_core::AdapterAuthentication::Lenovo,
            has_detail: true,
            detail: None,
        })
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }

    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> Result<lctrl_core::ChangeReport<ChargeMode>> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}

/// A vendor channel that knows the telemetry but not the AC line state.
struct VendorWithoutAcState(BatteryTelemetry);

impl BatteryControl for VendorWithoutAcState {
    fn battery_telemetry(&self, _index: u32) -> Result<BatteryTelemetry> {
        Ok(self.0.clone())
    }

    fn adapter_info(&self) -> Result<AdapterInfo> {
        Ok(AdapterInfo {
            ac_connected: None,
            status: None,
            connector_type: None,
            wattage_w: None,
            authentication: lctrl_core::AdapterAuthentication::Inbox,
            has_detail: false,
            detail: None,
        })
    }

    fn charge_mode(&self) -> Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }

    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> Result<lctrl_core::ChangeReport<ChargeMode>> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}

fn empty_telemetry() -> BatteryTelemetry {
    BatteryTelemetry {
        design_capacity_mwh: Some(99_900),
        full_charge_capacity_mwh: None,
        remaining_capacity_mwh: None,
        voltage_mv: None,
        current_ma: None,
        temperature_deci_kelvin: None,
        manufacture_date: None,
        first_used_date: None,
        design_voltage_mv: None,
        remaining_percent: None,
        life_percent: None,
        charge_status: None,
        health: None,
        remaining_time_min: None,
        charge_completion_time_min: None,
        wattage_w: None,
        cycle_count: None,
        manufacturer: None,
        model_name: None,
        firmware_version: None,
        serial_number: None,
        chemistry: None,
    }
}

fn os_snapshot() -> OsBatterySnapshot {
    OsBatterySnapshot {
        remaining_capacity_mwh: Some(95_110),
        full_charge_capacity_mwh: Some(95_110),
        design_capacity_mwh: None,
        voltage_mv: Some(17_715),
        charge_rate_ma: Some(0),
        discharge_rate_ma: Some(0),
        remaining_percent: Some(100),
        cycle_count: Some(224),
        temperature_deci_kelvin: None,
        estimated_runtime_min: None,
        power_online: Some(true),
        charging: Some(false),
        discharging: Some(false),
        critical: Some(false),
    }
}

#[test]
fn fallback_serves_telemetry_from_the_os_channel_when_the_vendor_channel_is_absent() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus {
            ac_line_status: Some(true),
            battery_flag: Some(1),
            battery_life_percent: Some(100),
            battery_life_time: None,
            battery_full_life_time: None,
        },
    };
    let battery = WindowsBatteryFallback::new(UnavailableVendor, channel);

    let telemetry = battery.battery_telemetry(0).unwrap();

    assert_eq!(telemetry.remaining_capacity_mwh, Some(95_110));
    assert_eq!(telemetry.cycle_count, Some(224));
    assert_eq!(telemetry.remaining_percent, Some(100));
    assert_eq!(telemetry.design_capacity_mwh, None);
}

#[test]
fn fallback_serves_the_adapter_view_from_the_os_channel_when_the_vendor_channel_is_absent() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus {
            ac_line_status: Some(true),
            battery_flag: Some(1),
            battery_life_percent: Some(100),
            battery_life_time: None,
            battery_full_life_time: None,
        },
    };
    let battery = WindowsBatteryFallback::new(UnavailableVendor, channel);

    let adapter = battery.adapter_info().unwrap();

    assert_eq!(adapter.ac_connected, Some(true));
    assert_eq!(adapter.status, None);
    assert_eq!(adapter.wattage_w, None);
}

#[test]
fn fallback_keeps_the_vendor_values_and_fills_only_the_gaps() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus::default(),
    };
    let battery = WindowsBatteryFallback::new(VendorTelemetry(empty_telemetry()), channel);

    let telemetry = battery.battery_telemetry(0).unwrap();

    assert_eq!(telemetry.design_capacity_mwh, Some(99_900));
    assert_eq!(telemetry.cycle_count, Some(224));
    assert_eq!(telemetry.remaining_capacity_mwh, Some(95_110));
    assert_eq!(telemetry.voltage_mv, Some(17_715));
}

#[test]
fn fallback_prefers_the_observed_charging_state_over_the_vendor_status_word() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus::default(),
    };
    let battery = WindowsBatteryFallback::new(VendorTelemetry(empty_telemetry()), channel);

    let telemetry = battery.battery_telemetry(0).unwrap();

    // The operating system reports neither charging nor discharging here,
    // which is a real observation and not a missing value.
    assert_eq!(
        telemetry.charge_status,
        Some(lctrl_core::ChargeStatus::NoActivity)
    );
}

#[test]
fn fallback_fills_the_ac_line_state_when_the_vendor_channel_has_no_detail() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus {
            ac_line_status: Some(true),
            battery_flag: Some(1),
            battery_life_percent: Some(100),
            battery_life_time: None,
            battery_full_life_time: None,
        },
    };
    let battery = WindowsBatteryFallback::new(VendorWithoutAcState(empty_telemetry()), channel);

    let adapter = battery.adapter_info().unwrap();

    assert_eq!(adapter.ac_connected, Some(true));
}

#[test]
fn fallback_prefers_the_vendor_adapter_view() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus {
            ac_line_status: Some(false),
            battery_flag: None,
            battery_life_percent: None,
            battery_life_time: None,
            battery_full_life_time: None,
        },
    };
    let battery = WindowsBatteryFallback::new(VendorTelemetry(empty_telemetry()), channel);

    let adapter = battery.adapter_info().unwrap();

    assert_eq!(adapter.ac_connected, Some(true));
    assert_eq!(adapter.wattage_w, Some(65));
    assert_eq!(
        adapter.authentication,
        lctrl_core::AdapterAuthentication::Lenovo
    );
}

#[test]
fn fallback_reports_the_channel_error_when_neither_source_works() {
    let battery = WindowsBatteryFallback::new(UnavailableVendor, FailingChannel);

    assert!(matches!(
        battery.battery_telemetry(0),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(matches!(
        battery.adapter_info(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
}

#[test]
fn fallback_never_claims_a_verified_charge_mode() {
    let channel = StubChannel {
        snapshot: Some(os_snapshot()),
        power: SystemPowerStatus::default(),
    };
    let battery = WindowsBatteryFallback::new(UnavailableVendor, channel);

    assert!(matches!(
        battery.charge_mode(),
        Err(LctrlError::Unsupported { .. })
    ));
    assert!(matches!(
        battery.set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
}

#[test]
fn os_channel_reads_the_battery_class_and_system_power_status_together() {
    struct OneShotWmi;

    impl WmiTransport for OneShotWmi {
        fn query(&self, _namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
            if wql.contains("BatteryStatus") {
                let object: WmiObject = [
                    ("Active".to_owned(), WmiValue::Bool(true)),
                    ("RemainingCapacity".to_owned(), WmiValue::U32(95_110)),
                    ("Voltage".to_owned(), WmiValue::U32(17_715)),
                    ("PowerOnline".to_owned(), WmiValue::Bool(true)),
                    ("Charging".to_owned(), WmiValue::Bool(false)),
                    ("Discharging".to_owned(), WmiValue::Bool(false)),
                ]
                .into_iter()
                .collect::<BTreeMap<_, _>>();
                return Ok(vec![object]);
            }
            Ok(Vec::new())
        }

        fn invoke_instance(
            &self,
            _namespace: &str,
            _class: &str,
            _object_path: &str,
            _method: &str,
            _input: &WmiObject,
        ) -> Result<WmiObject> {
            Err(LctrlError::Unsupported {
                feature: "test.invoke".into(),
            })
        }
    }

    struct StaticPower(SystemPowerStatus);

    impl SystemPowerStatusReader for StaticPower {
        fn read(&self) -> Result<SystemPowerStatus> {
            Ok(self.0)
        }
    }

    let channel = WindowsOsBattery::new(
        OneShotWmi,
        StaticPower(SystemPowerStatus {
            ac_line_status: Some(true),
            battery_flag: Some(1),
            battery_life_percent: Some(100),
            battery_life_time: None,
            battery_full_life_time: None,
        }),
    );

    let snapshot = channel.snapshot().unwrap();

    assert_eq!(snapshot.remaining_capacity_mwh, Some(95_110));
    assert_eq!(snapshot.remaining_percent, Some(100));
    assert_eq!(snapshot.power_online, Some(true));
}
