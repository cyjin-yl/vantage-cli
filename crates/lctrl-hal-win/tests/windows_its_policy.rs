#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
//
// The decode rules themselves are pinned on the host by
// `crates/lctrl-core/tests/its_policy_decode.rs`; this file covers the
// assembly: absent vs unreadable vs undecodable, and the read-only guarantee.
use parking_lot::Mutex;

use lctrl_core::LctrlError;
use lctrl_hal::ItsPolicyControl;
use lctrl_hal_win::{PerformanceRegistryReader, WindowsPerformanceP0};

struct FakeRegistry {
    dwords: Vec<(&'static str, u32)>,
    strings: Vec<(&'static str, &'static str)>,
    denied: Vec<&'static str>,
    /// Any write through this fixture fails the test instead of being
    /// tolerated: `perf policy` is read-only.
    write_attempts: Mutex<Vec<String>>,
}

impl FakeRegistry {
    fn new() -> Self {
        Self {
            dwords: Vec::new(),
            strings: Vec::new(),
            denied: Vec::new(),
            write_attempts: Mutex::new(Vec::new()),
        }
    }

    fn dword(mut self, name: &'static str, value: u32) -> Self {
        self.dwords.push((name, value));
        self
    }

    fn string(mut self, name: &'static str, value: &'static str) -> Self {
        self.strings.push((name, value));
        self
    }

    fn denied(mut self, name: &'static str) -> Self {
        self.denied.push(name);
        self
    }

    fn write_attempts(&self) -> Vec<String> {
        self.write_attempts.lock().clone()
    }
}

impl PerformanceRegistryReader for FakeRegistry {
    fn read_dword(&self, value: &str) -> lctrl_core::Result<u32> {
        if self.denied.iter().any(|name| *name == value) {
            return Err(LctrlError::PermissionDenied {
                need: "administrator".into(),
            });
        }
        self.dwords
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, number)| *number)
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("missing test registry {value}"),
            })
    }

    fn write_dword(&self, value: &str, _data: u32) -> lctrl_core::Result<()> {
        self.write_attempts.lock().push(value.to_owned());
        Ok(())
    }

    fn read_string(&self, value: &str) -> lctrl_core::Result<String> {
        self.strings
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, text)| (*text).to_string())
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("missing test registry {value}"),
            })
    }
}

fn live_registry() -> FakeRegistry {
    FakeRegistry::new()
        // Values as published on the 21VG target.
        .dword("Policy_PLxSetMode", 0x0001_0001)
        .dword("Policy_FanSpeedAddress", 0)
        .dword("Policy_APMinSTDFan", 1)
        .dword("Policy_APMinSTDFan_GPU", 60)
        .dword("Policy_APMinSTDFan_CPU", 150)
        .dword("Policy_TurboTime", 0x0808_0808)
        .dword("Policy_TurboValue", 20)
        .dword("Policy_EPPTurbo", 0x0508_0000)
        // The dispatcher's published `Policy_PowerBucket` default does not fit
        // its 12-slot array, so it must surface as an issue, not a short table.
        .string(
            "Policy_PowerBucket",
            "0:15;10:30;25:45;38:50;40:70;60:80;70:100;",
        )
        .string(
            "Policy_CPPC",
            "50:80;50:72;50:72;40:60;40:50;40:50;30:50;30:50",
        )
        .string("Policy_CFQC", "0:20;0:20;0:25;0:30;0:35;0:50;0:50;0:50;")
        .string("Policy_CFQC1", "0:25;0:25;0:30;0:35;0:40;0:60;0:60;0:60;")
}

impl PerformanceRegistryReader for &FakeRegistry {
    fn read_dword(&self, value: &str) -> lctrl_core::Result<u32> {
        <FakeRegistry as PerformanceRegistryReader>::read_dword(*self, value)
    }

    fn write_dword(&self, value: &str, data: u32) -> lctrl_core::Result<()> {
        <FakeRegistry as PerformanceRegistryReader>::write_dword(*self, value, data)
    }

