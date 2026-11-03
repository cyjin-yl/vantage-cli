use lctrl_core::CommandSelector;
use lctrl_core::{
    Availability, CapabilitySet, HardwareInfo, MagicBayInventory, Platform, StateLayer,
};
use lctrl_hal::{
    Hal, MagicBayControl, SwitchAction, command_capability_for, command_registry,
    minimal_dependency_checklist,
};
use vantage_cli::{
    Cli, CommandServices, ObservedStatus, command_capabilities, command_selector,
    execute_with_services,
};

struct FakeHal;

impl FakeHal {
    fn new() -> Self {
        Self
    }
}

impl Hal for FakeHal {
    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn hardware_info(&self) -> lctrl_core::Result<HardwareInfo> {
        Ok(HardwareInfo {
            product_name: Some("target".into()),
            family: Some("21VG".into()),
            bios_version: Some("1.07".into()),
        })
    }

    fn capabilities(&self) -> lctrl_core::Result<CapabilitySet> {
        let mut set = CapabilitySet::new(Platform::Linux);
        set.record("battery.status", Availability::Available, None)?;
        set.record(
            "perf.mode",
            Availability::Unavailable,
            Some("target".to_owned()),
        )?;
        Ok(set)
    }
}

#[test]
fn registry_covers_every_command_id() {
    for id in lctrl_core::CommandId::ALL {
        assert!(
            !command_capabilities(*id).is_empty(),
            "missing registry rows for {id}"
        );
    }
    assert_eq!(
        command_registry()
            .iter()
            .map(|entry| (entry.command, entry.axes.layer))
            .collect::<Vec<_>>()
            .len(),
        command_registry().len()
    );
}

#[test]
fn usb_and_privacy_layers_are_distinct_entries() {
    assert!(
        command_capability_for(CommandSelector::new(
            lctrl_core::CommandId::USB_ALWAYS_ON,
            StateLayer::Runtime
        ))
        .is_some()
    );
    assert!(
        command_capability_for(CommandSelector::new(
            lctrl_core::CommandId::USB_ALWAYS_ON,
            StateLayer::BiosPersistent
        ))
        .is_some()
    );
    assert!(
        command_capability_for(CommandSelector::new(
            lctrl_core::CommandId::PRIVACY_CAM,
            StateLayer::Runtime
        ))
        .is_some()
    );
    assert!(
        command_capability_for(CommandSelector::new(
            lctrl_core::CommandId::PRIVACY_CAM,
            StateLayer::BiosPersistent
        ))
        .is_some()
    );
}

#[test]
fn cli_aliases_map_to_expected_registry_rows() {
    let paths: &[(&[&str], lctrl_core::CommandId, StateLayer)] = &[
        (
            &["vantage", "battery"],
            lctrl_core::CommandId::BATTERY_STATUS,
            StateLayer::Runtime,
        ),
        (
            &["vantage", "power", "scheme"],
            lctrl_core::CommandId::POWER_SCHEME_ACTIVE,
            StateLayer::OsPersistent,
        ),
        (
            &["vantage", "perf", "fan"],
            lctrl_core::CommandId::PERF_FAN_STATUS,
            StateLayer::Runtime,
        ),
        (
            &["vantage", "perf", "temp"],
            lctrl_core::CommandId::PERF_TEMP_LIST,
            StateLayer::Runtime,
        ),
        (
            &["vantage", "bios"],
            lctrl_core::CommandId::BIOS_LIST,
            StateLayer::BiosPersistent,
        ),
    ];
    for (args, id, layer) in paths {
        let cli = Cli::try_parse_from(*args).unwrap();
        let selector = command_selector(&cli.command).unwrap();
        assert_eq!(selector.command, *id);
        assert_eq!(selector.layer, *layer);
    }
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "off", "--runtime"]).unwrap();
    let selector = command_selector(&cli.command).unwrap();
    assert_eq!(selector.layer, StateLayer::Runtime);
    let cli = Cli::try_parse_from(["vantage", "privacy", "cam", "off", "--persistent"]).unwrap();
    let selector = command_selector(&cli.command).unwrap();
    assert_eq!(selector.layer, StateLayer::BiosPersistent);
    assert!(
        command_selector(
            &Cli::try_parse_from(["vantage", "privacy", "cam", "off"])
                .unwrap()
                .command
        )
        .is_none()
    );
}

