use std::fmt;

use serde::Serialize;

use crate::{LctrlError, Result};

pub const DEFAULT_PROCESS_TOP_LIMIT: usize = 10;
pub const MAX_PROCESS_TOP_LIMIT: usize = 100;

/// A process identity that cannot be silently transferred to a reused PID.
/// `creation_time_100ns` is the Windows FILETIME value, in 100 ns units since
/// 1601-01-01 UTC.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub creation_time_100ns: u64,
}

impl ProcessIdentity {
    pub fn new(pid: u32, creation_time_100ns: u64) -> Result<Self> {
        let identity = Self {
            pid,
            creation_time_100ns,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<()> {
        if self.pid == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: "process mutations require a nonzero PID".into(),
            });
        }
        if self.creation_time_100ns == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("process {} has no exact creation-time identity", self.pid),
            });
        }
        Ok(())
    }
}

impl fmt::Display for ProcessIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "pid={} creation_time_100ns={}",
            self.pid, self.creation_time_100ns
        )
    }
}

/// Explicit user authorization for one named process instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessTarget {
    identity: ProcessIdentity,
    image_name: String,
}

impl ProcessTarget {
    pub fn new(identity: ProcessIdentity, image_name: impl Into<String>) -> Result<Self> {
        identity.validate()?;
        let image_name = image_name.into();
        if image_name.trim().is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "authorized process image name must not be empty".into(),
            });
        }
        if image_name.contains('\0') {
            return Err(LctrlError::InvalidArgument {
                detail: "authorized process image name must not contain NUL".into(),
            });
        }
        Ok(Self {
            identity,
            image_name,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }

    #[must_use]
    pub fn image_name(&self) -> &str {
        &self.image_name
    }

    /// Windows image names are accepted verbatim or with ASCII case changes.
    /// The PID and creation time still have to match exactly.
    #[must_use]
    pub fn accepts_image_name(&self, actual: &str) -> bool {
        self.image_name == actual || self.image_name.eq_ignore_ascii_case(actual)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessInventoryEntry {
    pub identity: ProcessIdentity,
    pub image_name: String,
    /// `None` means the process image path was not available with the caller's
    /// permissions. The ToolHelp image name remains available.
    pub image_path: Option<String>,
    pub total_cpu_time_100ns: u64,
    pub interval_cpu_time_100ns: u64,
    pub interval_wall_time_100ns: u64,
    pub working_set_bytes: Option<usize>,
}

impl ProcessInventoryEntry {
    /// CPU utilization in parts per million, where 1,000,000 is one completely
    /// busy logical processor. Multi-threaded processes can exceed that value.
    #[must_use]
    pub fn cpu_utilization_ppm(&self) -> Option<u64> {
        if self.interval_wall_time_100ns == 0 {
            return None;
        }
        let value = (u128::from(self.interval_cpu_time_100ns) * 1_000_000)
            / u128::from(self.interval_wall_time_100ns);
        Some(value.min(u128::from(u64::MAX)) as u64)
    }

    pub fn target(&self) -> Result<ProcessTarget> {
        ProcessTarget::new(self.identity, self.image_name.clone())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessInventory {
    pub processes: Vec<ProcessInventoryEntry>,
    /// Processes omitted because they exited, were reused, lacked a matched
    /// pair of CPU samples, or could not be queried with the caller's rights.
    pub skipped_processes: usize,
}

impl ProcessInventory {
    #[must_use]
    pub fn new(processes: Vec<ProcessInventoryEntry>, skipped_processes: usize) -> Self {
        Self {
            processes,
            skipped_processes,
        }
    }
}

/// Documented Windows process priority classes. Realtime is representable so
/// snapshots and explicit rejection are lossless; mutation policy never boosts
/// a process to realtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl fmt::Display for ProcessPriorityClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Idle => "idle",
            Self::BelowNormal => "below_normal",
            Self::Normal => "normal",
            Self::AboveNormal => "above_normal",
            Self::High => "high",
            Self::Realtime => "realtime",
        };
        formatter.write_str(name)
    }
}

/// Process affinity is an OS bit vector, not a processor-group or P/E-core
/// guess. Topology-labelled policies are explicitly rejected until a real
/// topology source supplies a mask.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAffinityPolicy {
    Preserve,
    Mask(usize),
    PerformanceCores,
    EfficiencyCores,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessWorkingSet {
    pub minimum_bytes: usize,
    pub maximum_bytes: usize,
    pub flags: u32,
}

impl ProcessWorkingSet {
    pub const HARDWS_MIN_ENABLE: u32 = 0x0000_0001;
    pub const HARDWS_MIN_DISABLE: u32 = 0x0000_0002;
    pub const HARDWS_MAX_ENABLE: u32 = 0x0000_0004;
    pub const HARDWS_MAX_DISABLE: u32 = 0x0000_0008;
    pub const HARDWS_MASK: u32 = Self::HARDWS_MIN_ENABLE
        | Self::HARDWS_MIN_DISABLE
        | Self::HARDWS_MAX_ENABLE
        | Self::HARDWS_MAX_DISABLE;

    pub fn new(minimum_bytes: usize, maximum_bytes: usize, flags: u32) -> Result<Self> {
        let working_set = Self {
            minimum_bytes,
            maximum_bytes,
            flags,
        };
        working_set.validate()?;
        Ok(working_set)
    }

    pub fn validate(&self) -> Result<()> {
        if self.minimum_bytes == 0 || self.maximum_bytes == 0 {
            return Err(LctrlError::InvalidArgument {
                detail: "process working-set limits must be nonzero".into(),
            });
        }
        if self.minimum_bytes == usize::MAX && self.maximum_bytes == usize::MAX {
            return Err(LctrlError::Unsupported {
                feature: "one-shot working-set trimming is not a restorable quota state".into(),
            });
        }
        if self.minimum_bytes > self.maximum_bytes {
            return Err(LctrlError::InvalidArgument {
                detail: "process working-set minimum exceeds maximum".into(),
            });
        }
        if self.flags & !Self::HARDWS_MASK != 0 {
            return Err(LctrlError::Unsupported {
                feature: format!("process working-set flags 0x{:x}", self.flags),
            });
        }
        if (self.flags & Self::HARDWS_MIN_ENABLE != 0 && self.flags & Self::HARDWS_MIN_DISABLE != 0)
            || (self.flags & Self::HARDWS_MAX_ENABLE != 0
                && self.flags & Self::HARDWS_MAX_DISABLE != 0)
        {
            return Err(LctrlError::InvalidArgument {
                detail: "process working-set enable/disable flags conflict".into(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessState {
    pub priority_class: ProcessPriorityClass,
    pub affinity_mask: usize,
    pub working_set: ProcessWorkingSet,
}

impl ProcessState {
    pub fn validate(&self) -> Result<()> {
        if self.affinity_mask == 0 {
            return Err(LctrlError::Unsupported {
                feature: "empty or multi-group process affinity mask".into(),
            });
        }
        self.working_set.validate()
    }
}

impl fmt::Display for ProcessState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "priority={} affinity=0x{:x} workset={}:{}:0x{:x}",
            self.priority_class,
            self.affinity_mask,
            self.working_set.minimum_bytes,
            self.working_set.maximum_bytes,
            self.working_set.flags
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessSnapshot {
    pub identity: ProcessIdentity,
    pub image_name: String,
    pub image_path: Option<String>,
    pub system_affinity_mask: usize,
    pub state: ProcessState,
}

/// A per-process mutation. `None` and `Preserve` leave the corresponding
/// snapshot field untouched. The defaults never alter system-wide policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ProcessPolicy {
    pub priority_class: Option<ProcessPriorityClass>,
    pub affinity: ProcessAffinityPolicy,
    pub working_set: Option<ProcessWorkingSet>,
}

impl ProcessPolicy {
    /// Conservative boost: AboveNormal priority only, never realtime and no
    /// implicit affinity or working-set change.
    #[must_use]
    pub const fn boost() -> Self {
        Self {
            priority_class: Some(ProcessPriorityClass::AboveNormal),
            affinity: ProcessAffinityPolicy::Preserve,
            working_set: None,
        }
    }

    /// Conservative throttle: BelowNormal priority only. Working-set quotas
    /// or an explicit affinity mask require an explicit caller-supplied policy.
    #[must_use]
    pub const fn throttle() -> Self {
        Self {
            priority_class: Some(ProcessPriorityClass::BelowNormal),
            affinity: ProcessAffinityPolicy::Preserve,
            working_set: None,
        }
    }
}

pub fn validate_process_top_limit(limit: usize) -> Result<()> {
    if limit == 0 || limit > MAX_PROCESS_TOP_LIMIT {
        return Err(LctrlError::InvalidArgument {
            detail: format!(
                "process top limit must be in 1..={MAX_PROCESS_TOP_LIMIT}, got {limit}"
            ),
        });
    }
    Ok(())
}
