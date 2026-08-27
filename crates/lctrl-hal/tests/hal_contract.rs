use std::sync::Arc;

use lctrl_core::{CapabilitySet, HardwareInfo, Platform};
use lctrl_hal::Hal;

struct FakeHal;

impl Hal for FakeHal {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn hardware_info(&self) -> lctrl_core::Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: Some("ThinkBook 14+ 2026".into()),
            family: Some("21VG".into()),
            bios_version: Some("1.07".into()),
        })
    }

    fn capabilities(&self) -> lctrl_core::Result<CapabilitySet> {
        let mut set = CapabilitySet::new(Platform::Linux);
        let _ = set.record("battery.status", lctrl_core::Availability::Available, None);
        Ok(set)
    }
}

#[test]
fn root_hal_trait_accepts_object_safe_calls() {
    let hal: &dyn Hal = &FakeHal;

    assert_eq!(hal.platform(), Platform::Linux);
    let info = hal.hardware_info().expect("hardware info");
    assert_eq!(info.product_name.as_deref(), Some("ThinkBook 14+ 2026"));
    assert_eq!(info.family.as_deref(), Some("21VG"));
    assert_eq!(info.bios_version.as_deref(), Some("1.07"));

    let caps = hal.capabilities().expect("capabilities");
    assert_eq!(caps.platform, Platform::Linux);
    assert_eq!(caps.features.len(), 1);
}

#[test]
fn fake_hal_is_send_and_sync() {
    let hal: Arc<dyn Hal> = Arc::new(FakeHal);
    let clone = Arc::clone(&hal);
    assert_eq!(clone.platform(), Platform::Linux);
}