#[test]
fn unknown_hardware_is_unsupported_without_service() {
    let hal = FakeHal::new();
    let cli = Cli::try_parse_from(["vantage", "--json", "magicbay", "lte", "status"]).unwrap();
    let result = execute_with_services(cli, CommandServices::new(&hal).with_magicbay(&MagicBay));
    let error = result.expect_err("magicbay LTE should be unsupported");
    assert!(
        matches!(error, lctrl_core::LctrlError::Unsupported { ref feature } if feature.contains("magicbay.lte")),
        "unexpected error {error}"
    );
    let observed = vantage_cli::evaluate_commands(&hal).unwrap();
    let row = observed
        .iter()
        .find(|row| {
            row.command.as_str() == "magicbay.lte.status" && row.layer == StateLayer::Runtime
        })
        .unwrap();
    assert_eq!(row.status, ObservedStatus::Unsupported);
}

struct MagicBay;

impl MagicBayControl for MagicBay {
    fn detect_magicbay(&self) -> lctrl_core::Result<MagicBayInventory> {
        Ok(MagicBayInventory::default())
    }
}

#[test]
fn capability_command_reports_the_same_matrix_as_the_registry_observation() {
    let hal = FakeHal::new();
    let cli = Cli::try_parse_from(["vantage", "--json", "capability"]).unwrap();
    let result = execute_with_services(cli, CommandServices::new(&hal)).unwrap();

    let rows = result
        .json
        .as_array()
        .expect("capability JSON is an array of rows");
    let observed = vantage_cli::evaluate_commands(&hal).unwrap();
    assert_eq!(rows.len(), observed.len());
    let charge = rows
        .iter()
        .find(|row| row["command"] == "battery.charge-mode")
        .expect("charge-mode row is reported");
    assert_eq!(charge["status"], "unverified");
    let thresholds = rows
        .iter()
        .find(|row| row["command"] == "battery.thresholds")
        .expect("thresholds row is reported");
    assert_eq!(thresholds["status"], "unsupported");
    let text = vantage_cli::render_success(&result, false);
    assert!(text.contains("battery.charge-mode"));
    assert!(text.contains("unsupported:"));
}

#[test]
fn runtime_and_persistent_layers_use_separate_services_and_statuses() {
    let hal = FakeHal::new();
    let observation = vantage_cli::evaluate_commands(&hal).unwrap();
    let runtime = observation
        .iter()
        .find(|row| row.command.as_str() == "privacy.cam" && row.layer == StateLayer::Runtime)
        .unwrap();
    let persistent = observation
        .iter()
        .find(|row| {
            row.command.as_str() == "privacy.cam" && row.layer == StateLayer::BiosPersistent
        })
        .unwrap();
    assert!(runtime.service_roles.contains(&"privacy"));
    assert!(persistent.service_roles.contains(&"bios"));
    assert_eq!(runtime.status, ObservedStatus::Unverified);
    assert_eq!(persistent.status, ObservedStatus::Unverified);
}

#[test]
fn touchpad_status_and_recovery_have_distinct_safety_axes() {
    let status = command_capability_for(lctrl_core::CommandSelector::new(
        lctrl_core::CommandId::TOUCHPAD_STATUS,
        StateLayer::Runtime,
    ))
    .unwrap();
    let recover = command_capability_for(lctrl_core::CommandSelector::new(
        lctrl_core::CommandId::TOUCHPAD_RECOVER,
        StateLayer::Runtime,
    ))
    .unwrap();
    assert!(status.axes.read && !status.axes.write);
    assert!(recover.axes.write);
    assert_eq!(recover.axes.readback, lctrl_core::Readback::Required);
    assert_eq!(recover.axes.rollback, lctrl_core::Rollback::Unavailable);
}

#[test]
fn unavailable_staging_is_reported_for_bios_writes_not_reads() {
    let hal = FakeHal::new();
    let mut capabilities = hal.capabilities().unwrap();
    capabilities
        .record(
            "bios.staging.contract",
            Availability::Unavailable,
            Some("ownership unverified".into()),
        )
        .unwrap();
    let rows = vantage_cli::evaluate_commands_with_hardware(
        &hal,
        &capabilities,
        lctrl_hal::HardwareObservation {
            magicbay: lctrl_core::HardwarePresence::Unknown,
        },
    );
    let write = rows
        .iter()
        .find(|row| row.command == lctrl_core::CommandId::KBD_FNLOCK)
        .unwrap();
    assert_eq!(write.status, ObservedStatus::Unavailable);
    assert!(
        write
            .detail
            .as_deref()
            .unwrap()
            .contains("ownership unverified")
    );
    let read = rows
        .iter()
        .find(|row| row.command == lctrl_core::CommandId::BIOS_GET)
        .unwrap();
    assert_ne!(read.status, ObservedStatus::Unavailable);
}

