use parking_lot::Mutex;

use lctrl_core::{
    ApplyMode, LctrlError, PowerGuid, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey,
    PowerSettingValue, PowerSource, PowerValueRange,
};
use lctrl_hal::PowerControl;
use lctrl_hal_win::{PowerApi, WindowsPowerP0};

struct FakePowerApi {
    schemes: Mutex<Vec<PowerScheme>>,
    active: Mutex<PowerSchemeId>,
    value: Mutex<u32>,
    calls: Mutex<Vec<&'static str>>,
}

impl FakePowerApi {
    fn new() -> Self {
        let balanced = PowerScheme::new(PowerSchemeId::new("balanced").unwrap(), "Balanced", true);
        let performance = PowerScheme::new(
            PowerSchemeId::new("performance").unwrap(),
            "Performance",
            false,
        );
        Self {
            schemes: Mutex::new(vec![balanced.clone(), performance]),
            active: Mutex::new(balanced.id),
            value: Mutex::new(50),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl PowerApi for FakePowerApi {
    fn schemes(&self) -> lctrl_core::Result<Vec<PowerScheme>> {
        self.calls.lock().push("schemes");
        Ok(self.schemes.lock().clone())
    }

    fn active_scheme(&self) -> lctrl_core::Result<PowerScheme> {
        self.calls.lock().push("active");
        let active = self.active.lock();
        self.schemes
            .lock()
            .iter()
            .find(|scheme| scheme.id == *active)
            .cloned()
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "missing active test scheme".into(),
            })
    }

    fn activate(&self, id: &PowerSchemeId) -> lctrl_core::Result<()> {
        self.calls.lock().push("activate");
        *self.active.lock() = id.clone();
        Ok(())
    }

    fn read_value(&self, _key: &PowerSettingKey, _source: PowerSource) -> lctrl_core::Result<u32> {
        self.calls.lock().push("read_value");
        Ok(*self.value.lock())
    }

    fn range(&self, _key: &PowerSettingKey) -> lctrl_core::Result<PowerValueRange> {
        self.calls.lock().push("range");
        PowerValueRange::new(0, 100, 10)
    }

    fn write_value(
        &self,
        _key: &PowerSettingKey,
        _source: PowerSource,
        value: u32,
    ) -> lctrl_core::Result<()> {
        self.calls.lock().push("write_value");
        *self.value.lock() = value;
        Ok(())
    }
}

fn mutation(value: u32) -> PowerMutation {
    let key = PowerSettingKey {
        subgroup: PowerGuid::new("subgroup").unwrap(),
        setting: PowerGuid::new("setting").unwrap(),
    };
    let range = PowerValueRange::new(0, 100, 10).unwrap();
    PowerMutation::SetValue {
        key,
        source: PowerSource::Ac,
        value: PowerSettingValue::new(value, &range).unwrap(),
    }
}

#[test]
fn dry_run_reads_current_value_but_does_not_write() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    let report = p0
        .apply_power_mutation(mutation(70), ApplyMode::DryRun)
        .unwrap();

    assert_eq!(report.actual(), None);
    assert_eq!(&*p0.api().calls.lock(), &["read_value", "range"]);
}

#[test]
fn set_value_reads_validates_writes_and_reads_back() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    let report = p0
        .apply_power_mutation(mutation(70), ApplyMode::Commit)
        .unwrap();

    assert_eq!(report.actual(), Some(&mutation(70)));
    assert_eq!(
        &*p0.api().calls.lock(),
        &["read_value", "range", "write_value", "read_value"]
    );
}

#[test]
fn activate_requires_enumerated_scheme_and_readback() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);
    let request = PowerMutation::Activate(PowerSchemeId::new("performance").unwrap());

    let report = p0
        .apply_power_mutation(request.clone(), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.actual(), Some(&request));
    assert_eq!(
        &*p0.api().calls.lock(),
        &["active", "schemes", "activate", "active"]
    );
}

#[test]
fn activate_rejects_unknown_scheme_before_write() {
    let api = FakePowerApi::new();
    let p0 = WindowsPowerP0::new(api);

    assert!(matches!(
        p0.apply_power_mutation(
            PowerMutation::Activate(PowerSchemeId::new("unknown").unwrap()),
            ApplyMode::Commit
        ),
        Err(LctrlError::InvalidArgument { .. })
    ));
    assert_eq!(&*p0.api().calls.lock(), &["active", "schemes"]);
}
