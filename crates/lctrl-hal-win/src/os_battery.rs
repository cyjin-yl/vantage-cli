//! Public Windows battery observations, associated only by exact InstanceName.
//! Aggregate system power status is deliberately separate from per-pack data.

use crate::{WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};
use lctrl_core::{BatterySourceIssue, LctrlError, OsBatterySnapshot, Result, SystemPowerStatus};

const SYSTEM_BATTERY: u32 = 0x8000_0000;
const CAPACITY_RELATIVE: u32 = 0x4000_0000;
const SHORT_TERM: u32 = 0x2000_0000;

/// A source of the operating system's aggregate power status.
pub trait SystemPowerStatusReader: Send + Sync {
    fn read(&self) -> Result<SystemPowerStatus>;
}

/// Reads battery state through the public Windows battery classes.
pub trait OsBatteryChannel: Send + Sync {
    fn snapshot(&self) -> Result<OsBatterySnapshot>;
    fn system_power_status(&self) -> Result<SystemPowerStatus>;
}

/// The Windows implementation of the public battery channel.
#[derive(Clone, Copy, Debug)]
pub struct WindowsOsBattery<W, P> {
    wmi: W,
    power: P,
}

impl<W, P> WindowsOsBattery<W, P> {
    pub const fn new(wmi: W, power: P) -> Self {
        Self { wmi, power }
    }
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
    pub const fn power(&self) -> &P {
        &self.power
    }
}

fn issue(issues: &mut Vec<BatterySourceIssue>, source: &'static str, detail: impl Into<String>) {
    issues.push(BatterySourceIssue {
        source,
        detail: detail.into(),
    });
}

fn rows(
    wmi: &impl WmiTransport,
    class: &'static str,
    properties: &str,
    issues: &mut Vec<BatterySourceIssue>,
) -> Vec<WmiObject> {
    match wmi.query(ROOT_WMI, &format!("SELECT {properties} FROM {class}")) {
        Ok(rows) => {
            if rows.is_empty() {
                issue(issues, class, "class exposed no instances");
            }
            for row in &rows {
                if identity(row).is_none() {
                    issue(issues, class, "instance has missing or empty InstanceName");
                }
            }
            rows
        }
        Err(error) => {
            issue(issues, class, error.to_string());
            Vec::new()
        }
    }
}

fn identity(row: &WmiObject) -> Option<&str> {
    match row.get("InstanceName") {
        Some(WmiValue::String(name)) if !name.trim().is_empty() => Some(name),
        _ => None,
    }
}

fn associated<'a>(
    rows: &'a [WmiObject],
    name: &str,
    class: &'static str,
    issues: &mut Vec<BatterySourceIssue>,
) -> Option<&'a WmiObject> {
    let mut matches = rows.iter().filter(|row| identity(row) == Some(name));
    let first = matches.next();
    if matches.next().is_some() {
        issue(
            issues,
            class,
            "duplicate InstanceName; association rejected",
        );
        None
    } else {
        if first.is_none() {
            issue(issues, class, "no matching InstanceName");
        }
        first
    }
}

