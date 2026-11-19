use lenovo_laptop_control_core::{
    AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode, ChargeModeActual,
    FanDescriptor, FanId, FanMode, PerformanceMode, PerformanceState, PowerMutation, PowerScheme,
    PowerSchemeId, PowerSettingKey, PowerSettingValue, PowerSource,
};
use lenovo_laptop_control_hal::{BatteryControl, FanControl, PerformanceControl, PowerControl};

struct FakeP0;

impl BatteryControl for FakeP0 {
    fn battery_telemetry(&self, _index: u32) -> lenovo_laptop_control_core::Result<BatteryTelemetry> {
        unreachable!()
    }
    fn adapter_info(&self) -> lenovo_laptop_control_core::Result<AdapterInfo> {
        unreachable!()
    }
    fn charge_mode(&self) -> lenovo_laptop_control_core::Result<ChargeModeActual> {
        Ok(ChargeModeActual::Normal)
    }
    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> lenovo_laptop_control_core::Result<ChangeReport<ChargeMode>> {
        unreachable!()
    }
}

impl PerformanceControl for FakeP0 {
    fn performance_state(&self) -> lenovo_laptop_control_core::Result<PerformanceState> {
        unreachable!()
    }
    fn set_performance_mode(
        &self,
        _mode: PerformanceMode,
        _apply: ApplyMode,
    ) -> lenovo_laptop_control_core::Result<ChangeReport<PerformanceMode>> {
        unreachable!()
    }
}

impl FanControl for FakeP0 {
    fn fan_mode(&self) -> lenovo_laptop_control_core::Result<FanMode> {
        Ok(FanMode::Standard)
    }
    fn set_fan_mode(
        &self,
        mode: FanMode,
        apply: ApplyMode,
    ) -> lenovo_laptop_control_core::Result<ChangeReport<FanMode>> {
        Ok(match apply {
            ApplyMode::DryRun => ChangeReport::dry_run(FanMode::Standard, mode),
            ApplyMode::Commit => ChangeReport::committed(FanMode::Standard, mode, mode),
        })
    }
    fn fans(&self) -> lenovo_laptop_control_core::Result<Vec<FanDescriptor>> {
        Ok(vec![])
    }
    fn fan_table(
        &self,
        _fan: FanId,
        _sensor: lenovo_laptop_control_core::SensorId,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::FanTable> {
        unreachable!()
    }
}

impl PowerControl for FakeP0 {
    fn power_schemes(&self) -> lenovo_laptop_control_core::Result<Vec<PowerScheme>> {
        Ok(vec![])
    }

    fn active_power_scheme(&self) -> lenovo_laptop_control_core::Result<PowerScheme> {
        unreachable!()
    }

    fn power_value_range(
        &self,
        _key: &PowerSettingKey,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::PowerValueRange> {
        unreachable!()
    }

    fn read_power_setting(
        &self,
        _scheme: &PowerSchemeId,
        _key: &PowerSettingKey,
    ) -> lenovo_laptop_control_core::Result<lenovo_laptop_control_core::PowerSettingObservation> {
        unreachable!()
    }

    fn apply_power_mutation(
        &self,
        _mutation: PowerMutation,
        _apply: ApplyMode,
    ) -> lenovo_laptop_control_core::Result<ChangeReport<PowerMutation>> {
        unreachable!()
    }
}

#[test]
fn p0_extension_traits_are_object_safe() {
    let value = FakeP0;
    let battery: &dyn BatteryControl = &value;
    let performance: &dyn PerformanceControl = &value;
    let fan: &dyn FanControl = &value;
    let power: &dyn PowerControl = &value;

    assert_eq!(battery.charge_mode().unwrap(), ChargeModeActual::Normal);
    assert!(fan.fans().unwrap().is_empty());
    assert!(power.power_schemes().unwrap().is_empty());
    let _ = performance;
}

#[test]
fn power_mutation_api_keeps_ac_dc_in_payload() {
    let key = PowerSettingKey {
        subgroup: lenovo_laptop_control_core::PowerGuid::new("subgroup").unwrap(),
        setting: lenovo_laptop_control_core::PowerGuid::new("setting").unwrap(),
    };
    let range = lenovo_laptop_control_core::PowerValueRange::new(0, 100, 1).unwrap();
    let mutation = PowerMutation::SetValue {
        key,
        source: PowerSource::Dc,
        value: PowerSettingValue::new(50, &range).unwrap(),
    };
    assert!(matches!(
        mutation,
        PowerMutation::SetValue {
            source: PowerSource::Dc,
            ..
        }
    ));
    let _ = PowerSchemeId::new("balanced").unwrap();
}
