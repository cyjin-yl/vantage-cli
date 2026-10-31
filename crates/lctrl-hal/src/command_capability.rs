use std::collections::BTreeMap;

use lctrl_core::{
    CommandAxes, CommandDependency, CommandId, CommandSelector, Dependency, DependencyUse,
    HardwarePresence, HardwareRequirement, Readback, Rollback, StateLayer,
};

/// A concrete service or executor that can implement a command.
///
/// Roles map one-to-one to `CommandServices` fields only when the platform
/// composition root exposes that field. Unexposed roles remain explicit so a
/// registry entry never looks supported merely because a field happens to exist.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ServiceRole {
    Hal,
    Battery,
    Bios,
    Diagnostics,
    Fan,
    Keyboard,
    MagicBay,
    Performance,
    Power,
    Privacy,
    Touchpad,
    Panel,
    PanelBrightness,
    Temperature,
    Tuning,
    Update,
    Events,
    Process,
    ProcessorPower,
    Usb,
    Sense,
    Audio,
    MobileBroadband,
    MagicBayCamera,
    MagicBayDisplay,
    Osd,
    Hotkey,
    Wifi,
    MachineFix,
    ScenePolicy,
}

impl ServiceRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hal => "hal",
            Self::Battery => "battery",
            Self::Bios => "bios",
            Self::Diagnostics => "diagnostics",
            Self::Fan => "fan",
            Self::Keyboard => "keyboard",
            Self::MagicBay => "magicbay",
            Self::Performance => "performance",
            Self::Power => "power",
            Self::Privacy => "privacy",
            Self::Touchpad => "touchpad",
            Self::Panel => "panel",
            Self::PanelBrightness => "panel_brightness",
            Self::Temperature => "temperature",
            Self::Tuning => "tuning",
            Self::Update => "update",
            Self::Events => "events",
            Self::Process => "process",
            Self::ProcessorPower => "processor_power",
            Self::Usb => "usb",
            Self::Sense => "sense",
            Self::Audio => "audio",
            Self::MobileBroadband => "mobile_broadband",
            Self::MagicBayCamera => "magicbay_camera",
            Self::MagicBayDisplay => "magicbay_display",
            Self::Osd => "osd",
            Self::Hotkey => "hotkey",
            Self::Wifi => "wifi",
            Self::MachineFix => "machine_fix",
            Self::ScenePolicy => "scene_policy",
        }
    }
}

/// How a registry entry proves its intended feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureBinding {
    /// Direct `CapabilitySet` probe key.
    Capability(&'static str),
    /// Meaningful CLI/HAL feature name, but no current capability probe.
    Unprobed(&'static str),
    /// Any of these capabilities is sufficient.
    AnyOf(&'static [&'static str]),
    /// The command depends on a profile- or snapshot-selected subset.
    Dynamic(&'static [&'static str]),
    /// The implementation is the service/executor itself and has no HAL probe.
    ServiceOnly,
    /// A pure local operation with no HAL feature.
    LocalOnly,
}

