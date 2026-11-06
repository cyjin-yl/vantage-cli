//! Read-only processor identity and clock observation over public interfaces.
//!
//! Three documented sources are combined (`docs/evidence/re-cpu-20261105.md`):
//!
//! * `root\cimv2` `Win32_Processor` for topology and the firmware-declared
//!   rated clock;
//! * `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0` for the identity
//!   strings and the firmware's approximate frequency;
//! * `Win32_PerfFormattedData_Counters_ProcessorInformation` for the only
//!   frequency observation that moves with turbo.
//!
//! Nothing here writes hardware or firmware, and a source that cannot be read
//! is left absent rather than replaced by a plausible constant.

use std::collections::BTreeMap;

use lctrl_core::{
    Availability, CpuClockCluster, CpuClockSource, CpuStatus, LctrlError, Result,
    derive_current_clock_mhz,
};

use crate::{WmiObject, WmiTransport, WmiValue};

const ROOT_CIMV2: &str = "ROOT\\CIMV2";

const PROCESSOR_QUERY: &str = "SELECT Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, L2CacheSize, L3CacheSize FROM Win32_Processor";

/// The class whose `ProcessorFrequency` is the rated reference and whose
/// `PercentProcessorPerformance` is the relative performance counter.
const CLOCK_QUERY: &str = "SELECT Name, ProcessorFrequency, PercentProcessorPerformance FROM Win32_PerfFormattedData_Counters_ProcessorInformation";

/// Registry value names of the firmware-published processor block under
/// `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`.
pub const PROCESSOR_NAME_VALUE: &str = "ProcessorNameString";
pub const VENDOR_IDENTIFIER_VALUE: &str = "VendorIdentifier";
pub const IDENTIFIER_VALUE: &str = "Identifier";
/// The firmware's approximate frequency in MHz (`REG_DWORD`). A rated value.
pub const APPROXIMATE_CLOCK_VALUE: &str = "~MHz";

/// Reads the processor block the firmware publishes under
/// `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`.
///
/// The trait keeps the registry transport out of the observation logic so the
/// grouping and derivation can be exercised without a Windows registry.
pub trait CpuRegistryReader: Send + Sync {
    /// Read a `REG_SZ` value.
    fn read_string(&self, value: &str) -> Result<String>;

    /// Read a `REG_DWORD` value.
    fn read_dword(&self, value: &str) -> Result<u32>;
}

/// Read processor identity, rated clocks and the derived current clock.
///
/// Identity is taken from `Win32_Processor` first and from the firmware block
/// second, because the WMI strings carry the marketing name while the registry
/// carries the CPUID family/model/stepping text the WMI class does not.
///
/// A missing performance-counter class leaves the current clock absent with
/// [`Availability::Unavailable`]; it never falls back to a rated clock. When
/// neither identity source can be read the whole call fails instead of
/// returning an empty-looking success.
pub fn cpu_status<W, R>(wmi: &W, registry: &R) -> Result<CpuStatus>
where
    W: WmiTransport,
    R: CpuRegistryReader,
{
    // The identity query is consumed here so the failure can be reported as
    // the command's own error without holding a borrow of the object list.
    let mut identity_failure = None;
    let processor = match wmi.query(ROOT_CIMV2, PROCESSOR_QUERY) {
        Ok(objects) => objects.into_iter().next(),
        Err(error) => {
            identity_failure = Some(error);
            None
        }
    };
    let registry_name = registry.read_string(PROCESSOR_NAME_VALUE).ok();
    let registry_vendor = registry.read_string(VENDOR_IDENTIFIER_VALUE).ok();
    let registry_identifier = registry.read_string(IDENTIFIER_VALUE).ok();
    let registry_clock = registry.read_dword(APPROXIMATE_CLOCK_VALUE).ok();

    if processor.is_none()
        && registry_name.is_none()
        && registry_vendor.is_none()
        && registry_identifier.is_none()
        && registry_clock.is_none()
    {
        return Err(identity_failure.unwrap_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("{PROCESSOR_NAME_VALUE:?} registry value and Win32_Processor"),
        }));
    }

    let processor = processor.as_ref();
    let mut status = CpuStatus {
        name: processor
            .and_then(|object| string_field(object, "Name"))
            .or(registry_name),
        vendor: registry_vendor.or_else(|| {
            processor.and_then(|object| string_field(object, "Manufacturer"))
        }),
        identifier: registry_identifier,
        cores: processor.and_then(|object| u32_field(object, "NumberOfCores")),
        threads: processor.and_then(|object| u32_field(object, "NumberOfLogicalProcessors")),
        firmware_max_clock_mhz: processor.and_then(|object| u32_field(object, "MaxClockSpeed")),
        firmware_approximate_clock_mhz: registry_clock,
        l2_cache_kb: processor.and_then(|object| u32_field(object, "L2CacheSize")),
        l3_cache_kb: processor.and_then(|object| u32_field(object, "L3CacheSize")),
        current_clock_mhz: None,
        current_clock_source: None,
        current_clock_availability: Availability::Unavailable,
        current_clock_detail: None,
        clusters: Vec::new(),
    };

    let (clusters, availability, detail) = match wmi.query(ROOT_CIMV2, CLOCK_QUERY) {
        Ok(rows) => clock_clusters(&rows),
        Err(error) => (
            Vec::new(),
            Availability::Unavailable,
            Some(format!(
                "processor performance counter query failed: {error}"
            )),
        ),
    };
    let current = clusters
        .iter()
        .filter_map(|cluster| cluster.current_clock_mhz)
        .max();
    status.current_clock_mhz = current;
    status.current_clock_source = current.map(|_| CpuClockSource::ProcessorPerformanceCounter);
    status.current_clock_availability = availability;
    status.current_clock_detail = match availability {
        Availability::Available => None,
        Availability::Limited | Availability::Unavailable => detail,
    };
    status.clusters = clusters;
    Ok(status)
}

