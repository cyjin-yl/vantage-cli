use lctrl_core::{LctrlError, SystemPowerStatus};
use lctrl_hal_win::{
    OsBatteryChannel, SystemPowerStatusReader, WindowsOsBattery, WmiObject, WmiTransport, WmiValue,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct FakeWmi {
    replies: BTreeMap<&'static str, Vec<WmiObject>>,
    failure: Option<&'static str>,
    queries: parking_lot::Mutex<Vec<String>>,
}
impl FakeWmi {
    fn standard() -> Self {
        Self {
            replies: BTreeMap::from([
                ("BatteryStatus", vec![status("pack")]),
                ("BatteryStaticData", vec![static_data("pack", 0x8000_0000)]),
                (
                    "BatteryFullChargedCapacity",
                    vec![row(
                        "pack",
                        &[("FullChargedCapacity", WmiValue::U32(100_000))],
                    )],
                ),
                (
                    "BatteryCycleCount",
                    vec![row("pack", &[("CycleCount", WmiValue::U32(224))])],
                ),
                (
                    "BatteryRuntime",
                    vec![row("pack", &[("EstimatedRuntime", WmiValue::U32(7200))])],
                ),
            ]),
            ..Self::default()
        }
    }
}
impl WmiTransport for FakeWmi {
    fn query(&self, namespace: &str, wql: &str) -> lctrl_core::Result<Vec<WmiObject>> {
        assert_eq!(namespace, "ROOT\\WMI");
        assert!(!wql.contains('*'));
        self.queries.lock().push(wql.into());
        let class = wql.split(" FROM ").nth(1).unwrap();
        if self.failure == Some(class) {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{class}: provider failed"),
            });
        }
        Ok(self.replies.get(class).cloned().unwrap_or_default())
    }
    fn invoke_instance(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &WmiObject,
    ) -> lctrl_core::Result<WmiObject> {
        panic!("read-only battery queries must not invoke methods")
    }
}
struct Power;
impl SystemPowerStatusReader for Power {
    fn read(&self) -> lctrl_core::Result<SystemPowerStatus> {
        Ok(SystemPowerStatus {
            battery_life_percent: Some(87),
            ..SystemPowerStatus::default()
        })
    }
}
fn row(name: &str, entries: &[(&str, WmiValue)]) -> WmiObject {
    let mut result: WmiObject = entries
        .iter()
        .map(|(key, value)| ((*key).into(), value.clone()))
        .collect();
    result.insert("InstanceName".into(), WmiValue::String(name.into()));
    result
}
fn status(name: &str) -> WmiObject {
    row(
        name,
        &[
            ("Active", WmiValue::Bool(true)),
            ("RemainingCapacity", WmiValue::U32(50_000)),
            ("Voltage", WmiValue::U32(17_715)),
            ("PowerOnline", WmiValue::Bool(true)),
            ("Charging", WmiValue::Bool(false)),
            ("Discharging", WmiValue::Bool(true)),
            ("ChargeRate", WmiValue::U32(25_000)),
        ],
    )
}
fn static_data(name: &str, capabilities: u32) -> WmiObject {
    row(
        name,
        &[
            ("Capabilities", WmiValue::U32(capabilities)),
            ("DesignedCapacity", WmiValue::U32(110_000)),
        ],
    )
}

#[test]
fn exact_association_survives_reordered_rows_and_excludes_short_term_devices() {
    let mut wmi = FakeWmi::standard();
    wmi.replies
        .get_mut("BatteryStatus")
        .unwrap()
        .insert(0, status("ups"));
    wmi.replies
        .get_mut("BatteryStaticData")
        .unwrap()
        .push(static_data("ups", 0xa000_0000));
    wmi.replies
        .get_mut("BatteryFullChargedCapacity")
        .unwrap()
        .insert(0, row("ups", &[("FullChargedCapacity", WmiValue::U32(1))]));
    let channel = WindowsOsBattery::new(wmi, Power);
    let snapshot = channel.snapshot().unwrap();
    assert_eq!(snapshot.remaining_capacity_mwh, Some(50_000));
    assert_eq!(snapshot.full_charge_capacity_mwh, Some(100_000));
    assert_eq!(snapshot.design_capacity_mwh, Some(110_000));
    assert_eq!(snapshot.remaining_percent, Some(50));
    assert_eq!(snapshot.voltage_mv, Some(17_715));
    assert_eq!(snapshot.cycle_count, Some(224));
    assert_eq!(snapshot.estimated_runtime_min, Some(120));
    let serialized = serde_json::to_value(&snapshot).unwrap();
    assert!(serialized.get("charge_rate_ma").is_none());
    assert!(serialized.get("discharge_rate_ma").is_none());
    assert!(snapshot.source_issues.is_empty());
    assert_eq!(channel.wmi().queries.lock().len(), 5);
}

#[test]
fn multiple_active_system_packs_are_not_arbitrarily_selected() {
    let mut wmi = FakeWmi::standard();
    wmi.replies
        .get_mut("BatteryStatus")
        .unwrap()
        .push(status("second"));
    wmi.replies
        .get_mut("BatteryStaticData")
        .unwrap()
        .push(static_data("second", 0x8000_0000));
    assert!(WindowsOsBattery::new(wmi, Power).snapshot().is_err());
}

