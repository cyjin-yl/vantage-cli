use lctrl_core::{
    AdapterInfo, ApplyMode, BatteryTelemetry, ChangeReport, ChargeMode, ChargeModeActual,
    FanDescriptor, FanId, FanMode, PerformanceMode, PerformanceState, PowerMutation, PowerScheme,
    PowerSchemeId, PowerSettingKey, PowerSettingValue, PowerSource,
};
use lctrl_hal::{BatteryControl, FanControl, PerformanceControl, PowerControl};

struct FakeP0;

impl BatteryControl for FakeP0 {
    fn battery_telemetry(&self, _index: u32) -> lctrl_core::Result<BatteryTelemetry> {
        unreachable!()
    }
    fn adapter_info(&self) -> lctrl_core::Result<AdapterInfo> {
        unreachable!()
    }
    fn charge_mode(&self) -> lctrl_core::Result<ChargeModeActual> {
        Ok(ChargeModeActual::Normal)
    }
    fn set_charge_mode(
        &self,
        _mode: ChargeMode,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<ChargeMode>> {
        unreachable!()
    }
}

impl PerformanceControl for FakeP0 {
    fn performance_state(&self) -> lctrl_core::Result<PerformanceState> {
        unreachable!()
    }
    fn set_performance_mode(
        &self,
        _mode: PerformanceMode,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<PerformanceMode>> {
        unreachable!()
    }
}

impl FanControl for FakeP0 {
    fn fan_mode(&self) -> lctrl_core::Result<FanMode> {
        Ok(FanMode::Standard)
    }
    fn set_fan_mode(
        &self,
        mode: FanMode,
        apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<FanMode>> {
        Ok(match apply {
            ApplyMode::DryRun => ChangeReport::dry_run(FanMode::Standard, mode),
            ApplyMode::Commit => ChangeReport::committed(FanMode::Standard, mode, mode),
        })
    }
    fn fans(&self) -> lctrl_core::Result<Vec<FanDescriptor>> {
        Ok(vec![])
    }
    fn fan_table(
        &self,
        _fan: FanId,
        _sensor: lctrl_core::SensorId,
    ) -> lctrl_core::Result<lctrl_core::FanTable> {
        unreachable!()
    }
}

impl PowerControl for FakeP0 {
    fn power_schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
        Ok(vec![])
    }

    fn active_power_scheme(&self) -> lctrl_core::Result<PowerScheme> {
        unreachable!()
    }

    fn power_value_range(
        &self,
        _key: &PowerSettingKey,
    ) -> lctrl_core::Result<lctrl_core::PowerValueRange> {
        unreachable!()
    }

    fn apply_power_mutation(
        &self,
        _mutation: PowerMutation,
        _apply: ApplyMode,
    ) -> lctrl_core::Result<ChangeReport<PowerMutation>> {
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
        subgroup: lctrl_core::PowerGuid::new("subgroup").unwrap(),
        setting: lctrl_core::PowerGuid::new("setting").unwrap(),
    };
    let range = lctrl_core::PowerValueRange::new(0, 100, 1).unwrap();
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
