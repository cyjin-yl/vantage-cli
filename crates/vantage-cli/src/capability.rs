use lctrl_core::{CapabilitySet, CommandSelector, HardwarePresence, HardwareRequirement};
use lctrl_hal::HardwareObservation;
use lctrl_hal::{CommandCapability, DispatchSupport, FeatureBinding, ServiceBinding, ServiceRole};

/// Map a parsed CLI command to the exact registry selector.
///
/// Layer flags are validated first, so an invalid privacy runtime/persistent
/// combination maps to `None` rather than a guessed row.
#[must_use]
pub fn command_selector(command: &crate::Command) -> Option<CommandSelector> {
    use crate::Command;

    let id = |layer| {
        Some(CommandSelector {
            command: command_id(command)?,
            layer,
        })
    };
    match command {
        Command::Privacy {
            command: Some(privacy_command),
        } => {
            let layer = privacy_layer(privacy_command)?;
            match layer {
                "runtime" => id(lctrl_core::StateLayer::Runtime),
                "persistent" => id(lctrl_core::StateLayer::BiosPersistent),
                _ => None,
            }
        }
        Command::Privacy { command: None } => None,
        _ => id(command_layer(command)),
    }
}

#[must_use]
pub fn command_layer(command: &crate::Command) -> lctrl_core::StateLayer {
    use crate::Command;
    use crate::{
        BatteryCommand, BiosCommand, LteCommand, PanelCommand, PerfCommand, PrivacyCommand,
    };
    use lctrl_core::StateLayer;

    match command {
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { .. }),
        } => StateLayer::Runtime,
        Command::Battery {
            command: Some(BatteryCommand::Thresholds { .. }),
        } => StateLayer::Runtime,
        Command::Battery {
            command: Some(BatteryCommand::ExtremeLife { .. }),
        } => StateLayer::Runtime,
        Command::Battery {
            command: Some(BatteryCommand::NightCharge { .. }),
        } => StateLayer::Runtime,
        Command::Battery {
            command: Some(BatteryCommand::TemporaryMode),
        } => StateLayer::Runtime,
        Command::Battery {
            command: Some(BatteryCommand::Watch),
        } => StateLayer::Runtime,
        Command::Battery {
            command: None | Some(BatteryCommand::Status | BatteryCommand::Adapter),
        } => StateLayer::Runtime,
        Command::Usb { .. } => StateLayer::Runtime, // registry selector validation uses explicit helpers for persistent flag
        Command::Power {
            command: None | Some(_),
        } => StateLayer::OsPersistent,
        Command::Perf {
            command: Some(PerfCommand::Pl1 { .. } | PerfCommand::Pl2 { .. }),
        } => StateLayer::Runtime,
        Command::Perf { .. } => StateLayer::Runtime,
        Command::Tune {
            command: Some(crate::TuneCommand::Profile { command: None }),
        } => StateLayer::NotApplicable,
        Command::Tune {
            command: Some(crate::TuneCommand::Profile { .. }),
        } => StateLayer::Composite,
        Command::Tune {
            command: Some(crate::TuneCommand::Epp { .. } | crate::TuneCommand::Turbo { .. }),
        } => StateLayer::PlatformDependent,
        Command::Tune {
            command: Some(crate::TuneCommand::Telemetry { .. } | crate::TuneCommand::Watch),
        } => StateLayer::Runtime,
        Command::Tune {
            command: Some(crate::TuneCommand::Pl1 { .. } | crate::TuneCommand::Restore),
        } => StateLayer::Composite,
        Command::Tune { command: None } => StateLayer::NotApplicable,
        Command::Kbd {
            command: Some(crate::KbdCommand::Backlight { .. }),
        } => StateLayer::Runtime,
        Command::Kbd {
            command:
                Some(
                    crate::KbdCommand::Fnlock { .. }
                    | crate::KbdCommand::FnCtrlSwap { .. }
                    | crate::KbdCommand::Winlock { .. },
                ),
        } => StateLayer::BiosPersistent,
        Command::Kbd { command: None } => StateLayer::NotApplicable,
        Command::Panel {
            command: Some(PanelCommand::Rate { .. }),
        } => StateLayer::Runtime,
        Command::Panel { .. } => StateLayer::Runtime,
        Command::Privacy {
            command: Some(PrivacyCommand::Fingerprint { .. } | PrivacyCommand::Status),
        } => StateLayer::Composite,
        Command::Privacy { .. } => StateLayer::Composite,
        Command::Sense { .. } => StateLayer::Runtime,
        Command::Audio { .. } => StateLayer::Runtime,
        Command::Bios {
            command:
                Some(
                    BiosCommand::Set { .. }
                    | BiosCommand::Save
                    | BiosCommand::Discard
                    | BiosCommand::Defaults
                    | BiosCommand::Password { command: Some(_) },
                ),
        } => StateLayer::BiosPersistent,
        Command::Bios { .. } => StateLayer::BiosPersistent,
        Command::Magicbay {
            command:
                Some(crate::MagicbayCommand::Lte {
                    command: Some(LteCommand::Apn { .. }),
                }),
        } => StateLayer::OsPersistent,
        Command::Magicbay { command: Some(_) } => StateLayer::Runtime,
        Command::Magicbay { command: None } => StateLayer::NotApplicable,
        Command::Osd { command: None } => StateLayer::NotApplicable,
        Command::Osd {
            command: Some(crate::OsdCommand::Test),
        } => StateLayer::Runtime,
        Command::Osd { command: Some(_) } => StateLayer::OsPersistent,
        Command::Update { .. } => StateLayer::LocalPersistent,
        Command::Snapshot {
            command: Some(crate::SnapshotCommand::Diff),
        } => StateLayer::Composite,
        Command::Snapshot { .. } => StateLayer::LocalPersistent,
        Command::Daemon {
            command: Some(crate::DaemonCommand::Status),
        } => StateLayer::Runtime,
        Command::Daemon {
            command: Some(crate::DaemonCommand::Install),
        } => StateLayer::OsPersistent,
        Command::Daemon { .. } => StateLayer::Runtime,
        Command::Scan { .. } => StateLayer::NotApplicable,
        Command::Touchpad {
            state: crate::TouchpadAction::Status | crate::TouchpadAction::Recover,
        } => StateLayer::Runtime,
        Command::Touchpad { .. } => StateLayer::PlatformDependent,
        Command::Info | Command::Doctor | Command::Capability | Command::Completions { .. } => {
            StateLayer::Runtime
        }
    }
}