#[test]
fn missing_charge_flags_remain_unknown() {
    let mut wmi = FakeWmi::standard();
    let status = &mut wmi.replies.get_mut("BatteryStatus").unwrap()[0];
    status.remove("Charging");
    status.remove("Discharging");
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.charging, None);
    assert_eq!(snapshot.discharging, None);
}

#[test]
fn known_non_system_short_term_inactive_and_absent_batteries_are_rejected() {
    for capabilities in [0, 0xa000_0000] {
        let mut wmi = FakeWmi::standard();
        wmi.replies
            .insert("BatteryStaticData", vec![static_data("pack", capabilities)]);
        assert!(WindowsOsBattery::new(wmi, Power).snapshot().is_err());
    }
    let mut inactive = FakeWmi::standard();
    inactive.replies.get_mut("BatteryStatus").unwrap()[0]
        .insert("Active".into(), WmiValue::Bool(false));
    assert!(WindowsOsBattery::new(inactive, Power).snapshot().is_err());
    assert!(
        WindowsOsBattery::new(FakeWmi::default(), Power)
            .snapshot()
            .is_err()
    );
}

#[test]
fn missing_and_empty_status_identities_are_rejected() {
    for name in [None, Some("")] {
        let mut wmi = FakeWmi::standard();
        let status = &mut wmi.replies.get_mut("BatteryStatus").unwrap()[0];
        status.remove("InstanceName");
        if let Some(name) = name {
            status.insert("InstanceName".into(), WmiValue::String(name.into()));
        }
        assert!(WindowsOsBattery::new(wmi, Power).snapshot().is_err());
    }
}

#[test]
fn duplicate_status_identity_is_rejected_even_if_one_row_is_inactive() {
    let mut wmi = FakeWmi::standard();
    let mut duplicate = status("pack");
    duplicate.insert("Active".into(), WmiValue::Bool(false));
    wmi.replies
        .get_mut("BatteryStatus")
        .unwrap()
        .push(duplicate);
    assert!(WindowsOsBattery::new(wmi, Power).snapshot().is_err());
}

#[test]
fn duplicate_optional_identity_does_not_contaminate_valid_status() {
    let mut wmi = FakeWmi::standard();
    wmi.replies
        .get_mut("BatteryFullChargedCapacity")
        .unwrap()
        .push(row("pack", &[("FullChargedCapacity", WmiValue::U32(1))]));
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.full_charge_capacity_mwh, None);
    assert_eq!(snapshot.remaining_percent, None);
    assert_eq!(snapshot.voltage_mv, Some(17_715));
    assert!(
        snapshot
            .source_issues
            .iter()
            .any(|issue| issue.source == "BatteryFullChargedCapacity"
                && issue.detail.contains("duplicate"))
    );
}

#[test]
fn missing_mismatched_and_duplicate_static_capabilities_never_imply_absolute_units() {
    for statics in [
        vec![],
        vec![static_data("PACK", 0x8000_0000)],
        vec![row("pack", &[])],
        vec![
            static_data("pack", 0x8000_0000),
            static_data("pack", 0x8000_0000),
        ],
    ] {
        let mut wmi = FakeWmi::standard();
        wmi.replies.insert("BatteryStaticData", statics);
        let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
        assert_eq!(snapshot.remaining_capacity_mwh, None);
        assert_eq!(snapshot.full_charge_capacity_mwh, None);
        assert_eq!(snapshot.design_capacity_mwh, None);
        assert_eq!(snapshot.remaining_percent, None);
        assert_eq!(snapshot.voltage_mv, Some(17_715));
        assert!(!snapshot.source_issues.is_empty());
    }
}

#[test]
fn relative_capacities_and_unknown_sentinels_are_not_mwh_or_percent() {
    let mut relative = FakeWmi::standard();
    relative
        .replies
        .insert("BatteryStaticData", vec![static_data("pack", 0xc000_0000)]);
    let snapshot = WindowsOsBattery::new(relative, Power).snapshot().unwrap();
    assert_eq!(snapshot.remaining_capacity_mwh, None);
    assert_eq!(snapshot.full_charge_capacity_mwh, None);
    assert_eq!(snapshot.design_capacity_mwh, None);
    assert_eq!(snapshot.remaining_percent, None);
    let mut unknown = FakeWmi::standard();
    unknown.replies.get_mut("BatteryStatus").unwrap()[0]
        .insert("RemainingCapacity".into(), WmiValue::U32(u32::MAX));
    unknown
        .replies
        .get_mut("BatteryFullChargedCapacity")
        .unwrap()[0]
        .insert("FullChargedCapacity".into(), WmiValue::U32(u32::MAX));
    unknown.replies.get_mut("BatteryStaticData").unwrap()[0]
        .insert("DesignedCapacity".into(), WmiValue::U32(u32::MAX));
    unknown.replies.get_mut("BatteryRuntime").unwrap()[0]
        .insert("EstimatedRuntime".into(), WmiValue::U32(u32::MAX));
    let snapshot = WindowsOsBattery::new(unknown, Power).snapshot().unwrap();
    assert_eq!(snapshot.remaining_capacity_mwh, None);
    assert_eq!(snapshot.full_charge_capacity_mwh, None);
    assert_eq!(snapshot.design_capacity_mwh, None);
    assert_eq!(snapshot.remaining_percent, None);
    assert_eq!(snapshot.estimated_runtime_min, None);
}