impl FeatureBinding {
    #[must_use]
    pub fn feature_ids(self) -> &'static [&'static str] {
        match self {
            Self::AnyOf(features) | Self::Dynamic(features) => features,
            _ => &[],
        }
    }

    #[must_use]
    pub const fn is_probed(self) -> bool {
        matches!(
            self,
            Self::Capability(_) | Self::AnyOf(_) | Self::Dynamic(_)
        )
    }

    #[must_use]
    pub const fn direct_feature(self) -> Option<&'static str> {
        match self {
            Self::Capability(feature) | Self::Unprobed(feature) => Some(feature),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceBinding {
    Local,
    One(ServiceRole),
    AnyOf(&'static [ServiceRole]),
    /// A composite command can use any subset; absence reduces confidence but
    /// is not itself `Unsupported`.
    Composite(&'static [ServiceRole]),
    /// Handled by the platform binary outside `CommandServices`.
    External,
    /// No current HAL trait method or executor is attached.
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchSupport {
    /// The current CLI dispatch reaches this service/executor. It is not a
    /// hardware-effect claim; observed capability is still required.
    Implemented,
    /// The current dispatch is explicitly unavailable.
    Unsupported(&'static str),
    External,
}

#[derive(Clone, Copy, Debug)]
pub struct CommandCapability {
    pub command: CommandId,
    pub axes: CommandAxes,
    pub feature: FeatureBinding,
    pub service: ServiceBinding,
    pub dependencies: &'static [CommandDependency],
    pub hardware: HardwareRequirement,
    pub dispatch: DispatchSupport,
    pub notes: Option<&'static str>,
}

impl CommandCapability {
    #[must_use]
    pub const fn new(
        command: CommandId,
        axes: CommandAxes,
        feature: FeatureBinding,
        service: ServiceBinding,
        dependencies: &'static [CommandDependency],
        dispatch: DispatchSupport,
    ) -> Self {
        Self {
            command,
            axes,
            feature,
            service,
            dependencies,
            hardware: HardwareRequirement::None,
            dispatch,
            notes: None,
        }
    }

    #[must_use]
    pub const fn with_hardware(mut self, hardware: HardwareRequirement) -> Self {
        self.hardware = hardware;
        self
    }

    #[must_use]
    pub const fn selector(&self) -> CommandSelector {
        self.axes.selector(self.command)
    }

    #[must_use]
    pub const fn static_reason(&self) -> Option<&'static str> {
        match self.dispatch {
            DispatchSupport::Implemented | DispatchSupport::External => self.notes,
            DispatchSupport::Unsupported(reason) => Some(reason),
        }
    }
}

/// Cross-process executor observations. Absence is recorded only from actual
/// inventories; `Unknown` never becomes NotApplicable or available.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HardwareObservation {
    pub magicbay: HardwarePresence,
}

const SNAPSHOT_FEATURES: &[&str] = &[
    "battery.charge_mode",
    "perf.mode",
    "perf.fan.mode",
    "tune.epp",
    "tune.pl1",
    "tune.pl2",
    "tune.tau",
    "kbd.backlight",
    "touchpad",
    "privacy.camera",
    "power.scheme",
];

const SNAPSHOT_SERVICES: &[ServiceRole] = &[
    ServiceRole::Battery,
    ServiceRole::Performance,
    ServiceRole::Fan,
    ServiceRole::Tuning,
    ServiceRole::Keyboard,
    ServiceRole::Touchpad,
    ServiceRole::Privacy,
    ServiceRole::Power,
];

/// Complete static registry for every current CLI command/subcommand and
/// argument-selected layer.
pub const COMMAND_REGISTRY: &[CommandCapability] = &[
    CommandCapability::new(
        CommandId::INFO,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::LocalOnly,
        ServiceBinding::One(ServiceRole::Hal),
        &[],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::DOCTOR,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::LocalOnly,
        ServiceBinding::One(ServiceRole::Hal),
        &[],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("battery.status"),
        ServiceBinding::One(ServiceRole::Battery),
        &[
            lctrl_core::optional(Dependency::EnergyDriver),
            lctrl_core::optional(Dependency::PublicWin32Cim),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_ADAPTER,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("battery.adapter"),
        ServiceBinding::One(ServiceRole::Battery),
        &[
            lctrl_core::optional(Dependency::EnergyDriver),
            lctrl_core::optional(Dependency::PublicWin32Cim),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_CHARGE_MODE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("battery.charge_mode"),
        ServiceBinding::One(ServiceRole::Battery),
        &[
            lctrl_core::required(Dependency::EnergyDriver),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_THRESHOLDS,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Absent("battery.thresholds"),
        ServiceBinding::Unsupported,
        &[],
        DispatchSupport::Unsupported("custom charge thresholds are unsupported on this target: the PWRMGRV/PWMIF32V chain is absent and the ACPI MHQI method is missing (RE-interfaces.md §11.6)"),
    ),
    CommandCapability::new(
        CommandId::BATTERY_EXTREME_LIFE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Capability("battery.extreme-life"),
        ServiceBinding::Unsupported,
        &[],
        DispatchSupport::Unsupported("extreme-life must match the original AiTurbo EcoQoS daemon contract; a generic substitute is not implemented (RE-interfaces.md §11.6)"),
    ),
    CommandCapability::new(
        CommandId::BATTERY_NIGHT_CHARGE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("battery.night-charge"),
        ServiceBinding::One(ServiceRole::Battery),
        &[lctrl_core::required(Dependency::EnergyDriver)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_TEMPORARY_MODE,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("battery.temporary-mode"),
        ServiceBinding::One(ServiceRole::Battery),
        &[lctrl_core::required(Dependency::EnergyDriver)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BATTERY_WATCH,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::ServiceOnly,
        ServiceBinding::One(ServiceRole::Events),
        &[
            lctrl_core::required(Dependency::WindowsWmiEvents),
            lctrl_core::required(Dependency::LinuxUdev),
        ],
        DispatchSupport::Unsupported("battery event streaming requires an event service"),
    ),
    CommandCapability::new(
        CommandId::USB_ALWAYS_ON,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("usb.charging"),
        ServiceBinding::One(ServiceRole::Usb),
        &[lctrl_core::required(Dependency::EnergyDriver)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::USB_ALWAYS_ON,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("bios"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::USB_CHARGE_ON_BATTERY,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("usb.charging"),
        ServiceBinding::One(ServiceRole::Usb),
        &[lctrl_core::required(Dependency::EnergyDriver)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::USB_CHARGE_ON_BATTERY,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("bios"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_ACTIVE,
        CommandAxes::read_only(StateLayer::OsPersistent),
        FeatureBinding::Capability("power.scheme"),
        ServiceBinding::One(ServiceRole::Power),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_LIST,
        CommandAxes::read_only(StateLayer::OsPersistent),
        FeatureBinding::Capability("power.scheme"),
        ServiceBinding::One(ServiceRole::Power),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_GET,
        CommandAxes::read_only(StateLayer::OsPersistent),
        FeatureBinding::Capability("power.scheme"),
        ServiceBinding::One(ServiceRole::Power),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_SETTING_READ,
        CommandAxes::read_only(StateLayer::OsPersistent),
        FeatureBinding::Capability("power.scheme.setting.read"),
        ServiceBinding::One(ServiceRole::Power),
        &[lctrl_core::required(Dependency::Win32PowerApi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_APPLY,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("power.scheme"),
        ServiceBinding::One(ServiceRole::Power),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SCHEME_SET,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("power.scheme.setting"),
        ServiceBinding::One(ServiceRole::Power),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::POWER_SAVER_ONCE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("power.saver-once"),
        ServiceBinding::Composite(&[ServiceRole::Battery, ServiceRole::Power]),
        &[
            lctrl_core::required(Dependency::EnergyDriver),
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Unsupported("composite saver workflow has no transactional executor"),
    ),
    CommandCapability::new(
        CommandId::PERF_MODE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.mode"),
        ServiceBinding::One(ServiceRole::Performance),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::optional(Dependency::Win32Registry),
            lctrl_core::optional(Dependency::LenovoProcessManagement),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::AnyOf(&["perf.fan.mode", "perf.fan.inventory"]),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_AUTO,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.fan.mode"),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_MANUAL,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.fan.mode"),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_FULLSPEED,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.fan.mode"),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_SMART,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.fan.mode"),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_FAN_CURVE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Capability("perf.fan.curve"),
        ServiceBinding::One(ServiceRole::Fan),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Unsupported("no verified fan-table kernel interface is attached"),
    ),
    CommandCapability::new(
        CommandId::PERF_TEMP_LIST,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("perf.temp"),
        ServiceBinding::One(ServiceRole::Temperature),
        &[
            lctrl_core::required(Dependency::PublicWin32Cim),
            lctrl_core::optional(Dependency::EnergyDriver),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_TEMP_READ,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("perf.temp"),
        ServiceBinding::One(ServiceRole::Temperature),
        &[
            lctrl_core::required(Dependency::PublicWin32Cim),
            lctrl_core::optional(Dependency::EnergyDriver),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_TEMP_WATCH,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("perf.temp"),
        ServiceBinding::One(ServiceRole::Temperature),
        &[
            lctrl_core::required(Dependency::PublicWin32Cim),
            lctrl_core::optional(Dependency::EnergyDriver),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_PL1,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("tune.pl1"),
        ServiceBinding::One(ServiceRole::Tuning),
        &[
            lctrl_core::optional(Dependency::IntelDptf),
            lctrl_core::required(Dependency::LinuxPowercap),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_PL2,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("tune.pl2"),
        ServiceBinding::One(ServiceRole::Tuning),
        &[
            lctrl_core::optional(Dependency::IntelDptf),
            lctrl_core::required(Dependency::LinuxPowercap),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_DBDC_GET,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Capability("perf.dbdc"),
        ServiceBinding::One(ServiceRole::Performance),
        &[lctrl_core::required(Dependency::LenovoVendorWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PERF_TOP,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Capability("perf.top"),
        ServiceBinding::One(ServiceRole::Process),
        &[
            lctrl_core::required(Dependency::Win32ProcessApi),
            lctrl_core::required(Dependency::LinuxProcfs),
        ],
        DispatchSupport::Unsupported("process telemetry is not attached"),
    ),
    CommandCapability::new(
        CommandId::PERF_BOOST,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.boost"),
        ServiceBinding::One(ServiceRole::Process),
        &[lctrl_core::required(Dependency::Win32ProcessApi)],
        DispatchSupport::Unsupported("process policy control is not attached"),
    ),
    CommandCapability::new(
        CommandId::PERF_THROTTLE,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("perf.throttle"),
        ServiceBinding::One(ServiceRole::Process),
        &[lctrl_core::required(Dependency::Win32ProcessApi)],
        DispatchSupport::Unsupported("process policy control is not attached"),
    ),
    CommandCapability::new(
        CommandId::TUNE_PROFILE_LIST,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::LocalOnly,
        ServiceBinding::Local,
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_PROFILE_SHOW,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::LocalOnly,
        ServiceBinding::Local,
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_PROFILE_APPLY,
        CommandAxes::mutation(
            StateLayer::Composite,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Dynamic(&[
            "battery.charge_mode",
            "perf.mode",
            "perf.fan.mode",
            "tune.epp",
            "tune.pl1",
            "tune.pl2",
            "tune.tau",
            "kbd.backlight",
        ]),
        ServiceBinding::Composite(&[
            ServiceRole::Battery,
            ServiceRole::Performance,
            ServiceRole::Fan,
            ServiceRole::Tuning,
            ServiceRole::Keyboard,
        ]),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_PL1,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Dynamic(&["tune.pl1", "tune.pl2", "tune.tau"]),
        ServiceBinding::One(ServiceRole::Tuning),
        &[
            lctrl_core::optional(Dependency::IntelDptf),
            lctrl_core::required(Dependency::LinuxPowercap),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_EPP,
        CommandAxes::mutation(
            StateLayer::PlatformDependent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("tune.epp"),
        ServiceBinding::One(ServiceRole::Tuning),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_TURBO,
        CommandAxes::mutation(
            StateLayer::PlatformDependent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("tune.turbo"),
        ServiceBinding::One(ServiceRole::ProcessorPower),
        &[
            lctrl_core::required(Dependency::Win32PowerApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_RESTORE,
        CommandAxes::mutation(
            StateLayer::Composite,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Dynamic(&[
            "battery.charge_mode",
            "perf.mode",
            "perf.fan.mode",
            "tune.epp",
            "tune.pl1",
            "tune.pl2",
            "tune.tau",
            "kbd.backlight",
        ]),
        ServiceBinding::Composite(&[
            ServiceRole::Battery,
            ServiceRole::Performance,
            ServiceRole::Fan,
            ServiceRole::Tuning,
            ServiceRole::Keyboard,
        ]),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_TELEMETRY,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::AnyOf(&[
            "tune.epp",
            "tune.pl1",
            "tune.pl2",
            "tune.tau",
            "battery.status",
            "perf.mode",
            "perf.temp",
        ]),
        ServiceBinding::Composite(&[
            ServiceRole::Tuning,
            ServiceRole::Battery,
            ServiceRole::Performance,
            ServiceRole::Temperature,
        ]),
        &[],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TUNE_WATCH,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::ServiceOnly,
        ServiceBinding::One(ServiceRole::Events),
        &[
            lctrl_core::required(Dependency::WindowsWmiEvents),
            lctrl_core::required(Dependency::LinuxUdev),
        ],
        DispatchSupport::Unsupported("tuning event streaming requires an event service"),
    ),
    CommandCapability::new(
        CommandId::KBD_BACKLIGHT,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("kbd.backlight"),
        ServiceBinding::One(ServiceRole::Keyboard),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::KBD_FNLOCK,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("kbd.fnlock"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::KBD_FN_CTRL_SWAP,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Unprobed("kbd.fn-ctrl-swap"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::KBD_WINLOCK,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Capability("wmi.gamezone.methods"),
        ServiceBinding::Unsupported,
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported("GAMEZONE Win-key methods are not verified on the target"),
    ),
    CommandCapability::new(
        CommandId::TOUCHPAD,
        CommandAxes::mutation(
            StateLayer::PlatformDependent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("touchpad"),
        ServiceBinding::One(ServiceRole::Touchpad),
        &[
            lctrl_core::required(Dependency::Win32DeviceManagement),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TOUCHPAD_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("touchpad.status"),
        ServiceBinding::One(ServiceRole::Touchpad),
        &[
            lctrl_core::required(Dependency::Win32DeviceManagement),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::TOUCHPAD_RECOVER,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("touchpad.recover"),
        ServiceBinding::One(ServiceRole::Touchpad),
        &[lctrl_core::required(Dependency::Win32DeviceManagement)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PANEL_RATE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("panel.refresh"),
        ServiceBinding::One(ServiceRole::Panel),
        &[
            lctrl_core::required(Dependency::Win32DisplayApi),
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxDrm),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PANEL_BRIGHTNESS_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("panel.brightness.status"),
        ServiceBinding::One(ServiceRole::PanelBrightness),
        &[lctrl_core::required(Dependency::PublicWin32Cim)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PANEL_BRIGHTNESS_SET,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("panel.brightness.set"),
        ServiceBinding::One(ServiceRole::PanelBrightness),
        &[lctrl_core::required(Dependency::PublicWin32Cim)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PANEL_COLOR,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("panel.color.write"),
        ServiceBinding::One(ServiceRole::Panel),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxDrm),
        ],
        DispatchSupport::Unsupported("no verified panel color writer is attached"),
    ),
    CommandCapability::new(
        CommandId::PANEL_SUPER_RESOLUTION,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("panel.super-resolution.write"),
        ServiceBinding::Unsupported,
        &[],
        DispatchSupport::Unsupported("no verified SmartEngine writer is attached"),
    ),
    CommandCapability::new(
        CommandId::PANEL_OVERDRIVE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("panel.overdrive.write"),
        ServiceBinding::One(ServiceRole::Panel),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxDrm),
        ],
        DispatchSupport::Unsupported("no verified overdrive writer is attached"),
    ),
    CommandCapability::new(
        CommandId::PANEL_EYE_CARE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("panel.eye-care.write"),
        ServiceBinding::One(ServiceRole::Panel),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::LinuxDrm),
        ],
        DispatchSupport::Unsupported("no verified eye-care writer is attached"),
    ),
    CommandCapability::new(
        CommandId::PRIVACY_CAM,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("privacy.camera"),
        ServiceBinding::One(ServiceRole::Privacy),
        &[
            lctrl_core::required(Dependency::Win32DeviceManagement),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PRIVACY_CAM,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("privacy.camera"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PRIVACY_MIC,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("privacy.microphone"),
        ServiceBinding::One(ServiceRole::Privacy),
        &[
            lctrl_core::required(Dependency::Win32DeviceManagement),
            lctrl_core::required(Dependency::WindowsAudio),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Unsupported("runtime microphone HAL/CLI wiring is not attached"),
    ),
    CommandCapability::new(
        CommandId::PRIVACY_MIC,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("privacy.microphone"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PRIVACY_FINGERPRINT,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("privacy.fingerprint"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::PRIVACY_STATUS,
        CommandAxes::read_only(StateLayer::Composite),
        FeatureBinding::AnyOf(&[
            "privacy.camera",
            "privacy.microphone",
            "privacy.fingerprint",
        ]),
        ServiceBinding::AnyOf(&[ServiceRole::Privacy, ServiceRole::Bios]),
        &[
            lctrl_core::required(Dependency::LenovoFirmwareWmi),
            lctrl_core::required(Dependency::Win32DeviceManagement),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("sense.status"),
        ServiceBinding::One(ServiceRole::Sense),
        &[lctrl_core::required(Dependency::SmartSense)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_LOCK_ON_LEAVE,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("sense.lock-on-leave"),
        ServiceBinding::One(ServiceRole::Sense),
        &[
            lctrl_core::required(Dependency::SmartSense),
            lctrl_core::optional(Dependency::WindowsHpd),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_WAKE_ON_APPROACH,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("sense.wake-on-approach"),
        ServiceBinding::One(ServiceRole::Sense),
        &[
            lctrl_core::required(Dependency::SmartSense),
            lctrl_core::optional(Dependency::WindowsHpd),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_PAUSE_VIDEO,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("sense.pause-video"),
        ServiceBinding::One(ServiceRole::Sense),
        &[
            lctrl_core::required(Dependency::SmartSense),
            lctrl_core::optional(Dependency::WindowsHpd),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_ATTENTION_TRACKING,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("sense.attention-tracking"),
        ServiceBinding::One(ServiceRole::Sense),
        &[
            lctrl_core::required(Dependency::SmartSense),
            lctrl_core::optional(Dependency::WindowsHpd),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SENSE_KBD_LIGHT_AUTO,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("sense.kbd-light-auto"),
        ServiceBinding::One(ServiceRole::Sense),
        &[
            lctrl_core::required(Dependency::SmartSense),
            lctrl_core::optional(Dependency::WindowsHpd),
        ],
        DispatchSupport::Unsupported(
            "keyboard-light auto requires the Vantage contract RPC channel, which is not recovered",
        ),
    ),
    CommandCapability::new(
        CommandId::AUDIO_DOLBY,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("audio.dolby"),
        ServiceBinding::One(ServiceRole::Audio),
        &[lctrl_core::required(Dependency::DolbyAudioSdk)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::AUDIO_NOISE_CANCEL,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("audio.noise-cancel"),
        ServiceBinding::One(ServiceRole::Audio),
        &[lctrl_core::required(Dependency::NoiseCancellationSdk)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_LIST,
        CommandAxes::read_only(StateLayer::BiosPersistent),
        FeatureBinding::Unprobed("bios"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_GET,
        CommandAxes::read_only(StateLayer::BiosPersistent),
        FeatureBinding::Unprobed("bios"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_SET,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Unprobed("bios"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_SAVE,
        CommandAxes::write_only(
            StateLayer::BiosPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("bios.save.global-buffer"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported("standalone global BIOS-buffer save is not supported"),
    ),
    CommandCapability::new(
        CommandId::BIOS_DISCARD,
        CommandAxes::write_only(
            StateLayer::BiosPersistent,
            Readback::NotApplicable,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("bios.discard"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_DEFAULTS,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("bios.defaults"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported(
            "experimental or underspecified BIOS defaults are unavailable",
        ),
    ),
    CommandCapability::new(
        CommandId::BIOS_PASSWORD_STATUS,
        CommandAxes::read_only(StateLayer::BiosPersistent),
        FeatureBinding::Unprobed("bios.password.status"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::BIOS_PASSWORD_SET,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("bios.password.set"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported("SetBiosPassword parameter encoding is underspecified"),
    ),
    CommandCapability::new(
        CommandId::BIOS_PASSWORD_CLEAR,
        CommandAxes::mutation(
            StateLayer::BiosPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Unprobed("bios.password.clear"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported("SetBiosPassword parameter encoding is underspecified"),
    ),
    CommandCapability::new(
        CommandId::BIOS_PASSWORD_VERIFY,
        CommandAxes::read_only(StateLayer::BiosPersistent),
        FeatureBinding::Unprobed("bios.password.verify"),
        ServiceBinding::One(ServiceRole::Bios),
        &[lctrl_core::required(Dependency::LenovoFirmwareWmi)],
        DispatchSupport::Unsupported("SetBiosPassword parameter encoding is underspecified"),
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_DETECT,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Capability("magicbay.inventory"),
        ServiceBinding::One(ServiceRole::MagicBay),
        &[
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::LinuxSysfs),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_LTE_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("magicbay.lte"),
        ServiceBinding::One(ServiceRole::MobileBroadband),
        &[
            lctrl_core::required(Dependency::Win32MobileBroadband),
            lctrl_core::required(Dependency::LinuxModemManager),
        ],
        DispatchSupport::Unsupported("standard MBIM control is not attached"),
    )
    .with_hardware(HardwareRequirement::MagicBay),
    CommandCapability::new(
        CommandId::MAGICBAY_LTE_CONNECT,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("magicbay.lte"),
        ServiceBinding::One(ServiceRole::MobileBroadband),
        &[
            lctrl_core::required(Dependency::Win32MobileBroadband),
            lctrl_core::required(Dependency::LinuxModemManager),
        ],
        DispatchSupport::Unsupported("standard MBIM control is not attached"),
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_LTE_DISCONNECT,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("magicbay.lte"),
        ServiceBinding::One(ServiceRole::MobileBroadband),
        &[
            lctrl_core::required(Dependency::Win32MobileBroadband),
            lctrl_core::required(Dependency::LinuxModemManager),
        ],
        DispatchSupport::Unsupported("standard MBIM control is not attached"),
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_LTE_APN,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("magicbay.lte"),
        ServiceBinding::One(ServiceRole::MobileBroadband),
        &[
            lctrl_core::required(Dependency::Win32MobileBroadband),
            lctrl_core::required(Dependency::LinuxModemManager),
        ],
        DispatchSupport::Unsupported("standard MBIM control is not attached"),
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_CAM,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("magicbay.cam"),
        ServiceBinding::One(ServiceRole::MagicBayCamera),
        &[
            lctrl_core::required(Dependency::Win32MediaFoundation),
            lctrl_core::required(Dependency::LinuxV4l2),
        ],
        DispatchSupport::Unsupported("standard UVC camera control is not attached"),
    ),
    CommandCapability::new(
        CommandId::MAGICBAY_DISPLAY,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("magicbay.display"),
        ServiceBinding::One(ServiceRole::MagicBayDisplay),
        &[
            lctrl_core::required(Dependency::Win32DisplayApi),
            lctrl_core::required(Dependency::LinuxDrm),
        ],
        DispatchSupport::Unsupported("standard display control is not attached"),
    )
    .with_hardware(HardwareRequirement::MagicBay),
    CommandCapability::new(
        CommandId::MAGICBAY_WATCH,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::ServiceOnly,
        ServiceBinding::One(ServiceRole::Events),
        &[
            lctrl_core::required(Dependency::Win32DeviceNotifications),
            lctrl_core::required(Dependency::LinuxUdev),
        ],
        DispatchSupport::Unsupported("no documented MagicBay event class exists"),
    ),
    CommandCapability::new(
        CommandId::OSD_ENABLE,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("osd.enable"),
        ServiceBinding::One(ServiceRole::Osd),
        &[
            lctrl_core::required(Dependency::LocalDaemon),
            lctrl_core::optional(Dependency::LenovoUtilityService),
            lctrl_core::optional(Dependency::LenovoFnAndFunctionKeys),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::OSD_DISABLE,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("osd.disable"),
        ServiceBinding::One(ServiceRole::Osd),
        &[
            lctrl_core::required(Dependency::LocalDaemon),
            lctrl_core::optional(Dependency::LenovoUtilityService),
            lctrl_core::optional(Dependency::LenovoFnAndFunctionKeys),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::OSD_TEST,
        CommandAxes::action(StateLayer::Runtime),
        FeatureBinding::Unprobed("osd.test"),
        ServiceBinding::One(ServiceRole::Osd),
        &[
            lctrl_core::required(Dependency::LocalDaemon),
            lctrl_core::optional(Dependency::LenovoUtilityService),
            lctrl_core::optional(Dependency::LenovoFnAndFunctionKeys),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::OSD_SHOW,
        CommandAxes::action(StateLayer::Runtime),
        FeatureBinding::Unprobed("osd.show"),
        ServiceBinding::One(ServiceRole::Osd),
        &[
            lctrl_core::required(Dependency::LocalDaemon),
            lctrl_core::optional(Dependency::LenovoUtilityService),
            lctrl_core::optional(Dependency::LenovoFnAndFunctionKeys),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::HOTKEY_LIST,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("hotkey.list"),
        ServiceBinding::One(ServiceRole::Hotkey),
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::HOTKEY_REGISTER,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("hotkey.register"),
        ServiceBinding::One(ServiceRole::Hotkey),
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::HOTKEY_TRIGGER,
        CommandAxes::action(StateLayer::Runtime),
        FeatureBinding::Unprobed("hotkey.trigger"),
        ServiceBinding::One(ServiceRole::Hotkey),
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::HOTKEY_UNREGISTER,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Unverified,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("hotkey.unregister"),
        ServiceBinding::One(ServiceRole::Hotkey),
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_CHECK,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Unprobed("update.capability"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::Win32WinTrust),
            lctrl_core::optional(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Unsupported(
            "remote update checking has no authenticated catalog contract",
        ),
    ),
    CommandCapability::new(
        CommandId::UPDATE_LOCAL_INSTALL,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("update.local_driver"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::Win32WinTrust),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_LOCAL_ROLLBACK,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Capability("update.local_driver.rollback"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_LOCAL_HISTORY,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::Capability("update.local_driver.policy"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_LOCAL_IGNORE,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("update.local_driver.policy"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_LOCAL_SCHEDULE,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Capability("update.local_driver.policy"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::UPDATE_DOWNLOAD,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Unverified,
            Rollback::Transactional,
        ),
        FeatureBinding::Unprobed("update.download.without-authenticated-manifest"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::AuthenticatedUpdateCatalog),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Unsupported("no authenticated remote update manifest is attached"),
    ),
    CommandCapability::new(
        CommandId::UPDATE_INSTALL,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Unprobed("update.install.without-authenticated-manifest"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::AuthenticatedUpdateCatalog),
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Unsupported("no authenticated remote update manifest is attached"),
    ),
    CommandCapability::new(
        CommandId::UPDATE_HISTORY,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::Unprobed("update.history.without-catalog"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Unsupported("no remote update catalog is attached"),
    ),
    CommandCapability::new(
        CommandId::UPDATE_IGNORE,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("update.ignore.without-catalog"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Unsupported("no remote update catalog is attached"),
    ),
    CommandCapability::new(
        CommandId::UPDATE_ROLLBACK,
        CommandAxes::mutation(
            StateLayer::NotApplicable,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Unprobed("update.rollback.without-install-ledger"),
        ServiceBinding::One(ServiceRole::Update),
        &[
            lctrl_core::required(Dependency::Win32SetupApi),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Unsupported("no remote update install ledger is attached"),
    ),
    CommandCapability::new(
        CommandId::UPDATE_SCHEDULE,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("update.schedule.without-catalog"),
        ServiceBinding::One(ServiceRole::Update),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Unsupported("no remote update catalog is attached"),
    ),
    CommandCapability::new(
        CommandId::SCAN_LIST,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Capability("diagnostics.inventory"),
        ServiceBinding::One(ServiceRole::Diagnostics),
        &[
            lctrl_core::required(Dependency::PublicWin32Cim),
            lctrl_core::required(Dependency::LinuxDiagnosticTools),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SCAN_RUN,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Capability("diagnostics.inventory"),
        ServiceBinding::One(ServiceRole::Diagnostics),
        &[
            lctrl_core::required(Dependency::PublicWin32Cim),
            lctrl_core::required(Dependency::LinuxDiagnosticTools),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::WIFI_SECURITY,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::Unprobed("wifi.security"),
        ServiceBinding::One(ServiceRole::Wifi),
        &[lctrl_core::required(Dependency::Win32ProcessApi)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::FIX_RULES,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::LocalOnly,
        ServiceBinding::Local,
        &[],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::FIX_SCAN,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::Unprobed("machinefix.scan"),
        ServiceBinding::One(ServiceRole::MachineFix),
        &[lctrl_core::required(Dependency::Win32Scm)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::FIX_RUN,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("machinefix.run"),
        ServiceBinding::One(ServiceRole::MachineFix),
        &[
            lctrl_core::required(Dependency::Win32Scm),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::FIX_TRACK,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::Unprobed("machinefix.track"),
        ServiceBinding::One(ServiceRole::MachineFix),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SCENE_LIST,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::LocalOnly,
        ServiceBinding::Local,
        &[],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SCENE_TEST,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::PreviousState,
        ),
        FeatureBinding::Unprobed("scene.test"),
        ServiceBinding::One(ServiceRole::ScenePolicy),
        &[
            lctrl_core::required(Dependency::Win32Scm),
            lctrl_core::required(Dependency::LocalFilesystem),
        ],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SCENE_TRACK,
        CommandAxes::read_only(StateLayer::LocalPersistent),
        FeatureBinding::Unprobed("scene.track"),
        ServiceBinding::One(ServiceRole::ScenePolicy),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SNAPSHOT_CAPTURE,
        CommandAxes::mutation(
            StateLayer::LocalPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::Dynamic(SNAPSHOT_FEATURES),
        ServiceBinding::Composite(SNAPSHOT_SERVICES),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SNAPSHOT_DIFF,
        CommandAxes::read_only(StateLayer::Composite),
        FeatureBinding::Dynamic(SNAPSHOT_FEATURES),
        ServiceBinding::Composite(SNAPSHOT_SERVICES),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::SNAPSHOT_RESTORE,
        CommandAxes::mutation(
            StateLayer::Composite,
            Readback::Required,
            Rollback::Transactional,
        ),
        FeatureBinding::Dynamic(SNAPSHOT_FEATURES),
        ServiceBinding::Composite(SNAPSHOT_SERVICES),
        &[lctrl_core::required(Dependency::LocalFilesystem)],
        DispatchSupport::Implemented,
    ),
    CommandCapability::new(
        CommandId::DAEMON_START,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Required,
            Rollback::Unavailable,
        ),
        FeatureBinding::ServiceOnly,
        ServiceBinding::External,
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::External,
    ),
    CommandCapability::new(
        CommandId::DAEMON_STOP,
        CommandAxes::mutation(
            StateLayer::Runtime,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::ServiceOnly,
        ServiceBinding::External,
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::External,
    ),
    CommandCapability::new(
        CommandId::DAEMON_STATUS,
        CommandAxes::read_only(StateLayer::Runtime),
        FeatureBinding::ServiceOnly,
        ServiceBinding::External,
        &[lctrl_core::required(Dependency::LocalDaemon)],
        DispatchSupport::External,
    ),
    CommandCapability::new(
        CommandId::DAEMON_INSTALL,
        CommandAxes::mutation(
            StateLayer::OsPersistent,
            Readback::Unverified,
            Rollback::Unavailable,
        ),
        FeatureBinding::ServiceOnly,
        ServiceBinding::External,
        &[
            lctrl_core::required(Dependency::LocalDaemon),
            lctrl_core::required(Dependency::LocalFilesystem),
            lctrl_core::required(Dependency::Win32TaskScheduler),
            lctrl_core::required(Dependency::LinuxSystemd),
        ],
        DispatchSupport::External,
    ),
    CommandCapability::new(
        CommandId::COMPLETIONS,
        CommandAxes::read_only(StateLayer::NotApplicable),
        FeatureBinding::LocalOnly,
        ServiceBinding::Local,
        &[],
        DispatchSupport::Implemented,
    ),
];

#[must_use]
pub const fn command_registry() -> &'static [CommandCapability] {
    COMMAND_REGISTRY
}

#[must_use]
pub fn command_capability(command: CommandId) -> Option<&'static CommandCapability> {
    COMMAND_REGISTRY
        .iter()
        .find(|capability| capability.command == command)
}

#[must_use]
pub fn command_capabilities(command: CommandId) -> Vec<&'static CommandCapability> {
    COMMAND_REGISTRY
        .iter()
        .filter(|capability| capability.command.as_str() == command.as_str())
        .collect()
}

#[must_use]
pub fn command_capability_for(selector: CommandSelector) -> Option<&'static CommandCapability> {
    COMMAND_REGISTRY
        .iter()
        .find(|capability| capability.selector() == selector)
}

/// Minimal vendor-service switch checklist for the selected command layers.
///
/// This is never a recommendation to disable a component. Commands which are
/// unsupported are still listed so their declared dependency is not mistaken
/// for a replacement.
#[must_use]
pub fn minimal_dependency_checklist(selected: &[CommandSelector]) -> Vec<DependencySwitch> {
    let mut selected: Vec<_> = selected.to_vec();
    selected.sort();
    selected.dedup();

    let mut switches = BTreeMap::new();
    for &dependency in Dependency::VENDOR_SWITCHES {
        switches.insert(
            dependency,
            DependencySwitch {
                dependency,
                required_by: Vec::new(),
                optional_for: Vec::new(),
                unsupported_for: Vec::new(),
            },
        );
    }

    for selector in &selected {
        let Some(entry) = command_capability_for(*selector) else {
            continue;
        };
        let Some(&usage) = entry
            .dependencies
            .iter()
            .find(|item| item.dependency.platform() == Some(lctrl_core::Platform::Windows))
        else {
            continue;
        };
        let Some(switch) = switches.get_mut(&usage.dependency) else {
            continue;
        };
        match (entry.dispatch, usage.usage) {
            (DispatchSupport::Implemented, DependencyUse::Required) => {
                switch.required_by.push(*selector);
            }
            (DispatchSupport::Implemented, DependencyUse::Optional) => {
                switch.optional_for.push(*selector);
            }
            (DispatchSupport::Unsupported(_) | DispatchSupport::External, _) => {
                switch.unsupported_for.push(*selector);
            }
        }
    }

    Dependency::VENDOR_SWITCHES
        .iter()
        .filter_map(|dependency| switches.remove(dependency))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencySwitch {
    pub dependency: Dependency,
    pub required_by: Vec<CommandSelector>,
    pub optional_for: Vec<CommandSelector>,
    pub unsupported_for: Vec<CommandSelector>,
}

impl DependencySwitch {
    #[must_use]
    pub fn required(&self) -> bool {
        !self.required_by.is_empty()
    }

    #[must_use]
    pub fn action(&self) -> SwitchAction {
        if self.required() {
            SwitchAction::Keep
        } else {
            SwitchAction::Review
        }
    }

    #[must_use]
    pub fn reason(&self) -> &'static str {
        if self.required() {
            "a selected supported command requires this component"
        } else if !self.unsupported_for.is_empty() {
            "a selected command is unsupported or external; removal does not create replacement support"
        } else if !self.optional_for.is_empty() {
            "only optional or alternate paths reference this component"
        } else {
            "no selected registry entry references this component"
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwitchAction {
    Keep,
    /// Never a safe automatic disable action; independent feature review is required.
    Review,
}
