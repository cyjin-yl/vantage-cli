//! Linux process telemetry backed by `/proc`.
//!
//! The inventory path is complete: two CPU samples of every runnable process
//! are taken `SAMPLE_INTERVAL` apart, ordered by interval CPU time and
//! capped by the caller's limit. Per-process mutation is deliberately not
//! implemented. Nice-level and affinity changes are real on Linux, but the
//! `ProcessState` model carries a hard working-set quota that has no Linux
//! equivalent, and `top` is the only consumer that does not need it. Rather
//! than emit a fabricated quota, the mutating operations report
//! [`LctrlError::Unsupported`] until a policy that is honest about that
//! boundary is designed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, ProcessInventory, ProcessInventoryEntry, ProcessPolicy,
    ProcessSnapshot, ProcessState, ProcessTarget, Result, validate_process_top_limit,
};
use lctrl_hal::ProcessControl;

/// How long to leave between the two CPU samples. Shorter than the Windows
/// telemetry's 186 ms interval so `perf top` stays responsive while still
/// producing a real (nonzero) interval reading on an idle system.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(120);
const PROC_ROOT: &str = "/proc";
/// The kernel reports CPU time in `USER_HZ` jiffies, always 100 on x86
/// userland, so one jiffy is exactly 10 ms = 100,000 ticks of 100 ns.
const JIFFY_TO_100NS: u64 = 100_000;

/// A `ProcessControl` that reads `/proc` from the supplied root, so tests and
/// a mounted sysroot are exercised the same way as the host.
pub struct LinuxProcessController {
    proc_root: PathBuf,
}

impl LinuxProcessController {
    #[must_use]
    pub fn new() -> Self {
        Self {
            proc_root: PathBuf::from(PROC_ROOT),
        }
    }

    /// Bind the controller to a custom `/proc` root.
    #[must_use]
    pub fn with_root(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
        }
    }
}

impl Default for LinuxProcessController {
    fn default() -> Self {
        Self::new()
    }
}

/// One `/proc/<pid>` observation. `creation` distinguishes a reused PID, and
/// `cpu_jiffies` is the running total the interval is derived from.
struct ProcSample {
    name: String,
    image_path: Option<String>,
    creation_ticks: u64,
    cpu_jiffies: u64,
    rss_bytes: u64,
}

/// Read `/proc/<pid>/stat`, `/proc/<pid>/comm` and `/proc/<pid>/exe`. The stat
/// file's `(command)` field is split at the last `)` so names containing spaces
/// or parentheses are not mis-parsed.
fn sample(proc_root: &Path, pid: u32) -> io::Result<ProcSample> {
    let stat = fs::read_to_string(proc_root.join(pid.to_string()).join("stat"))?;
    let close = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "stat missing ')'"))?;
    // Everything after `) ` is a fixed-width field list starting at state(3).
    let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    // Field numbers per proc(5): index here is field-3, so utime(14)=idx11,
    // stime(15)=idx12, starttime(22)=idx19, rss(24)=idx21.
    let get = |i: usize| {
        fields
            .get(i)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "short stat"))?
            .parse::<u64>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    };
    let utime = get(11)?;
    let stime = get(12)?;
    let starttime = get(19)?;
    let rss_pages = get(21)?;
    let name = stat
        .get(stat.find('(').map(|o| o + 1).unwrap_or(0)..close)
        .unwrap_or("")
        .to_owned();
    let image_path = fs::read_link(proc_root.join(pid.to_string()).join("exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let page_size: u64 = 4096;
    Ok(ProcSample {
        name: if name.is_empty() {
            format!("[kernel {pid}]")
        } else {
            name
        },
        image_path,
        creation_ticks: starttime,
        cpu_jiffies: utime.saturating_add(stime),
        rss_bytes: rss_pages.saturating_mul(page_size),
    })
}

fn list_pids(proc_root: &Path) -> Vec<u32> {
    let mut pids: Vec<u32> = fs::read_dir(proc_root)
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()))
                .collect()
        })
        .unwrap_or_default();
    pids.sort_unstable();
    pids
}

/// Convert kernel `starttime` ticks (in `USER_HZ` jiffies since boot) into the
/// opaque 100-ns identity field. It is only required to be nonzero, stable per
/// instance and unique to the process — matching the identity the PID reuses.
fn identity_100ns(creation_ticks: u64) -> u64 {
    creation_ticks.saturating_mul(JIFFY_TO_100NS)
}