#[test]
fn capability_metadata_does_not_prove_an_unobserved_service_is_attached() {
    let hal = FakeHal::new();
    let rows = vantage_cli::evaluate_commands(&hal).unwrap();
    let battery = rows
        .iter()
        .find(|row| row.command == lctrl_core::CommandId::BATTERY_STATUS)
        .unwrap();
    assert_eq!(battery.status, ObservedStatus::Unverified);
    assert!(battery.service_roles.contains(&"battery"));
    assert!(
        rows.iter()
            .flat_map(|row| &row.dependencies)
            .all(|dependency| {
                dependency
                    .dependency
                    .platform()
                    .is_none_or(|platform| platform == Platform::Linux)
            })
    );
}

#[test]
fn observed_absent_hardware_is_not_applicable_not_unsupported() {
    let hal = FakeHal::new();
    let capabilities = hal.capabilities().unwrap();
    let rows = vantage_cli::evaluate_commands_with_hardware(
        &hal,
        &capabilities,
        lctrl_hal::HardwareObservation {
            magicbay: lctrl_core::HardwarePresence::Absent,
        },
    );
    let row = rows
        .iter()
        .find(|row| {
            row.command.as_str() == "magicbay.lte.status" && row.layer == StateLayer::Runtime
        })
        .unwrap();
    assert_eq!(row.status, ObservedStatus::NotApplicable);
}

#[test]
fn mutations_have_required_readback_and_unsupported_have_honest_axes() {
    let hal = FakeHal::new();
    let rows = vantage_cli::evaluate_commands(&hal).unwrap();
    let charge = rows
        .iter()
        .find(|row| {
            row.command.as_str() == "battery.charge-mode" && row.layer == StateLayer::Runtime
        })
        .unwrap();
    assert_eq!(charge.axes.readback, lctrl_core::Readback::Required);
    assert_eq!(charge.axes.rollback, lctrl_core::Rollback::Transactional);
    assert!(charge.axes.write);
    let thresholds = rows
        .iter()
        .find(|row| row.command.as_str() == "battery.thresholds")
        .unwrap();
    assert_eq!(thresholds.status, ObservedStatus::Unsupported);
    assert_eq!(thresholds.axes.readback, lctrl_core::Readback::Unverified);
    assert_eq!(thresholds.axes.rollback, lctrl_core::Rollback::Unavailable);
}

#[test]
fn brightness_read_and_write_have_distinct_unverified_capabilities() {
    let rows = vantage_cli::evaluate_commands(&FakeHal::new()).unwrap();
    for (args, id, writes) in [
        (
            &["vantage", "panel", "brightness"][..],
            lctrl_core::CommandId::PANEL_BRIGHTNESS_STATUS,
            false,
        ),
        (
            &["vantage", "panel", "brightness", "50"][..],
            lctrl_core::CommandId::PANEL_BRIGHTNESS_SET,
            true,
        ),
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        let selector = command_selector(&cli.command).unwrap();
        assert_eq!(selector.command, id);
        let observed = rows.iter().find(|row| row.command == id).unwrap();
        assert_eq!(observed.status, ObservedStatus::Unverified);
        assert_eq!(observed.axes.write, writes);
    }
}

#[test]
fn vendor_dependency_checklist_never_recommends_blanket_disable() {
    let selected = command_registry()
        .iter()
        .map(|entry| entry.selector())
        .collect::<Vec<_>>();
    let checklist = minimal_dependency_checklist(&selected);
    for switch in &checklist {
        let expected = if switch.required() {
            SwitchAction::Keep
        } else {
            SwitchAction::Review
        };
        assert_eq!(switch.action(), expected);
        assert!(!switch.reason().is_empty());
    }
    let perf = command_registry()
        .iter()
        .find(|entry| entry.command.as_str() == "perf.mode")
        .unwrap();
    assert!(
        perf.dependencies
            .iter()
            .any(|dependency| dependency.dependency
                == lctrl_core::Dependency::LenovoProcessManagement)
    );
}