#[must_use]
pub fn privacy_layer(command: &crate::PrivacyCommand) -> Option<&'static str> {
    match command {
        crate::PrivacyCommand::Cam {
            runtime: true,
            persistent: false,
            ..
        }
        | crate::PrivacyCommand::Mic {
            runtime: true,
            persistent: false,
            ..
        } => Some("runtime"),
        crate::PrivacyCommand::Cam {
            runtime: false,
            persistent: true,
            ..
        }
        | crate::PrivacyCommand::Mic {
            runtime: false,
            persistent: true,
            ..
        } => Some("persistent"),
        crate::PrivacyCommand::Fingerprint { .. } | crate::PrivacyCommand::Status => None,
        _ => None,
    }
}

#[must_use]
pub fn command_id(command: &crate::Command) -> Option<lctrl_core::CommandId> {
    use crate::Command;
    use crate::{
        AudioCommand, BatteryCommand, BiosCommand, DaemonCommand, LteCommand, MagicbayCommand,
        OsdCommand, PanelCommand, PasswordCommand, PerfCommand, PrivacyCommand, ProfileCommand,
        SchemeCommand, SenseCommand, SnapshotCommand, TempCommand, TuneCommand, UpdateCommand,
        UsbCommand,
    };
    use lctrl_core::CommandId;

    Some(match command {
        Command::Info => CommandId::INFO,
        Command::Doctor => CommandId::DOCTOR,
        Command::Battery {
            command: None | Some(BatteryCommand::Status),
        } => CommandId::BATTERY_STATUS,
        Command::Battery {
            command: Some(BatteryCommand::Adapter),
        } => CommandId::BATTERY_ADAPTER,
        Command::Battery {
            command: Some(BatteryCommand::ChargeMode { .. }),
        } => CommandId::BATTERY_CHARGE_MODE,
        Command::Battery {
            command: Some(BatteryCommand::Thresholds { .. }),
        } => CommandId::BATTERY_THRESHOLDS,
        Command::Battery {
            command: Some(BatteryCommand::ExtremeLife { .. }),
        } => CommandId::BATTERY_EXTREME_LIFE,
        Command::Battery {
            command: Some(BatteryCommand::NightCharge { .. }),
        } => CommandId::BATTERY_NIGHT_CHARGE,
        Command::Battery {
            command: Some(BatteryCommand::TemporaryMode),
        } => CommandId::BATTERY_TEMPORARY_MODE,
        Command::Battery {
            command: Some(BatteryCommand::Watch),
        } => CommandId::BATTERY_WATCH,
        Command::Usb {
            command:
                Some(UsbCommand::AlwaysOn {
                    persistent: false, ..
                }),
        } => CommandId::USB_ALWAYS_ON,
        Command::Usb {
            command: Some(UsbCommand::AlwaysOn {
                persistent: true, ..
            }),
        } => CommandId::USB_ALWAYS_ON,
        Command::Usb {
            command:
                Some(UsbCommand::ChargeOnBattery {
                    persistent: false, ..
                }),
        } => CommandId::USB_CHARGE_ON_BATTERY,
        Command::Usb {
            command:
                Some(UsbCommand::ChargeOnBattery {
                    persistent: true, ..
                }),
        } => CommandId::USB_CHARGE_ON_BATTERY,
        Command::Usb { command: None } => return None,
        Command::Power {
            command:
                None
                | Some(crate::PowerCommand::Scheme {
                    command: Some(SchemeCommand::Get { .. }),
                }),
        } => CommandId::POWER_SCHEME_ACTIVE,
        Command::Power {
            command: Some(crate::PowerCommand::Scheme { command: None }),
        } => CommandId::POWER_SCHEME_ACTIVE,
        Command::Power {
            command:
                Some(crate::PowerCommand::Scheme {
                    command: Some(SchemeCommand::List),
                }),
        } => CommandId::POWER_SCHEME_LIST,
        Command::Power {
            command:
                Some(crate::PowerCommand::Scheme {
                    command: Some(SchemeCommand::Apply { .. }),
                }),
        } => CommandId::POWER_SCHEME_APPLY,
        Command::Power {
            command:
                Some(crate::PowerCommand::Scheme {
                    command: Some(SchemeCommand::Set { .. }),
                }),
        } => CommandId::POWER_SCHEME_SET,
        Command::Power {
            command: Some(crate::PowerCommand::SaverOnce),
        } => CommandId::POWER_SAVER_ONCE,
        Command::Perf {
            command:
                None
                | Some(PerfCommand::Fan {
                    command: None | Some(crate::FanCommand::Status),
                }),
        } => CommandId::PERF_FAN_STATUS,
        Command::Perf {
            command: Some(PerfCommand::Mode { .. }),
        } => CommandId::PERF_MODE,
        Command::Perf {
            command:
                Some(PerfCommand::Fan {
                    command: Some(crate::FanCommand::Auto),
                }),
        } => CommandId::PERF_FAN_AUTO,
        Command::Perf {
            command:
                Some(PerfCommand::Fan {
                    command: Some(crate::FanCommand::Manual),
                }),
        } => CommandId::PERF_FAN_MANUAL,
        Command::Perf {
            command:
                Some(PerfCommand::Fan {
                    command: Some(crate::FanCommand::Fullspeed),
                }),
        } => CommandId::PERF_FAN_FULLSPEED,
        Command::Perf {
            command:
                Some(PerfCommand::Fan {
                    command: Some(crate::FanCommand::Smart),
                }),
        } => CommandId::PERF_FAN_SMART,
        Command::Perf {
            command:
                Some(PerfCommand::Fan {
                    command: Some(crate::FanCommand::Curve { .. }),
                }),
        } => CommandId::PERF_FAN_CURVE,
        Command::Perf {
            command: Some(PerfCommand::Temp { command: None }),
        } => CommandId::PERF_TEMP_LIST,
        Command::Perf {
            command:
                Some(PerfCommand::Temp {
                    command: Some(TempCommand::Read { .. }),
                }),
        } => CommandId::PERF_TEMP_READ,
        Command::Perf {
            command:
                Some(PerfCommand::Temp {
                    command: Some(TempCommand::Watch),
                }),
        } => CommandId::PERF_TEMP_WATCH,
        Command::Perf {
            command: Some(PerfCommand::Pl1 { .. }),
        } => CommandId::PERF_PL1,
        Command::Perf {
            command: Some(PerfCommand::Pl2 { .. }),
        } => CommandId::PERF_PL2,
        Command::Perf {
            command: Some(PerfCommand::Top),
        } => CommandId::PERF_TOP,
        Command::Perf {
            command: Some(PerfCommand::Boost { .. }),
        } => CommandId::PERF_BOOST,
        Command::Perf {
            command: Some(PerfCommand::Throttle { .. }),
        } => CommandId::PERF_THROTTLE,
        Command::Tune {
            command:
                Some(TuneCommand::Profile {
                    command: Some(ProfileCommand::List),
                }),
        } => CommandId::TUNE_PROFILE_LIST,
        Command::Tune {
            command:
                Some(TuneCommand::Profile {
                    command: Some(ProfileCommand::Show { .. }),
                }),
        } => CommandId::TUNE_PROFILE_SHOW,
        Command::Tune {
            command:
                Some(TuneCommand::Profile {
                    command: Some(ProfileCommand::Apply { .. }),
                }),
        } => CommandId::TUNE_PROFILE_APPLY,
        Command::Tune {
            command: Some(TuneCommand::Pl1 { .. }),
        } => CommandId::TUNE_PL1,
        Command::Tune {
            command: Some(TuneCommand::Epp { .. }),
        } => CommandId::TUNE_EPP,
        Command::Tune {
            command: Some(TuneCommand::Turbo { .. }),
        } => CommandId::TUNE_TURBO,
        Command::Tune {
            command: Some(TuneCommand::Restore),
        } => CommandId::TUNE_RESTORE,
        Command::Tune {
            command: Some(TuneCommand::Telemetry { .. }),
        } => CommandId::TUNE_TELEMETRY,
        Command::Tune {
            command: Some(TuneCommand::Watch),
        } => CommandId::TUNE_WATCH,
        Command::Tune {
            command: None | Some(TuneCommand::Profile { command: None }),
        } => return None,
        Command::Kbd {
            command: Some(crate::KbdCommand::Backlight { .. }),
        } => CommandId::KBD_BACKLIGHT,
        Command::Kbd {
            command: Some(crate::KbdCommand::Fnlock { .. }),
        } => CommandId::KBD_FNLOCK,
        Command::Kbd {
            command: Some(crate::KbdCommand::FnCtrlSwap { .. }),
        } => CommandId::KBD_FN_CTRL_SWAP,
        Command::Kbd {
            command: Some(crate::KbdCommand::Winlock { .. }),
        } => CommandId::KBD_WINLOCK,
        Command::Kbd { command: None } => return None,
        Command::Touchpad {
            state: crate::TouchpadAction::Status,
        } => CommandId::TOUCHPAD_STATUS,
        Command::Touchpad {
            state: crate::TouchpadAction::Recover,
        } => CommandId::TOUCHPAD_RECOVER,
        Command::Touchpad { .. } => CommandId::TOUCHPAD,
        Command::Panel {
            command: Some(PanelCommand::Brightness { percent: None }),
        } => CommandId::PANEL_BRIGHTNESS_STATUS,
        Command::Panel {
            command: Some(PanelCommand::Brightness { percent: Some(_) }),
        } => CommandId::PANEL_BRIGHTNESS_SET,
        Command::Panel {
            command: Some(PanelCommand::Rate { .. }),
        } => CommandId::PANEL_RATE,
        Command::Panel {
            command: Some(PanelCommand::Color { .. }),
        } => CommandId::PANEL_COLOR,
        Command::Panel {
            command: Some(PanelCommand::SuperResolution { .. }),
        } => CommandId::PANEL_SUPER_RESOLUTION,
        Command::Panel {
            command: Some(PanelCommand::Overdrive { .. }),
        } => CommandId::PANEL_OVERDRIVE,
        Command::Panel {
            command: Some(PanelCommand::EyeCare { .. }),
        } => CommandId::PANEL_EYE_CARE,
        Command::Panel { command: None } => return None,
        Command::Privacy {
            command: Some(PrivacyCommand::Cam { .. }),
        } => CommandId::PRIVACY_CAM,
        Command::Privacy {
            command: Some(PrivacyCommand::Mic { .. }),
        } => CommandId::PRIVACY_MIC,
        Command::Privacy {
            command: Some(PrivacyCommand::Fingerprint { .. }),
        } => CommandId::PRIVACY_FINGERPRINT,
        Command::Privacy {
            command: Some(PrivacyCommand::Status),
        } => CommandId::PRIVACY_STATUS,
        Command::Privacy { command: None } => return None,
        Command::Sense {
            command: Some(SenseCommand::Status),
        } => CommandId::SENSE_STATUS,
        Command::Sense {
            command: Some(SenseCommand::LockOnLeave { .. }),
        } => CommandId::SENSE_LOCK_ON_LEAVE,
        Command::Sense {
            command: Some(SenseCommand::WakeOnApproach { .. }),
        } => CommandId::SENSE_WAKE_ON_APPROACH,
        Command::Sense {
            command: Some(SenseCommand::PauseVideo { .. }),
        } => CommandId::SENSE_PAUSE_VIDEO,
        Command::Sense {
            command: Some(SenseCommand::AttentionTracking { .. }),
        } => CommandId::SENSE_ATTENTION_TRACKING,
        Command::Sense {
            command: Some(SenseCommand::KbdLightAuto { .. }),
        } => CommandId::SENSE_KBD_LIGHT_AUTO,
        Command::Sense { command: None } => return None,
        Command::Audio {
            command: Some(AudioCommand::Dolby { .. }),
        } => CommandId::AUDIO_DOLBY,
        Command::Audio {
            command: Some(AudioCommand::NoiseCancel { .. }),
        } => CommandId::AUDIO_NOISE_CANCEL,
        Command::Audio { command: None } => return None,
        Command::Bios {
            command: None | Some(BiosCommand::List),
        } => CommandId::BIOS_LIST,
        Command::Bios {
            command: Some(BiosCommand::Get { .. }),
        } => CommandId::BIOS_GET,
        Command::Bios {
            command: Some(BiosCommand::Set { .. }),
        } => CommandId::BIOS_SET,
        Command::Bios {
            command: Some(BiosCommand::Save),
        } => CommandId::BIOS_SAVE,
        Command::Bios {
            command: Some(BiosCommand::Discard),
        } => CommandId::BIOS_DISCARD,
        Command::Bios {
            command: Some(BiosCommand::Defaults),
        } => CommandId::BIOS_DEFAULTS,
        Command::Bios {
            command:
                Some(BiosCommand::Password {
                    command: Some(PasswordCommand::Status),
                }),
        } => CommandId::BIOS_PASSWORD_STATUS,
        Command::Bios {
            command:
                Some(BiosCommand::Password {
                    command: Some(PasswordCommand::Set { .. }),
                }),
        } => CommandId::BIOS_PASSWORD_SET,
        Command::Bios {
            command:
                Some(BiosCommand::Password {
                    command: Some(PasswordCommand::Clear),
                }),
        } => CommandId::BIOS_PASSWORD_CLEAR,
        Command::Bios {
            command:
                Some(BiosCommand::Password {
                    command: Some(PasswordCommand::Verify { .. }),
                }),
        } => CommandId::BIOS_PASSWORD_VERIFY,
        Command::Bios {
            command: Some(BiosCommand::Password { command: None }),
        } => return None,
        Command::Magicbay {
            command: Some(MagicbayCommand::Detect),
        } => CommandId::MAGICBAY_DETECT,
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Status),
                }),
        } => CommandId::MAGICBAY_LTE_STATUS,
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Connect),
                }),
        } => CommandId::MAGICBAY_LTE_CONNECT,
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Disconnect),
                }),
        } => CommandId::MAGICBAY_LTE_DISCONNECT,
        Command::Magicbay {
            command:
                Some(MagicbayCommand::Lte {
                    command: Some(LteCommand::Apn { .. }),
                }),
        } => CommandId::MAGICBAY_LTE_APN,
        Command::Magicbay {
            command: Some(MagicbayCommand::Cam),
        } => CommandId::MAGICBAY_CAM,
        Command::Magicbay {
            command: Some(MagicbayCommand::Display),
        } => CommandId::MAGICBAY_DISPLAY,
        Command::Magicbay {
            command: Some(MagicbayCommand::Watch),
        } => CommandId::MAGICBAY_WATCH,
        Command::Magicbay { .. } => return None,
        Command::Osd { command: None } => return None,
        Command::Osd {
            command: Some(OsdCommand::Enable),
        } => CommandId::OSD_ENABLE,
        Command::Osd {
            command: Some(OsdCommand::Disable),
        } => CommandId::OSD_DISABLE,
        Command::Osd {
            command: Some(OsdCommand::Test),
        } => CommandId::OSD_TEST,
        Command::Update {
            command: Some(UpdateCommand::Check { .. }),
        } => CommandId::UPDATE_CHECK,
        Command::Update {
            command: Some(UpdateCommand::Download { .. }),
        } => CommandId::UPDATE_DOWNLOAD,
        Command::Update {
            command: Some(UpdateCommand::Install { .. }),
        } => CommandId::UPDATE_INSTALL,
        Command::Update {
            command: Some(UpdateCommand::History),
        } => CommandId::UPDATE_HISTORY,
        Command::Update {
            command: Some(UpdateCommand::Ignore { .. }),
        } => CommandId::UPDATE_IGNORE,
        Command::Update {
            command: Some(UpdateCommand::Rollback { .. }),
        } => CommandId::UPDATE_ROLLBACK,
        Command::Update {
            command: Some(UpdateCommand::Schedule { .. }),
        } => CommandId::UPDATE_SCHEDULE,
        Command::Update { command: None } => return None,
        Command::Scan {
            command: Some(crate::ScanCommand::List),
        } => CommandId::SCAN_LIST,
        Command::Scan {
            command: Some(crate::ScanCommand::Run { .. }),
        } => CommandId::SCAN_RUN,
        Command::Scan { command: None } => return None,
        Command::Snapshot {
            command: Some(SnapshotCommand::Capture),
        } => CommandId::SNAPSHOT_CAPTURE,
        Command::Snapshot {
            command: Some(SnapshotCommand::Diff),
        } => CommandId::SNAPSHOT_DIFF,
        Command::Snapshot {
            command: Some(SnapshotCommand::Restore),
        } => CommandId::SNAPSHOT_RESTORE,
        Command::Snapshot { command: None } => return None,
        Command::Daemon {
            command: Some(DaemonCommand::Start),
        } => CommandId::DAEMON_START,
        Command::Daemon {
            command: Some(DaemonCommand::Stop),
        } => CommandId::DAEMON_STOP,
        Command::Daemon {
            command: Some(DaemonCommand::Status),
        } => CommandId::DAEMON_STATUS,
        Command::Daemon {
            command: Some(DaemonCommand::Install),
        } => CommandId::DAEMON_INSTALL,
        Command::Daemon { command: None } => return None,
        Command::Capability => return None,
        Command::Completions { .. } => CommandId::COMPLETIONS,
    })
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ObservedCommandCapability {
    pub command: lctrl_core::CommandId,
    pub layer: lctrl_core::StateLayer,
    pub status: ObservedStatus,
    pub axes: lctrl_core::CommandAxes,
    pub feature_ids: Vec<&'static str>,
    pub service_roles: Vec<&'static str>,
    pub dependencies: Vec<lctrl_core::CommandDependency>,
    pub hardware: HardwarePresence,
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedStatus {
    Available,
    Limited,
    Unavailable,
    Unsupported,
    NotApplicable,
    Unverified,
    External,
}

/// Observe every registry row against the actual attached `CommandServices`
/// and the root HAL's returned `CapabilitySet`.
pub fn evaluate_commands(
    hal: &dyn lctrl_hal::Hal,
) -> lctrl_core::Result<Vec<ObservedCommandCapability>> {
    let capabilities = hal.capabilities()?;
    let hardware = HardwareObservation::default();
    Ok(evaluate_commands_with_hardware(
        hal,
        &capabilities,
        hardware,
    ))
}

pub fn evaluate_commands_with_hardware(
    hal: &dyn lctrl_hal::Hal,
    capabilities: &CapabilitySet,
    hardware: lctrl_hal::HardwareObservation,
) -> Vec<ObservedCommandCapability> {
    lctrl_hal::command_registry()
        .iter()
        .map(|entry| evaluate_command(hal, capabilities, entry, hardware))
        .collect()
}

fn evaluate_command(
    hal: &dyn lctrl_hal::Hal,
    capabilities: &CapabilitySet,
    entry: &CommandCapability,
    hardware: lctrl_hal::HardwareObservation,
) -> ObservedCommandCapability {
    let platform = hal.platform();
    let service_roles = service_role_names(entry.service);
    let service = service_presence(entry.service);
    let feature = feature_presence(entry.feature, capabilities, platform);
    let blocked_staging = if entry.axes.write
        && entry.service == ServiceBinding::One(ServiceRole::Bios)
        && matches!(entry.dispatch, DispatchSupport::Implemented)
    {
        capabilities
            .get("bios.staging.owned")
            .filter(|capability| capability.availability == lctrl_core::Availability::Unavailable)
    } else {
        None
    };
    if let Some(capability) = blocked_staging {
        return make(
            entry,
            hardware,
            ObservedStatus::Unavailable,
            capability.detail.clone(),
            service_roles,
            dependencies(entry, platform),
        );
    }
    let status = match (entry.dispatch, service, feature) {
        (_, _, _)
            if entry.hardware == HardwareRequirement::MagicBay
                && hardware.magicbay == HardwarePresence::Absent =>
        {
            ObservedStatus::NotApplicable
        }
        (DispatchSupport::External, _, _) => ObservedStatus::External,
        (DispatchSupport::Unsupported(reason), _, _) => {
            if entry.hardware == HardwareRequirement::MagicBay
                && hardware.magicbay == HardwarePresence::Absent
            {
                return make(
                    entry,
                    hardware,
                    ObservedStatus::NotApplicable,
                    Some(reason.into()),
                    service_roles,
                    dependencies(entry, platform),
                );
            }
            return make(
                entry,
                hardware,
                ObservedStatus::Unsupported,
                Some(reason.into()),
                service_roles,
                dependencies(entry, platform),
            );
        }
        (DispatchSupport::Implemented, _service, FeaturePresence::Unsupported) => {
            ObservedStatus::Unsupported
        }
        (DispatchSupport::Implemented, _service, FeaturePresence::Unavailable) => {
            ObservedStatus::Unavailable
        }
        (DispatchSupport::Implemented, ServicePresence::Unexposed, _) => ObservedStatus::Unverified,
        (DispatchSupport::Implemented, _service, FeaturePresence::Limited) => {
            ObservedStatus::Limited
        }
        (DispatchSupport::Implemented, _service, FeaturePresence::Unverified) => {
            ObservedStatus::Unverified
        }
        (DispatchSupport::Implemented, _service, FeaturePresence::Available) => {
            if entry.hardware != HardwareRequirement::None
                && hardware.magicbay == HardwarePresence::Unknown
            {
                ObservedStatus::Unverified
            } else {
                ObservedStatus::Available
            }
        }
    };
    make(
        entry,
        hardware,
        status,
        None,
        service_roles,
        dependencies(entry, platform),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ServicePresence {
    Present,
    Unexposed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeaturePresence {
    Available,
    Limited,
    Unavailable,
    Unsupported,
    Unverified,
}

fn service_presence(binding: ServiceBinding) -> ServicePresence {
    match binding {
        ServiceBinding::Local => ServicePresence::Present,
        ServiceBinding::One(role) => role_presence(&[role]),
        ServiceBinding::AnyOf(roles) => role_presence(roles),
        ServiceBinding::Composite(roles) => role_presence(roles),
        ServiceBinding::External | ServiceBinding::Unsupported => ServicePresence::Present,
    }
}

fn role_presence(roles: &[ServiceRole]) -> ServicePresence {
    // The current evaluator receives only the root HAL, not the composition
    // root's optional service attachments. Never infer that a hardware
    // service is attached merely because a capability name exists.
    if !roles.is_empty() && roles.iter().all(|role| *role == ServiceRole::Hal) {
        ServicePresence::Present
    } else {
        ServicePresence::Unexposed
    }
}

fn feature_presence(
    binding: FeatureBinding,
    capabilities: &CapabilitySet,
    _platform: lctrl_core::Platform,
) -> FeaturePresence {
    match binding {
        FeatureBinding::LocalOnly => FeaturePresence::Available,
        FeatureBinding::ServiceOnly => FeaturePresence::Unverified,
        FeatureBinding::Unprobed(_) => FeaturePresence::Unverified,
        FeatureBinding::Capability(feature) => capability_presence(feature, capabilities),
        FeatureBinding::AnyOf(features) => {
            let mut limited = false;
            let mut unavailable = false;
            for feature in features {
                match capability_presence(feature, capabilities) {
                    FeaturePresence::Available => return FeaturePresence::Available,
                    FeaturePresence::Limited => limited = true,
                    FeaturePresence::Unavailable => unavailable = true,
                    FeaturePresence::Unsupported => unavailable = true,
                    FeaturePresence::Unverified => {}
                }
            }
            if limited {
                FeaturePresence::Limited
            } else if unavailable {
                FeaturePresence::Unavailable
            } else {
                FeaturePresence::Unverified
            }
        }
        FeatureBinding::Dynamic(features) => {
            let mut available = 0;
            for feature in features {
                match capability_presence(feature, capabilities) {
                    FeaturePresence::Available | FeaturePresence::Limited => available += 1,
                    FeaturePresence::Unavailable | FeaturePresence::Unsupported => continue,
                    FeaturePresence::Unverified => continue,
                }
            }
            if available == features.len() {
                FeaturePresence::Available
            } else if available > 0 {
                FeaturePresence::Limited
            } else {
                FeaturePresence::Unverified
            }
        }
    }
}

fn capability_presence(feature: &str, capabilities: &CapabilitySet) -> FeaturePresence {
    match capabilities.get(feature) {
        None => FeaturePresence::Unverified,
        Some(capability) => match capability.availability {
            lctrl_core::Availability::Available => FeaturePresence::Available,
            lctrl_core::Availability::Limited => FeaturePresence::Limited,
            lctrl_core::Availability::Unavailable => FeaturePresence::Unsupported,
        },
    }
}

fn service_role_names(binding: ServiceBinding) -> Vec<&'static str> {
    match binding {
        ServiceBinding::Local => Vec::new(),
        ServiceBinding::One(role) => vec![role.as_str()],
        ServiceBinding::AnyOf(roles) | ServiceBinding::Composite(roles) => {
            roles.iter().map(|role| role.as_str()).collect()
        }
        ServiceBinding::External | ServiceBinding::Unsupported => Vec::new(),
    }
}

fn dependencies(
    entry: &CommandCapability,
    platform: lctrl_core::Platform,
) -> Vec<lctrl_core::CommandDependency> {
    entry
        .dependencies
        .iter()
        .copied()
        .filter(|dependency| {
            dependency
                .dependency
                .platform()
                .is_none_or(|required| required == platform)
        })
        .collect()
}

fn make(
    entry: &CommandCapability,
    hardware: lctrl_hal::HardwareObservation,
    status: ObservedStatus,
    detail: Option<String>,
    service_roles: Vec<&'static str>,
    dependencies: Vec<lctrl_core::CommandDependency>,
) -> ObservedCommandCapability {
    let feature_ids = match entry.feature {
        FeatureBinding::Capability(feature) | FeatureBinding::Unprobed(feature) => vec![feature],
        FeatureBinding::AnyOf(features) | FeatureBinding::Dynamic(features) => features.to_vec(),
        FeatureBinding::ServiceOnly | FeatureBinding::LocalOnly => Vec::new(),
    };
    ObservedCommandCapability {
        command: entry.command,
        layer: entry.axes.layer,
        status,
        axes: entry.axes,
        feature_ids,
        service_roles,
        dependencies,
        hardware: hardware.magicbay,
        detail,
    }
}