/// Group the counter rows into clock domains and derive each domain's current
/// clock.
///
/// Aggregate rows (`_Total`, `0,_Total`) carry no per-logical-processor
/// identity and are excluded; the remaining rows must be named `<group>,<index>`
/// with both fields numeric.
fn clock_clusters(rows: &[WmiObject]) -> (Vec<CpuClockCluster>, Availability, Option<String>) {
    let mut grouped: BTreeMap<u32, (Vec<u32>, Option<u32>)> = BTreeMap::new();
    let mut per_processor_rows = 0_usize;
    for row in rows {
        let Some(index) = row
            .get("Name")
            .and_then(|value| match value {
                WmiValue::String(name) => counter_logical_index(name),
                _ => None,
            })
        else {
            continue;
        };
        per_processor_rows += 1;
        let Some(rated) = u32_field(row, "ProcessorFrequency") else {
            continue;
        };
        let percent = u32_field(row, "PercentProcessorPerformance");
        let entry = grouped.entry(rated).or_default();
        entry.0.push(index);
        entry.1 = match (entry.1, percent) {
            (Some(previous), Some(current)) => Some(previous.max(current)),
            (previous, current) => previous.or(current),
        };
    }

    if grouped.is_empty() {
        let detail = if per_processor_rows == 0 {
            format!(
                "{CLOCK_QUERY} returned no per-logical-processor row (only aggregate rows or none)"
            )
        } else {
            "processor performance counter rows carried no ProcessorFrequency".to_owned()
        };
        return (Vec::new(), Availability::Unavailable, Some(detail));
    }

    let mut clusters: Vec<CpuClockCluster> = grouped
        .into_iter()
        .map(|(rated_clock_mhz, (mut logical_processors, percent))| {
            logical_processors.sort_unstable();
            logical_processors.dedup();
            CpuClockCluster {
                rated_clock_mhz,
                logical_processors,
                percent_processor_performance: percent,
                current_clock_mhz: percent
                    .and_then(|percent| derive_current_clock_mhz(rated_clock_mhz, percent)),
            }
        })
        .collect();
    clusters.sort_by_key(|cluster| cluster.logical_processors.first().copied());

    if clusters
        .iter()
        .any(|cluster| cluster.current_clock_mhz.is_some())
    {
        (clusters, Availability::Available, None)
    } else {
        (
            clusters,
            Availability::Limited,
            Some(
                "every logical processor reported 0 percent processor performance; the counter \
                 carries no frequency for an idle sampling interval"
                    .to_owned(),
            ),
        )
    }
}

/// Parse a performance-counter instance name of the form `<group>,<index>`.
///
/// Aggregate instances (`_Total`, `0,_Total`) do not match and return `None`.
fn counter_logical_index(name: &str) -> Option<u32> {
    let mut fields = name.split(',');
    let group = fields.next()?;
    let index = fields.next()?;
    if fields.next().is_some() {
        return None;
    }
    if group.is_empty() || !group.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    index.parse().ok()
}

fn string_field(object: &WmiObject, name: &str) -> Option<String> {
    match object.get(name) {
        Some(WmiValue::String(value)) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn u32_field(object: &WmiObject, name: &str) -> Option<u32> {
    match object.get(name) {
        Some(WmiValue::U64(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::U32(value)) => Some(*value),
        Some(WmiValue::U16(value)) => Some(u32::from(*value)),
        Some(WmiValue::U8(value)) => Some(u32::from(*value)),
        Some(WmiValue::I64(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::I32(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::I16(value)) => u32::try_from(*value).ok(),
        Some(WmiValue::I8(value)) => u32::try_from(*value).ok(),
        _ => None,
    }
}