    fn read_string(&self, value: &str) -> lctrl_core::Result<String> {
        <FakeRegistry as PerformanceRegistryReader>::read_string(*self, value)
    }
}

#[test]
fn policy_decodes_packed_values_with_the_raw_ones_beside_them() {
    let registry = live_registry();
    let controller = WindowsPerformanceP0::new(&registry);

    let policy = controller.its_policy().unwrap();

    let plx = policy.plx_set_mode.expect("PLxSetMode is published");
    assert_eq!(plx.raw, 0x0001_0001);
    assert_eq!((plx.ac, plx.dc), (1, 1));

    let fan = policy.ap_min_std_fan.expect("APMinSTDFan is published");
    assert!(fan.enable);
    // 60 is kept, 150 clamps to the dispatcher's 50.
    assert_eq!((fan.gpu_threshold_pct, fan.cpu_threshold_pct), (60, 50));

    let turbo = policy.turbo_value.expect("TurboValue is published");
    assert_eq!(turbo.raw, 20);
    assert!(!turbo.default_epp);

    let time = policy.turbo_time.expect("TurboTime is published");
    assert_eq!(time.raw, 0x0808_0808);
    assert_eq!(time.turbo_time, 8);

    let epp = policy.epp_turbo.expect("EPPTurbo is published");
    assert_eq!((epp.mode_nibble, epp.turbo_time), (5, 8));
    assert!(!epp.not_turbo_to_apm);

    assert_eq!(policy.fan_speed_address, Some(0));
    assert!(
        registry.write_attempts().is_empty(),
        "perf policy must not write: {:?}",
        registry.write_attempts()
    );
}

#[test]
fn policy_reports_a_table_that_does_not_fit_instead_of_shortening_it() {
    let registry = live_registry();
    let policy = WindowsPerformanceP0::new(&registry).its_policy().unwrap();

    assert!(
        policy.power_bucket.is_none(),
        "a 14-number value cannot be a 12-slot table"
    );
    let issue = policy
        .issues
        .iter()
        .find(|issue| issue.value == "Policy_PowerBucket")
        .expect("the refused table is reported");
    assert_eq!(
        issue.raw.as_deref(),
        Some("0:15;10:30;25:45;38:50;40:70;60:80;70:100;")
    );
    assert!(issue.detail.contains("12"), "detail: {}", issue.detail);

    // The 8-pair curves fit their 16 slots and decode.
    let cppc = policy.cppc.expect("CPPC is published");
    assert_eq!(cppc.slots.len(), 16);
    assert_eq!(cppc.pairs()[0], (50, 80));
    assert_eq!(policy.cfqc.unwrap().slots.len(), 16);
    assert_eq!(policy.cfqc1.unwrap().slots.len(), 16);
}

#[test]
fn policy_keeps_absent_values_quiet_and_surfaces_read_failures() {
    let registry = live_registry().denied("Policy_TurboValue");
    let policy = WindowsPerformanceP0::new(&registry).its_policy().unwrap();

    // One denied value does not disturb the rest of the decode.
    assert!(policy.epp_turbo.is_some());
    assert!(policy.plx_set_mode.is_some());

    // Present but unreadable: reported, with the error text.
    assert!(policy.turbo_value.is_none());
    let issue = policy
        .issues
        .iter()
        .find(|issue| issue.value == "Policy_TurboValue")
        .expect("the failed read is reported");
    assert!(issue.raw.is_none());
    assert!(
        issue.detail.contains("permission denied"),
        "detail: {}",
        issue.detail
    );
}

#[test]
fn policy_on_a_machine_without_the_dispatcher_publishes_nothing() {
    let registry = FakeRegistry::new();
    let policy = WindowsPerformanceP0::new(&registry).its_policy().unwrap();

    assert!(policy.plx_set_mode.is_none());
    assert!(policy.power_bucket.is_none());
    assert!(policy.issues.is_empty(), "issues: {:?}", policy.issues);
    assert!(registry.write_attempts().is_empty());
}