fn entry(
    pid: u32,
    now: &ProcSample,
    before: &ProcSample,
    wall_100ns: u64,
) -> ProcessInventoryEntry {
    let interval_cpu = now
        .cpu_jiffies
        .saturating_sub(before.cpu_jiffies)
        .saturating_mul(JIFFY_TO_100NS);
    ProcessInventoryEntry {
        identity: lctrl_core::ProcessIdentity {
            pid,
            creation_time_100ns: identity_100ns(now.creation_ticks),
        },
        image_name: now.name.clone(),
        image_path: now.image_path.clone(),
        total_cpu_time_100ns: now.cpu_jiffies.saturating_mul(JIFFY_TO_100NS),
        interval_cpu_time_100ns: interval_cpu,
        interval_wall_time_100ns: wall_100ns,
        working_set_bytes: Some(now.rss_bytes as usize),
    }
}

impl ProcessControl for LinuxProcessController {
    fn top_processes(&self, limit: usize) -> Result<ProcessInventory> {
        validate_process_top_limit(limit)?;
        let first: Vec<(u32, ProcSample)> = list_pids(&self.proc_root)
            .iter()
            .filter_map(|&pid| sample(&self.proc_root, pid).ok().map(|s| (pid, s)))
            .collect();
        let start = Instant::now();
        sleep(SAMPLE_INTERVAL);
        let wall = start.elapsed();
        // The real elapsed wall time, not the requested interval, so the PPM
        // figure is honest when the sleep is short or overran.
        let wall_100ns = (wall.as_nanos() / 100) as u64;
        let mut skipped = 0usize;
        let mut entries: Vec<ProcessInventoryEntry> = Vec::new();
        for (pid, before) in &first {
            match sample(&self.proc_root, *pid) {
                Ok(now) if now.creation_ticks == before.creation_ticks => {
                    entries.push(entry(*pid, &now, before, wall_100ns))
                }
                // Process exited or the PID was reused between samples.
                _ => skipped += 1,
            }
        }
        // Processes that only appeared in the second sample have no baseline;
        // count them as skipped rather than fabricating a zero interval.
        let second_pids: std::collections::BTreeSet<u32> =
            list_pids(&self.proc_root).into_iter().collect();
        let first_pids: std::collections::BTreeSet<u32> =
            first.iter().map(|(pid, _)| *pid).collect();
        skipped += second_pids.difference(&first_pids).count();
        entries.sort_by(|a, b| {
            b.interval_cpu_time_100ns
                .cmp(&a.interval_cpu_time_100ns)
                .then_with(|| a.identity.pid.cmp(&b.identity.pid))
        });
        entries.truncate(limit);
        Ok(ProcessInventory::new(entries, skipped))
    }

    fn resolve_target(&self, pid: u32) -> Result<ProcessTarget> {
        let now = sample(&self.proc_root, pid).map_err(|_| LctrlError::ChannelUnavailable {
            channel: format!("process {pid} is not readable"),
        })?;
        let identity = lctrl_core::ProcessIdentity {
            pid,
            creation_time_100ns: identity_100ns(now.creation_ticks),
        };
        ProcessTarget::new(identity, now.name)
    }

    fn process_snapshot(&self, target: &ProcessTarget) -> Result<ProcessSnapshot> {
        let identity = target.identity();
        let now =
            sample(&self.proc_root, identity.pid).map_err(|_| LctrlError::ChannelUnavailable {
                channel: format!("process {} is not readable", identity.pid),
            })?;
        if !target.accepts_image_name(&now.name) {
            return Err(LctrlError::InvalidArgument {
                detail: format!(
                    "pid {} now runs {} which does not match authorized {}",
                    identity.pid,
                    now.name,
                    target.image_name()
                ),
            });
        }
        Err(LctrlError::Unsupported {
            feature: "process working-set quota is a Windows concept; the Linux priority/affinity policy cannot be reported without one".into(),
        })
    }

    fn boost_process(
        &self,
        _target: &ProcessTarget,
        _policy: ProcessPolicy,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        Err(LctrlError::Unsupported {
            feature: "process working-set quota is a Windows concept; Linux boost is not implemented without an honest quota model".into(),
        })
    }

    fn throttle_process(
        &self,
        _target: &ProcessTarget,
        _policy: ProcessPolicy,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        Err(LctrlError::Unsupported {
            feature: "process working-set quota is a Windows concept; Linux throttle is not implemented without an honest quota model".into(),
        })
    }

    fn restore_process(
        &self,
        _target: &ProcessTarget,
        _snapshot: &ProcessSnapshot,
        _apply: ApplyMode,
    ) -> Result<ChangeReport<ProcessState>> {
        Err(LctrlError::Unsupported {
            feature: "process working-set quota is a Windows concept; Linux restore is not implemented without an honest quota model".into(),
        })
    }
}
