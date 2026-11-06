//! Read-only processor identity, rated clocks and the derived current clock.
//!
//! Every value here is something the operating system reports; nothing is
//! inferred from a product name. The live source inventory, the rejected
//! sources and the derivation are recorded in
//! `docs/evidence/re-cpu-20261105.md`.
//!
//! Two rules keep the payload honest on a turbo-capable part:
//!
//! * a *rated* clock (`firmware_*`) is a firmware-declared number and is never
//!   presented as the frequency the processor is running at;
//! * the current clock is always **derived** from the documented
//!   `% Processor Performance` counter and its rated reference, and stays
//!   absent when that counter produced no sample instead of falling back to a
//!   rated value.

use serde::Serialize;

use crate::Availability;

/// Where the derived current clock came from.
///
/// A single variant today: the derivation is fixed by evidence
/// (`docs/evidence/re-cpu-20261105.md` §4) rather than selected per platform,
/// and naming it in the payload lets a reader tell a derived value from a
/// measured one without reading the implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuClockSource {
    /// `rated_clock_mhz * percent_processor_performance / 100`, both taken
    /// from `Win32_PerfFormattedData_Counters_ProcessorInformation`.
    ProcessorPerformanceCounter,
}

/// One clock domain of a hybrid (or otherwise mixed) processor.
///
/// The groups are formed from the rated reference the OS performance counter
/// reports per logical processor, not from a core-type name: neither the
/// counter nor `Win32_Processor` labels a logical processor as P-core or
/// E-core.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CpuClockCluster {
    /// Rated clock the counter normalizes its percentage against, MHz.
    pub rated_clock_mhz: u32,
    /// Logical processor indices the counter reports with this rated clock.
    pub logical_processors: Vec<u32>,
    /// Highest `% Processor Performance` observed in the cluster, as reported
    /// by the counter (values above 100 indicate turbo).
    pub percent_processor_performance: Option<u32>,
    /// Derived from [`derive_current_clock_mhz`]; `None` when the cluster's
    /// percentage was zero (no non-idle sample, not 0 MHz).
    pub current_clock_mhz: Option<u32>,
}

/// Derive a current clock from a rated clock and the documented
/// `% Processor Performance` counter.
///
/// The counter reports the average processor performance relative to the
/// rated frequency, so a processor at `135 %` of a 1900 MHz rating runs at
/// 2565 MHz. A zero percentage is not 0 MHz: a logical processor that had no
/// non-idle work in the sampling interval carries no frequency information, so
/// the derivation refuses to produce a value for it.
#[must_use]
pub fn derive_current_clock_mhz(
    rated_clock_mhz: u32,
    percent_processor_performance: u32,
) -> Option<u32> {
    if percent_processor_performance == 0 {
        return None;
    }
    let product = u64::from(rated_clock_mhz).checked_mul(u64::from(percent_processor_performance))?;
    u32::try_from(product / 100).ok()
}

/// Processor identity, firmware-declared rated clocks and the derived current
/// clock for the machine.
///
/// Absent provider modules stay `None`; a source that cannot be read is never
/// replaced by a plausible constant. `current_clock_availability` states
/// whether the derived clock exists and, when it does not, why.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CpuStatus {
    /// `Win32_Processor.Name`, otherwise the registry `ProcessorNameString`.
    pub name: Option<String>,
    /// Registry `VendorIdentifier`, otherwise `Win32_Processor.Manufacturer`.
    pub vendor: Option<String>,
    /// Registry `Identifier`, the CPUID family/model/stepping text.
    pub identifier: Option<String>,
    /// `Win32_Processor.NumberOfCores`.
    pub cores: Option<u32>,
    /// `Win32_Processor.NumberOfLogicalProcessors`.
    pub threads: Option<u32>,
    /// `Win32_Processor.MaxClockSpeed`: the firmware-declared rated clock in
    /// MHz. On a turbo-capable part this is the rated (base) clock, not the
    /// turbo ceiling, so it is not a maximum frequency and never a current one.
    pub firmware_max_clock_mhz: Option<u32>,
    /// `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0` "`~MHz`": the
    /// firmware's approximate processor frequency in MHz. Also a rated value,
    /// kept separate because it does not agree with
    /// `firmware_max_clock_mhz` on every SKU.
    pub firmware_approximate_clock_mhz: Option<u32>,
    /// `Win32_Processor.L2CacheSize`, KB.
    pub l2_cache_kb: Option<u32>,
    /// `Win32_Processor.L3CacheSize`, KB.
    pub l3_cache_kb: Option<u32>,
    /// Highest derived current clock across [`Self::clusters`].
    ///
    /// Always a derived value (`rated × percent / 100`); it is `None` when no
    /// cluster produced a positive sample. It is never backfilled with a rated
    /// clock.
    pub current_clock_mhz: Option<u32>,
    /// Present exactly when [`Self::current_clock_mhz`] is present.
    pub current_clock_source: Option<CpuClockSource>,
    /// Whether the derived current clock exists, and if not, why not.
    pub current_clock_availability: Availability,
    /// Failure or zero-sample reason when `current_clock_availability` is not
    /// [`Availability::Available`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_clock_detail: Option<String>,
    /// Per clock domain observations, ordered by logical processor index.
    pub clusters: Vec<CpuClockCluster>,
}

