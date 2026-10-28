use lctrl_core::{
    AdapterAuthentication, AdapterInfo, ApplyMode, BatteryElectricalObservation, BatteryTelemetry,
    ChangeReport, ChargeMode, ChargeModeActual, LctrlError, OsBatterySnapshot, Result,
    SystemPowerStatus,
};
use lctrl_hal::{BatteryControl, BatteryElectricalSource};
use lctrl_hal_win::{OsBatteryChannel, WindowsBatterySources};

struct Vendor {
    available: bool,
}
impl BatteryControl for Vendor {
    fn battery_telemetry(&self, index: u32) -> Result<BatteryTelemetry> {
        if !self.available || index != 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: "vendor battery unavailable".into(),
            });
        }
        let mut raw = [0xff; 83];
        raw[0..2].copy_from_slice(&10000u16.to_le_bytes());
        raw[20..22].copy_from_slice(&15600u16.to_le_bytes());
        BatteryTelemetry::parse(&raw)
    }
    fn adapter_info(&self) -> Result<AdapterInfo> {
        if !self.available {
            return Err(LctrlError::ChannelUnavailable {
                channel: "vendor adapter unavailable".into(),
            });
        }
        Ok(AdapterInfo {
            ac_connected: None,
            status: None,
            connector_type: None,
            wattage_w: None,
            authentication: AdapterAuthentication::Inbox,
            has_detail: false,
            detail: None,
        })
    }
    fn charge_mode(&self) -> Result<ChargeModeActual> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
    fn set_charge_mode(&self, _: ChargeMode, _: ApplyMode) -> Result<ChangeReport<ChargeMode>> {
        Err(LctrlError::Unsupported {
            feature: "battery.charge-mode.readback".into(),
        })
    }
}
struct Os;
impl OsBatteryChannel for Os {
    fn snapshot(&self) -> Result<OsBatterySnapshot> {
        Ok(OsBatterySnapshot {
            design_capacity_mwh: Some(80_000),
            full_charge_capacity_mwh: Some(60_000),
            voltage_mv: Some(17_715),
            remaining_percent: Some(75),
            charging: Some(false),
            discharging: Some(false),
            ..Default::default()
        })
    }
    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        Ok(SystemPowerStatus {
            ac_line_status: Some(true),
            battery_life_percent: Some(99),
            ..Default::default()
        })
    }
}
struct Electrical;
impl BatteryElectricalSource for Electrical {
    fn read_electrical(&self) -> Result<BatteryElectricalObservation> {
        Ok(BatteryElectricalObservation {
            voltage_mv: Some(17_721),
            rate_mw: Some(-2500),
            power_state: 2,
            temperature_deci_kelvin: None,
            source_issues: Vec::new(),
        })
    }
}

#[test]
fn unbound_battery_and_power_sources_remain_distinct() {
    let sources = WindowsBatterySources::new(Vendor { available: true }, Os, Electrical);
    let device = sources.battery_telemetry(0).unwrap();
    let os = sources.os_battery_snapshot().unwrap();
    let electrical = sources.battery_electrical().unwrap();
    let system = sources.system_power_status().unwrap();
    assert_eq!(device.design_capacity_mwh, Some(100_000));
    assert_eq!(device.design_voltage_mv, Some(15_600));
    assert_eq!(device.voltage_mv, None);
    assert_eq!(device.current_ma, None);
    assert_eq!(os.design_capacity_mwh, Some(80_000));
    assert_eq!(os.voltage_mv, Some(17_715));
    assert_eq!(os.remaining_percent, Some(75));
    assert_eq!(electrical.voltage_mv, Some(17_721));
    assert_eq!(electrical.rate_mw, Some(-2500));
    assert_eq!(system.battery_life_percent, Some(99));
}

#[test]
fn missing_vendor_does_not_alias_an_os_pack_or_hide_aggregate_power() {
    let sources = WindowsBatterySources::new(Vendor { available: false }, Os, Electrical);
    assert!(matches!(
        sources.battery_telemetry(0),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert!(matches!(
        sources.adapter_info(),
        Err(LctrlError::ChannelUnavailable { .. })
    ));
    assert_eq!(
        sources.os_battery_snapshot().unwrap().voltage_mv,
        Some(17_715)
    );
    assert_eq!(
        sources.system_power_status().unwrap().ac_line_status,
        Some(true)
    );
}

#[test]
fn vendor_index_and_adapter_are_not_filled_from_os_state() {
    let sources = WindowsBatterySources::new(Vendor { available: true }, Os, Electrical);
    assert!(sources.battery_telemetry(1).is_err());
    assert_eq!(sources.adapter_info().unwrap().ac_connected, None);
    assert_eq!(
        sources.system_power_status().unwrap().ac_line_status,
        Some(true)
    );
    assert!(matches!(
        sources.set_charge_mode(ChargeMode::Conservation, ApplyMode::Commit),
        Err(LctrlError::Unsupported { .. })
    ));
}