#[test]
fn optional_provider_failure_retains_status_and_records_original_failure() {
    let mut wmi = FakeWmi::standard();
    wmi.failure = Some("BatteryCycleCount");
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.voltage_mv, Some(17_715));
    assert_eq!(snapshot.remaining_percent, Some(50));
    assert_eq!(snapshot.cycle_count, None);
    assert!(snapshot.source_issues.iter().any(
        |issue| issue.source == "BatteryCycleCount" && issue.detail.contains("provider failed")
    ));
}

#[test]
fn absent_and_unmatched_optional_rows_are_reported() {
    let mut wmi = FakeWmi::standard();
    wmi.replies.remove("BatteryCycleCount");
    wmi.replies.insert(
        "BatteryRuntime",
        vec![row("other", &[("EstimatedRuntime", WmiValue::U32(60))])],
    );
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.estimated_runtime_min, None);
    assert!(
        snapshot
            .source_issues
            .iter()
            .any(|issue| issue.source == "BatteryCycleCount")
    );
    assert!(
        snapshot
            .source_issues
            .iter()
            .any(|issue| issue.source == "BatteryRuntime")
    );
}

#[test]
fn aggregate_percentage_is_separate_and_per_pack_arithmetic_is_widened() {
    let mut wmi = FakeWmi::standard();
    wmi.replies.get_mut("BatteryStatus").unwrap()[0]
        .insert("RemainingCapacity".into(), WmiValue::U32(3_000_000_000));
    wmi.replies.get_mut("BatteryFullChargedCapacity").unwrap()[0]
        .insert("FullChargedCapacity".into(), WmiValue::U32(4_000_000_000));
    let channel = WindowsOsBattery::new(wmi, Power);
    assert_eq!(channel.snapshot().unwrap().remaining_percent, Some(75));
    assert_eq!(
        channel.system_power_status().unwrap().battery_life_percent,
        Some(87)
    );
}

#[test]
fn required_provider_failure_is_not_empty_success() {
    let mut wmi = FakeWmi::standard();
    wmi.failure = Some("BatteryStatus");
    assert!(
        WindowsOsBattery::new(wmi, Power)
            .snapshot()
            .unwrap_err()
            .to_string()
            .contains("provider failed")
    );
}

#[test]
fn zero_baselines_are_unavailable_but_zero_remaining_is_valid() {
    let mut wmi = FakeWmi::standard();
    wmi.replies.get_mut("BatteryStatus").unwrap()[0]
        .insert("RemainingCapacity".into(), WmiValue::U32(0));
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.remaining_capacity_mwh, Some(0));
    assert_eq!(snapshot.remaining_percent, Some(0));

    let mut wmi = FakeWmi::standard();
    wmi.replies.get_mut("BatteryFullChargedCapacity").unwrap()[0]
        .insert("FullChargedCapacity".into(), WmiValue::U32(0));
    wmi.replies.get_mut("BatteryStaticData").unwrap()[0]
        .insert("DesignedCapacity".into(), WmiValue::U32(0));
    let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
    assert_eq!(snapshot.full_charge_capacity_mwh, None);
    assert_eq!(snapshot.design_capacity_mwh, None);
    assert_eq!(snapshot.remaining_percent, None);
    assert!(
        snapshot
            .source_issues
            .iter()
            .any(|issue| issue.source == "BatteryFullChargedCapacity"
                && issue.detail.contains("zero"))
    );
    assert!(
        snapshot
            .source_issues
            .iter()
            .any(|issue| issue.source == "BatteryStaticData" && issue.detail.contains("zero"))
    );
}

#[test]
fn capacity_ratio_uses_widened_arithmetic_without_clamping_or_health_thresholds() {
    for (full, design, expected) in [
        (3_000_000_000, 4_000_000_000, Some(75)),
        (4_000_000_000, 3_000_000_000, Some(133)),
        (65_535, 100, Some(u16::MAX)),
        (65_536, 100, None),
        (100_000, 0, None),
        (u32::MAX, 100_000, None),
    ] {
        let mut wmi = FakeWmi::standard();
        wmi.replies.get_mut("BatteryFullChargedCapacity").unwrap()[0]
            .insert("FullChargedCapacity".into(), WmiValue::U32(full));
        wmi.replies.get_mut("BatteryStaticData").unwrap()[0]
            .insert("DesignedCapacity".into(), WmiValue::U32(design));
        let snapshot = WindowsOsBattery::new(wmi, Power).snapshot().unwrap();
        assert_eq!(snapshot.capacity_ratio_percent, expected);
    }
}