impl<W: WmiTransport, P: SystemPowerStatusReader> OsBatteryChannel for WindowsOsBattery<W, P> {
    fn snapshot(&self) -> Result<OsBatterySnapshot> {
        let mut issues = Vec::new();
        // Status is the required source: propagate its provider failure unchanged.
        let statuses = self.wmi.query(ROOT_WMI, "SELECT InstanceName,Active,RemainingCapacity,Voltage,PowerOnline,Charging,Discharging,Critical FROM BatteryStatus")?;
        let statics = rows(
            &self.wmi,
            "BatteryStaticData",
            "InstanceName,Capabilities,DesignedCapacity",
            &mut issues,
        );
        let mut selected = None;
        for status in &statuses {
            if bool_field(status, "Active") != Some(true) {
                continue;
            }
            let Some(name) = identity(status) else {
                issue(
                    &mut issues,
                    "BatteryStatus",
                    "active instance has missing or empty InstanceName",
                );
                continue;
            };
            if statuses
                .iter()
                .filter(|row| identity(row) == Some(name))
                .count()
                != 1
            {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "BatteryStatus has duplicate InstanceName; source issues: {issues:?}"
                    ),
                });
            }
            let static_row = associated(&statics, name, "BatteryStaticData", &mut issues);
            let capabilities = static_row.and_then(|row| u32_field(row, "Capabilities"));
            if capabilities.is_some_and(|caps| caps & SYSTEM_BATTERY == 0 || caps & SHORT_TERM != 0)
            {
                continue;
            }
            if selected.is_some() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "multiple active battery identities; no unique system battery; source issues: {issues:?}"
                    ),
                });
            }
            selected = Some((status, name, static_row, capabilities));
        }
        let (status, name, static_row, capabilities) = selected.ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("no uniquely identified active non-short-term system battery; source issues: {issues:?}"),
        })?;
        if capabilities.is_none() {
            issue(
                &mut issues,
                "BatteryStaticData",
                "missing capabilities; system-battery classification and absolute capacity units unavailable",
            );
        }
        let absolute = capabilities.is_some_and(|caps| caps & CAPACITY_RELATIVE == 0);
        if !absolute && capabilities.is_some() {
            issue(
                &mut issues,
                "BatteryStaticData",
                "relative capacity units; mWh unavailable",
            );
        }
        let full_rows = rows(
            &self.wmi,
            "BatteryFullChargedCapacity",
            "InstanceName,FullChargedCapacity",
            &mut issues,
        );
        let cycles = rows(
            &self.wmi,
            "BatteryCycleCount",
            "InstanceName,CycleCount",
            &mut issues,
        );
        let runtimes = rows(
            &self.wmi,
            "BatteryRuntime",
            "InstanceName,EstimatedRuntime",
            &mut issues,
        );
        let full = associated(&full_rows, name, "BatteryFullChargedCapacity", &mut issues);
        let cycle = associated(&cycles, name, "BatteryCycleCount", &mut issues);
        let runtime = associated(&runtimes, name, "BatteryRuntime", &mut issues);
        let remaining = absolute
            .then(|| capacity(status, "RemainingCapacity"))
            .flatten();
        let full_capacity = absolute
            .then(|| {
                full.and_then(|row| {
                    positive_capacity(
                        row,
                        "FullChargedCapacity",
                        "BatteryFullChargedCapacity",
                        &mut issues,
                    )
                })
            })
            .flatten();
        let design_capacity = absolute
            .then(|| {
                static_row.and_then(|row| {
                    positive_capacity(row, "DesignedCapacity", "BatteryStaticData", &mut issues)
                })
            })
            .flatten();
        // Per-pack percentage is floor(100 * remaining / full), using widened
        // arithmetic, only for supported absolute capacities and a valid ratio.
        // GetSystemPowerStatus is aggregate and must never supply this field.
        let percent = remaining.zip(full_capacity).and_then(|(remaining, full)| {
            if full == 0 || remaining > full {
                return None;
            }
            u16::try_from(u64::from(remaining) * 100 / u64::from(full)).ok()
        });
        // This is a capacity ratio, not a health classification. Ratios over
        // 100 remain observable when representable, without thresholds/clamps.
        let capacity_ratio_percent =
            full_capacity
                .zip(design_capacity)
                .and_then(|(full, design)| {
                    u16::try_from(u64::from(full) * 100 / u64::from(design)).ok()
                });
        Ok(OsBatterySnapshot {
            remaining_capacity_mwh: remaining,
            full_charge_capacity_mwh: full_capacity,
            design_capacity_mwh: design_capacity,
            voltage_mv: u32_field(status, "Voltage")
                .filter(|value| *value != u32::MAX)
                .and_then(|value| u16::try_from(value).ok()),
            remaining_percent: percent,
            capacity_ratio_percent,
            cycle_count: cycle
                .and_then(|row| u32_field(row, "CycleCount"))
                .and_then(|value| u16::try_from(value).ok()),
            estimated_runtime_min: runtime
                .and_then(|row| u32_field(row, "EstimatedRuntime"))
                .filter(|value| *value != u32::MAX)
                .and_then(|value| u16::try_from(value / 60).ok()),
            power_online: bool_field(status, "PowerOnline"),
            charging: bool_field(status, "Charging"),
            discharging: bool_field(status, "Discharging"),
            critical: bool_field(status, "Critical"),
            source_issues: issues,
        })
    }

    fn system_power_status(&self) -> Result<SystemPowerStatus> {
        self.power.read()
    }
}

fn capacity(row: &WmiObject, name: &str) -> Option<u32> {
    u32_field(row, name).filter(|value| *value != u32::MAX)
}

fn positive_capacity(
    row: &WmiObject,
    name: &str,
    source: &'static str,
    issues: &mut Vec<BatterySourceIssue>,
) -> Option<u32> {
    match capacity(row, name) {
        Some(value) if value > 0 => Some(value),
        _ => {
            issue(
                issues,
                source,
                format!("{name} is missing, unknown, or zero; capacity unavailable"),
            );
            None
        }
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    match object.get(name) {
        Some(WmiValue::U32(value)) => Some(*value),
        Some(WmiValue::U16(value)) => Some(u32::from(*value)),
        Some(WmiValue::I32(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::I16(value)) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn bool_field(object: &WmiObject, name: &str) -> Option<bool> {
    match object.get(name) {
        Some(WmiValue::Bool(value)) => Some(*value),
        _ => None,
    }
}
