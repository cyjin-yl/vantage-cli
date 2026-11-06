//! Read-only decode of the dispatcher's `Policy_*` `PowerSlider` values.
//!
//! Every rule here is a reimplementation of code in
//! `LNVDispatcherService.exe`; the offsets and strings that prove each one are
//! recorded in `docs/evidence/re-its-policy-20261105.md`. Nothing in this
//! module writes: the only registry access is `PerformanceRegistryReader`,
//! whose `read_dword`/`read_string` go through `RegGetValueW` (`KEY_READ`).
//!
//! Three states are kept distinct so a report never becomes a guess:
//!
//! * **absent** — the installed dispatcher does not publish the value
//!   (`ChannelUnavailable`), so the field stays `None` with no issue;
//! * **unreadable** — the value exists but the read failed for another reason
//!   (permission, wrong registry type); the field stays `None` and an
//!   [`ItsPolicyIssue`] carries the error;
//! * **undecodable** — the value was read but does not match the packing the
//!   dispatcher implies; the raw source and the counts are both reported.
//!
//! The four curve tables are opaque firmware curves: this module reports their
//! slots and pairs verbatim and assigns no physical meaning to a position.

use lctrl_core::{
    Availability, ItsApMinStdFan, ItsEppTurbo, ItsPlxSetMode, ItsPolicy, ItsPolicyIssue,
    ItsSlotTable, ItsTurboTime, ItsTurboValue, LctrlError, Result,
};
use lctrl_hal::ItsPolicyControl;

use crate::performance_p0::{PerformanceRegistryReader, WindowsPerformanceP0};
use crate::power_overlay::PowerOverlayControl;

/// Slots in the dispatcher's `CPU_PowerBucket` array (`mov $0x30` bytes /
/// `mov $0xc` count at `0x1400714bd`).
pub const POWER_BUCKET_SLOTS: usize = 12;
/// Slots in the `EPP_Curve` / `CPUFreq_Curve` / `CPUFreq1_Curve` arrays
/// (`mov $0x10` count at `0x1400716a2` and its siblings).
pub const CURVE_SLOTS: usize = 16;

impl<R, O> ItsPolicyControl for WindowsPerformanceP0<R, O>
where
    R: PerformanceRegistryReader,
    O: PowerOverlayControl,
{
    fn its_policy(&self) -> Result<ItsPolicy> {
        Ok(read_policy(self.registry()))
    }
}

/// Assemble the decoded policy from the dispatcher's published values.
///
/// A machine whose `PowerSlider` key is missing entirely decodes to an empty
/// [`ItsPolicy`] with no issues: nothing is published, so nothing is claimed.
#[must_use]
pub fn read_policy<R>(registry: &R) -> ItsPolicy
where
    R: PerformanceRegistryReader + ?Sized,
{
    let mut issues = Vec::new();

    ItsPolicy {
        plx_set_mode: dword(registry, "Policy_PLxSetMode", &mut issues)
            .map(ItsPlxSetMode::from_raw),
        fan_speed_address: dword(registry, "Policy_FanSpeedAddress", &mut issues),
        ap_min_std_fan: read_ap_min_std_fan(registry, &mut issues),
        turbo_value: dword(registry, "Policy_TurboValue", &mut issues)
            .map(ItsTurboValue::from_raw),
        turbo_time: dword(registry, "Policy_TurboTime", &mut issues)
            .map(ItsTurboTime::from_raw),
        epp_turbo: dword(registry, "Policy_EPPTurbo", &mut issues).map(ItsEppTurbo::from_raw),
        power_bucket: read_table(registry, "Policy_PowerBucket", POWER_BUCKET_SLOTS, &mut issues),
        cppc: read_table(registry, "Policy_CPPC", CURVE_SLOTS, &mut issues),
        cfqc: read_table(registry, "Policy_CFQC", CURVE_SLOTS, &mut issues),
        cfqc1: read_table(registry, "Policy_CFQC1", CURVE_SLOTS, &mut issues),
        issues,
    }
}

/// Record the `perf.policy` capability row.
///
/// The probe is the same read the command performs: a decodable
/// `Policy_PLxSetMode` means the dispatcher publishes the policy set, so the
/// row is available; a missing value leaves the row unavailable with the read
/// error as its reason.
pub fn record_its_policy_capability<R>(
    capabilities: &mut lctrl_core::CapabilitySet,
    registry: &R,
) -> Result<()>
where
    R: PerformanceRegistryReader + ?Sized,
{
    // `record` yields `Option<Capability>`; the probe discards it and only
    // needs the write to succeed.
    match registry.read_dword("Policy_PLxSetMode") {
        Ok(raw) => capabilities.record(
            "perf.policy",
            Availability::Available,
            Some(format!(
                "dispatcher publishes Policy_PLxSetMode=0x{raw:08x}; decoded read-only"
            )),
        )?,
        Err(error) => capabilities.record(
            "perf.policy",
            Availability::Unavailable,
            Some(format!("PowerSlider policy values unreadable: {error}")),
        )?,
    };
    Ok(())
}

fn read_ap_min_std_fan<R>(
    registry: &R,
    issues: &mut Vec<ItsPolicyIssue>,
) -> Option<ItsApMinStdFan>
where
    R: PerformanceRegistryReader + ?Sized,
{
    // The dispatcher reads the enable value first and skips both thresholds
    // when that read fails (`jne 0x14006ff35`), and substitutes 50 for a
    // threshold it cannot read (`movl $0x32`).
    let enable = dword(registry, "Policy_APMinSTDFan", issues)?;
    let gpu = dword(registry, "Policy_APMinSTDFan_GPU", issues)
        .unwrap_or(ItsApMinStdFan::DEFAULT_THRESHOLD_PCT);
    let cpu = dword(registry, "Policy_APMinSTDFan_CPU", issues)
        .unwrap_or(ItsApMinStdFan::DEFAULT_THRESHOLD_PCT);
    Some(ItsApMinStdFan::from_raws(enable, gpu, cpu))
}

fn read_table<R>(
    registry: &R,
    name: &str,
    capacity: usize,
    issues: &mut Vec<ItsPolicyIssue>,
) -> Option<ItsSlotTable>
where
    R: PerformanceRegistryReader + ?Sized,
{
    let source = text(registry, name, issues)?;
    match ItsSlotTable::parse(&source, capacity) {
        Ok(table) => Some(table),
        Err(detail) => {
            issues.push(ItsPolicyIssue::unparsed(name, source, detail));
            None
        }
    }
}

fn dword<R>(registry: &R, name: &str, issues: &mut Vec<ItsPolicyIssue>) -> Option<u32>
where
    R: PerformanceRegistryReader + ?Sized,
{
    match registry.read_dword(name) {
        Ok(value) => Some(value),
        // The dispatcher publishes policy values as a set; one that is absent
        // simply is not part of the set on this machine.
        Err(LctrlError::ChannelUnavailable { .. }) => None,
        Err(error) => {
            issues.push(ItsPolicyIssue::unreadable(name, error.to_string()));
            None
        }
    }
}

fn text<R>(registry: &R, name: &str, issues: &mut Vec<ItsPolicyIssue>) -> Option<String>
where
    R: PerformanceRegistryReader + ?Sized,
{
    match registry.read_string(name) {
        Ok(value) => Some(value),
        Err(LctrlError::ChannelUnavailable { .. }) => None,
        Err(error) => {
            issues.push(ItsPolicyIssue::unreadable(name, error.to_string()));
            None
        }
    }
}
