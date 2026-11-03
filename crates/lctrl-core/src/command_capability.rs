use serde::{Serialize, Serializer};

use crate::Platform;

/// Stable identifier for one parsed CLI command or subcommand.
///
/// Argument values do not create new identifiers. Flags which select a state
/// layer (for example privacy `--runtime` versus `--persistent`) are represented
/// by [`CommandSelector`], not by lossy string matching.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommandId(&'static str);

impl CommandId {
    pub const INFO: Self = Self("info");
    pub const DOCTOR: Self = Self("doctor");

    pub const BATTERY_STATUS: Self = Self("battery.status");
    pub const BATTERY_ADAPTER: Self = Self("battery.adapter");
    pub const BATTERY_CHARGE_MODE: Self = Self("battery.charge-mode");
    pub const BATTERY_THRESHOLDS: Self = Self("battery.thresholds");
    pub const BATTERY_EXTREME_LIFE: Self = Self("battery.extreme-life");
    pub const BATTERY_NIGHT_CHARGE: Self = Self("battery.night-charge");
    pub const BATTERY_TEMPORARY_MODE: Self = Self("battery.temporary-mode");
    pub const BATTERY_WATCH: Self = Self("battery.watch");

    pub const USB_ALWAYS_ON: Self = Self("usb.always-on");
    pub const USB_CHARGE_ON_BATTERY: Self = Self("usb.charge-on-battery");

    pub const POWER_SCHEME_ACTIVE: Self = Self("power.scheme");
    pub const POWER_SCHEME_LIST: Self = Self("power.scheme.list");
    pub const POWER_SCHEME_GET: Self = Self("power.scheme.get");
    pub const POWER_SCHEME_SETTING_READ: Self = Self("power.scheme.setting.read");
    pub const POWER_SCHEME_APPLY: Self = Self("power.scheme.apply");
    pub const POWER_SCHEME_SET: Self = Self("power.scheme.set");
    pub const POWER_SAVER_ONCE: Self = Self("power.saver-once");

    pub const PERF_MODE: Self = Self("perf.mode");
    pub const PERF_FAN_STATUS: Self = Self("perf.fan.status");
    pub const PERF_FAN_AUTO: Self = Self("perf.fan.auto");
    pub const PERF_FAN_MANUAL: Self = Self("perf.fan.manual");
    pub const PERF_FAN_FULLSPEED: Self = Self("perf.fan.fullspeed");
    pub const PERF_FAN_SMART: Self = Self("perf.fan.smart");
    pub const PERF_FAN_CURVE: Self = Self("perf.fan.curve");
    pub const PERF_TEMP_LIST: Self = Self("perf.temp");
    pub const PERF_TEMP_READ: Self = Self("perf.temp.read");
    pub const PERF_TEMP_WATCH: Self = Self("perf.temp.watch");
    pub const PERF_PL1: Self = Self("perf.pl1");
    pub const PERF_PL2: Self = Self("perf.pl2");
    pub const PERF_DBDC_GET: Self = Self("perf.dbdc.get");
    pub const PERF_TOP: Self = Self("perf.top");
    pub const PERF_BOOST: Self = Self("perf.boost");
    pub const PERF_THROTTLE: Self = Self("perf.throttle");

    pub const TUNE_PROFILE_LIST: Self = Self("tune.profile.list");
    pub const TUNE_PROFILE_SHOW: Self = Self("tune.profile.show");
    pub const TUNE_PROFILE_APPLY: Self = Self("tune.profile.apply");
    pub const TUNE_PL1: Self = Self("tune.pl1");
    pub const TUNE_EPP: Self = Self("tune.epp");
    pub const TUNE_TURBO: Self = Self("tune.turbo");
    pub const TUNE_RESTORE: Self = Self("tune.restore");
    pub const TUNE_TELEMETRY: Self = Self("tune.telemetry");
    pub const TUNE_WATCH: Self = Self("tune.watch");

    pub const KBD_BACKLIGHT: Self = Self("kbd.backlight");
    pub const KBD_FNLOCK: Self = Self("kbd.fnlock");
    pub const KBD_FN_CTRL_SWAP: Self = Self("kbd.fn-ctrl-swap");
    pub const KBD_WINLOCK: Self = Self("kbd.winlock");
    pub const TOUCHPAD: Self = Self("touchpad");
    pub const TOUCHPAD_STATUS: Self = Self("touchpad.status");
    pub const TOUCHPAD_RECOVER: Self = Self("touchpad.recover");

    pub const PANEL_RATE: Self = Self("panel.rate");
    pub const PANEL_BRIGHTNESS_STATUS: Self = Self("panel.brightness.status");
    pub const PANEL_BRIGHTNESS_SET: Self = Self("panel.brightness.set");
    pub const PANEL_COLOR: Self = Self("panel.color");
    pub const PANEL_SUPER_RESOLUTION: Self = Self("panel.super-resolution");
    pub const PANEL_OVERDRIVE: Self = Self("panel.overdrive");
    pub const PANEL_EYE_CARE: Self = Self("panel.eye-care");

    pub const PRIVACY_CAM: Self = Self("privacy.cam");
    pub const PRIVACY_MIC: Self = Self("privacy.mic");
    pub const PRIVACY_FINGERPRINT: Self = Self("privacy.fingerprint");
    pub const PRIVACY_STATUS: Self = Self("privacy.status");

    pub const SENSE_STATUS: Self = Self("sense.status");
    pub const SENSE_LOCK_ON_LEAVE: Self = Self("sense.lock-on-leave");
    pub const SENSE_WAKE_ON_APPROACH: Self = Self("sense.wake-on-approach");
    pub const SENSE_PAUSE_VIDEO: Self = Self("sense.pause-video");
    pub const SENSE_CAMERA_ASSIST: Self = Self("sense.camera-assist");
    pub const SENSE_APPROACH_AUTO_ADJUST: Self = Self("sense.approach-auto-adjust");
    pub const SENSE_LEAVE_AUTO_ADJUST: Self = Self("sense.leave-auto-adjust");
    pub const SENSE_RESET: Self = Self("sense.reset");
    pub const SENSE_ATTENTION_TRACKING: Self = Self("sense.attention-tracking");
    pub const SENSE_KBD_LIGHT_AUTO: Self = Self("sense.kbd-light-auto");

    pub const AUDIO_DOLBY: Self = Self("audio.dolby");
    pub const AUDIO_NOISE_CANCEL: Self = Self("audio.noise-cancel");

    pub const BIOS_LIST: Self = Self("bios.list");
    pub const BIOS_GET: Self = Self("bios.get");
    pub const BIOS_SET: Self = Self("bios.set");
    pub const BIOS_SAVE: Self = Self("bios.save");
    pub const BIOS_DISCARD: Self = Self("bios.discard");
    pub const BIOS_DEFAULTS: Self = Self("bios.defaults");
    pub const BIOS_PASSWORD_STATUS: Self = Self("bios.password.status");
    pub const BIOS_PASSWORD_SET: Self = Self("bios.password.set");
    pub const BIOS_PASSWORD_CLEAR: Self = Self("bios.password.clear");
    pub const BIOS_PASSWORD_VERIFY: Self = Self("bios.password.verify");

    pub const MAGICBAY_DETECT: Self = Self("magicbay.detect");
    pub const MAGICBAY_LTE_STATUS: Self = Self("magicbay.lte.status");
    pub const MAGICBAY_LTE_CONNECT: Self = Self("magicbay.lte.connect");
    pub const MAGICBAY_LTE_DISCONNECT: Self = Self("magicbay.lte.disconnect");
    pub const MAGICBAY_LTE_APN: Self = Self("magicbay.lte.apn");
    pub const MAGICBAY_CAM: Self = Self("magicbay.cam");
    pub const MAGICBAY_DISPLAY: Self = Self("magicbay.display");
    pub const MAGICBAY_WATCH: Self = Self("magicbay.watch");

    pub const OSD_ENABLE: Self = Self("osd.enable");
    pub const OSD_DISABLE: Self = Self("osd.disable");
    pub const OSD_TEST: Self = Self("osd.test");
    pub const OSD_SHOW: Self = Self("osd.show");

    pub const HOTKEY_LIST: Self = Self("hotkey.list");
    pub const HOTKEY_REGISTER: Self = Self("hotkey.register");
    pub const HOTKEY_TRIGGER: Self = Self("hotkey.trigger");
    pub const HOTKEY_UNREGISTER: Self = Self("hotkey.unregister");

    pub const UPDATE_CHECK: Self = Self("update.check");
    pub const UPDATE_LOCAL_INSTALL: Self = Self("update.local.install");
    pub const UPDATE_LOCAL_ROLLBACK: Self = Self("update.local.rollback");
    pub const UPDATE_LOCAL_HISTORY: Self = Self("update.local.history");
    pub const UPDATE_LOCAL_IGNORE: Self = Self("update.local.ignore");
    pub const UPDATE_LOCAL_SCHEDULE: Self = Self("update.local.schedule");
    pub const UPDATE_DOWNLOAD: Self = Self("update.download");
    pub const UPDATE_INSTALL: Self = Self("update.install");
    pub const UPDATE_HISTORY: Self = Self("update.history");
    pub const UPDATE_IGNORE: Self = Self("update.ignore");
    pub const UPDATE_ROLLBACK: Self = Self("update.rollback");
    pub const UPDATE_SCHEDULE: Self = Self("update.schedule");

    pub const SCAN_LIST: Self = Self("scan.list");
    pub const SCAN_RUN: Self = Self("scan.run");

    pub const WIFI_SECURITY: Self = Self("wifi.security");

    pub const FIX_RULES: Self = Self("fix.rules");
    pub const FIX_SCAN: Self = Self("fix.scan");
    pub const FIX_RUN: Self = Self("fix.run");
    pub const FIX_TRACK: Self = Self("fix.track");

    pub const SCENE_LIST: Self = Self("scene.list");
    pub const SCENE_TEST: Self = Self("scene.test");
    pub const SCENE_TRACK: Self = Self("scene.track");

    pub const SNAPSHOT_CAPTURE: Self = Self("snapshot.capture");
    pub const SNAPSHOT_DIFF: Self = Self("snapshot.diff");
    pub const SNAPSHOT_RESTORE: Self = Self("snapshot.restore");

    pub const DAEMON_START: Self = Self("daemon.start");
    pub const DAEMON_STOP: Self = Self("daemon.stop");
    pub const DAEMON_STATUS: Self = Self("daemon.status");
    pub const DAEMON_INSTALL: Self = Self("daemon.install");

    pub const COMPLETIONS: Self = Self("completions");

    /// Every command/subcommand currently accepted by the CLI parser.
    pub const ALL: &'static [Self] = &[
        Self::INFO,
        Self::DOCTOR,
        Self::BATTERY_STATUS,
        Self::BATTERY_ADAPTER,
        Self::BATTERY_CHARGE_MODE,
        Self::BATTERY_THRESHOLDS,
        Self::BATTERY_EXTREME_LIFE,
        Self::BATTERY_NIGHT_CHARGE,
        Self::BATTERY_TEMPORARY_MODE,
        Self::BATTERY_WATCH,
        Self::USB_ALWAYS_ON,
        Self::USB_CHARGE_ON_BATTERY,
        Self::POWER_SCHEME_ACTIVE,
        Self::POWER_SCHEME_LIST,
        Self::POWER_SCHEME_GET,
        Self::POWER_SCHEME_SETTING_READ,
        Self::POWER_SCHEME_APPLY,
        Self::POWER_SCHEME_SET,
        Self::POWER_SAVER_ONCE,
        Self::PERF_MODE,
        Self::PERF_FAN_STATUS,
        Self::PERF_FAN_AUTO,
        Self::PERF_FAN_MANUAL,
        Self::PERF_FAN_FULLSPEED,
        Self::PERF_FAN_SMART,
        Self::PERF_FAN_CURVE,
        Self::PERF_TEMP_LIST,
        Self::PERF_TEMP_READ,
        Self::PERF_TEMP_WATCH,
        Self::PERF_PL1,
        Self::PERF_PL2,
        Self::PERF_DBDC_GET,
        Self::PERF_TOP,
        Self::PERF_BOOST,
        Self::PERF_THROTTLE,
        Self::TUNE_PROFILE_LIST,
        Self::TUNE_PROFILE_SHOW,
        Self::TUNE_PROFILE_APPLY,
        Self::TUNE_PL1,
        Self::TUNE_EPP,
        Self::TUNE_TURBO,
        Self::TUNE_RESTORE,
        Self::TUNE_TELEMETRY,
        Self::TUNE_WATCH,
        Self::KBD_BACKLIGHT,
        Self::KBD_FNLOCK,
        Self::KBD_FN_CTRL_SWAP,
        Self::KBD_WINLOCK,
        Self::TOUCHPAD,
        Self::TOUCHPAD_STATUS,
        Self::TOUCHPAD_RECOVER,
        Self::PANEL_RATE,
        Self::PANEL_BRIGHTNESS_STATUS,
        Self::PANEL_BRIGHTNESS_SET,
        Self::PANEL_COLOR,
        Self::PANEL_SUPER_RESOLUTION,
        Self::PANEL_OVERDRIVE,
        Self::PANEL_EYE_CARE,
        Self::PRIVACY_CAM,
        Self::PRIVACY_MIC,
        Self::PRIVACY_FINGERPRINT,
        Self::PRIVACY_STATUS,
        Self::SENSE_STATUS,
        Self::SENSE_LOCK_ON_LEAVE,
        Self::SENSE_WAKE_ON_APPROACH,
        Self::SENSE_PAUSE_VIDEO,
        Self::SENSE_CAMERA_ASSIST,
        Self::SENSE_APPROACH_AUTO_ADJUST,
        Self::SENSE_LEAVE_AUTO_ADJUST,
        Self::SENSE_RESET,
        Self::SENSE_ATTENTION_TRACKING,
        Self::SENSE_KBD_LIGHT_AUTO,
        Self::AUDIO_DOLBY,
        Self::AUDIO_NOISE_CANCEL,
        Self::BIOS_LIST,
        Self::BIOS_GET,
        Self::BIOS_SET,
        Self::BIOS_SAVE,
        Self::BIOS_DISCARD,
        Self::BIOS_DEFAULTS,
        Self::BIOS_PASSWORD_STATUS,
        Self::BIOS_PASSWORD_SET,
        Self::BIOS_PASSWORD_CLEAR,
        Self::BIOS_PASSWORD_VERIFY,
        Self::MAGICBAY_DETECT,
        Self::MAGICBAY_LTE_STATUS,
        Self::MAGICBAY_LTE_CONNECT,
        Self::MAGICBAY_LTE_DISCONNECT,
        Self::MAGICBAY_LTE_APN,
        Self::MAGICBAY_CAM,
        Self::MAGICBAY_DISPLAY,
        Self::MAGICBAY_WATCH,
        Self::OSD_ENABLE,
        Self::OSD_DISABLE,
        Self::OSD_TEST,
        Self::OSD_SHOW,
        Self::HOTKEY_LIST,
        Self::HOTKEY_REGISTER,
        Self::HOTKEY_TRIGGER,
        Self::HOTKEY_UNREGISTER,
        Self::UPDATE_CHECK,
        Self::UPDATE_LOCAL_INSTALL,
        Self::UPDATE_LOCAL_ROLLBACK,
        Self::UPDATE_LOCAL_HISTORY,
        Self::UPDATE_LOCAL_IGNORE,
        Self::UPDATE_LOCAL_SCHEDULE,
        Self::UPDATE_DOWNLOAD,
        Self::UPDATE_INSTALL,
        Self::UPDATE_HISTORY,
        Self::UPDATE_IGNORE,
        Self::UPDATE_ROLLBACK,
        Self::UPDATE_SCHEDULE,
        Self::SCAN_LIST,
        Self::SCAN_RUN,
        Self::FIX_RULES,
        Self::FIX_SCAN,
        Self::FIX_RUN,
        Self::FIX_TRACK,
        Self::SCENE_LIST,
        Self::SCENE_TEST,
        Self::SCENE_TRACK,
        Self::SNAPSHOT_CAPTURE,
        Self::SNAPSHOT_DIFF,
        Self::SNAPSHOT_RESTORE,
        Self::DAEMON_START,
        Self::DAEMON_STOP,
        Self::DAEMON_STATUS,
        Self::DAEMON_INSTALL,
        Self::COMPLETIONS,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Serialize for CommandId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateLayer {
    /// No hardware or persistent state is changed by this operation.
    NotApplicable,
    /// Current hardware/kernel state, not a BIOS NVRAM setting.
    Runtime,
    /// A BIOS/firmware setting; saved-state readback is not proof of post-reboot effect.
    BiosPersistent,
    /// State owned and persisted by the operating system.
    OsPersistent,
    /// State persisted by this program, such as a snapshot or recovery file.
    LocalPersistent,
    /// A profile/snapshot spans more than one state layer.
    Composite,
    /// The layer differs between the Windows and Linux HAL implementations.
    PlatformDependent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readback {
    NotApplicable,
    /// A successful commit must read the relevant state back and compare it.
    Required,
    /// No verified readback contract exists; service success must not imply effect.
    Unverified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rollback {
    NotApplicable,
    /// The prior value must be captured so it can be restored.
    PreviousState,
    /// A multi-write operation must restore already-applied values on failure.
    Transactional,
    /// No reliable inverse operation is available.
    Unavailable,
}

/// Independent operation axes. `read && !write` is read-only; `write` never
/// implies that readback or rollback is available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CommandAxes {
    pub read: bool,
    pub write: bool,
    pub readback: Readback,
    pub rollback: Rollback,
    pub layer: StateLayer,
}

impl CommandAxes {
    #[must_use]
    pub const fn read_only(layer: StateLayer) -> Self {
        Self {
            read: true,
            write: false,
            readback: Readback::NotApplicable,
            rollback: Rollback::NotApplicable,
            layer,
        }
    }

    #[must_use]
    pub const fn mutation(layer: StateLayer, readback: Readback, rollback: Rollback) -> Self {
        Self {
            read: true,
            write: true,
            readback,
            rollback,
            layer,
        }
    }

    #[must_use]
    pub const fn write_only(layer: StateLayer, readback: Readback, rollback: Rollback) -> Self {
        Self {
            read: false,
            write: true,
            readback,
            rollback,
            layer,
        }
    }

    #[must_use]
    pub const fn action(layer: StateLayer) -> Self {
        Self {
            read: false,
            write: false,
            readback: Readback::NotApplicable,
            rollback: Rollback::NotApplicable,
            layer,
        }
    }

    #[must_use]
    pub const fn is_read_only(self) -> bool {
        self.read && !self.write
    }

    #[must_use]
    pub const fn selector(self, command: CommandId) -> CommandSelector {
        CommandSelector {
            command,
            layer: self.layer,
        }
    }
}

/// A command together with its argument-selected state layer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CommandSelector {
    pub command: CommandId,
    pub layer: StateLayer,
}

impl CommandSelector {
    #[must_use]
    pub const fn new(command: CommandId, layer: StateLayer) -> Self {
        Self { command, layer }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareRequirement {
    None,
    /// At least one attached MagicBay record is required for device operations.
    MagicBay,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwarePresence {
    #[default]
    Unknown,
    Present,
    Absent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyUse {
    Required,
    Optional,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dependency {
    PublicWin32Cim,
    Win32PowerApi,
    Win32Scm,
    Win32Registry,
    Win32SetupApi,
    Win32WinTrust,
    Win32DisplayApi,
    Win32DeviceManagement,
    Win32MobileBroadband,
    Win32MediaFoundation,
    Win32ProcessApi,
    Win32TaskScheduler,
    WindowsWmiEvents,
    Win32DeviceNotifications,
    WindowsHpd,
    WindowsAudio,
    LenovoFirmwareWmi,
    LenovoVendorWmi,
    EnergyDriver,
    LenovoProcessManagement,
    SmartSense,
    DolbyAudioSdk,
    NoiseCancellationSdk,
    IntelDptf,
    LenovoUtilityService,
    LenovoFnAndFunctionKeys,
    LenovoVantageService,
    LenovoPcManagerService,
    LnvDispatcherService,
    LenovoDiagnosticsDriver,
    SmartEngine,
    LinuxSysfs,
    LinuxProcfs,
    LinuxPowercap,
    LinuxModemManager,
    LinuxV4l2,
    LinuxDrm,
    LinuxUdev,
    LinuxDiagnosticTools,
    LinuxSystemd,
    LocalFilesystem,
    LocalDaemon,
    AuthenticatedUpdateCatalog,
}

impl Dependency {
    pub const VENDOR_SWITCHES: &'static [Self] = &[
        Self::EnergyDriver,
        Self::LenovoProcessManagement,
        Self::SmartSense,
        Self::DolbyAudioSdk,
        Self::NoiseCancellationSdk,
        Self::IntelDptf,
        Self::LenovoUtilityService,
        Self::LenovoFnAndFunctionKeys,
        Self::LenovoVantageService,
        Self::LenovoPcManagerService,
        Self::LnvDispatcherService,
        Self::LenovoDiagnosticsDriver,
        Self::SmartEngine,
    ];

    #[must_use]
    pub const fn platform(self) -> Option<Platform> {
        match self {
            Self::PublicWin32Cim
            | Self::Win32PowerApi
            | Self::Win32Scm
            | Self::Win32Registry
            | Self::Win32SetupApi
            | Self::Win32WinTrust
            | Self::Win32DisplayApi
            | Self::Win32DeviceManagement
            | Self::Win32MobileBroadband
            | Self::Win32MediaFoundation
            | Self::Win32ProcessApi
            | Self::Win32TaskScheduler
            | Self::WindowsWmiEvents
            | Self::Win32DeviceNotifications
            | Self::WindowsHpd
            | Self::WindowsAudio
            | Self::LenovoFirmwareWmi
            | Self::LenovoVendorWmi
            | Self::EnergyDriver
            | Self::LenovoProcessManagement
            | Self::SmartSense
            | Self::DolbyAudioSdk
            | Self::NoiseCancellationSdk
            | Self::IntelDptf
            | Self::LenovoUtilityService
            | Self::LenovoFnAndFunctionKeys
            | Self::LenovoVantageService
            | Self::LenovoPcManagerService
            | Self::LnvDispatcherService
            | Self::LenovoDiagnosticsDriver
            | Self::SmartEngine => Some(Platform::Windows),
            Self::LinuxSysfs
            | Self::LinuxProcfs
            | Self::LinuxPowercap
            | Self::LinuxModemManager
            | Self::LinuxV4l2
            | Self::LinuxDrm
            | Self::LinuxUdev
            | Self::LinuxDiagnosticTools
            | Self::LinuxSystemd => Some(Platform::Linux),
            Self::LocalFilesystem | Self::LocalDaemon | Self::AuthenticatedUpdateCatalog => None,
        }
    }

    /// Existing `CapabilitySet` channel key, where one exists. No service or
    /// endpoint support is inferred when this returns `None`.
    #[must_use]
    pub const fn capability_id(self) -> Option<&'static str> {
        match self {
            Self::EnergyDriver => Some("channel.energy_driver"),
            Self::LenovoFirmwareWmi => Some("channel.wmi.root"),
            _ => None,
        }
    }

    #[must_use]
    pub const fn component(self) -> &'static str {
        match self {
            Self::PublicWin32Cim => "public Win32/CIM/WMI",
            Self::Win32PowerApi => "Windows Power API",
            Self::Win32Scm => "Windows Service Control Manager",
            Self::Win32Registry => "Windows registry",
            Self::Win32SetupApi => "Windows SetupAPI/PNPnP",
            Self::Win32WinTrust => "Windows WinTrust signature verification",
            Self::Win32DisplayApi => "Windows display configuration/GDI",
            Self::Win32DeviceManagement => "Windows device installation and property APIs",
            Self::Win32MobileBroadband => "Windows Mobile Broadband (MBN/MBIM)",
            Self::Win32MediaFoundation => "Windows Media Foundation/UVC",
            Self::Win32ProcessApi => "Windows process and job APIs",
            Self::Win32TaskScheduler => "Windows Task Scheduler",
            Self::WindowsWmiEvents => "Windows WMI event subscription",
            Self::Win32DeviceNotifications => "Windows device-change notifications",
            Self::WindowsHpd => "Windows Human Presence Detection COM",
            Self::WindowsAudio => "Windows audio/kernel-streaming APIs",
            Self::LenovoFirmwareWmi => "Lenovo ACPI firmware through root\\WMI",
            Self::LenovoVendorWmi => "Lenovo vendor WMI data classes (root\\WMI)",
            Self::EnergyDriver => "AcpiVpc.sys / \\\\.\\EnergyDrv",
            Self::LenovoProcessManagement => "LenovoProcessManagement",
            Self::SmartSense => "SmartSense and ACPI\\IDEA2002",
            Self::DolbyAudioSdk => "Dolby audio endpoint/RPC SDK",
            Self::NoiseCancellationSdk => "microphone DSP or Elevoc/Aispeech SDK",
            Self::IntelDptf => "Intel DPTF/IPF (dptftcs, ipfsvc.exe)",
            Self::LenovoUtilityService => "LenovoUtilityService",
            Self::LenovoFnAndFunctionKeys => "LenovoFnAndFunctionKeys",
            Self::LenovoVantageService => "LenovoVantageService",
            Self::LenovoPcManagerService => "LenovoPcManagerService",
            Self::LnvDispatcherService => "LNVDispatcherService",
            Self::LenovoDiagnosticsDriver => "LenovoDiagnosticsDriver.sys",
            Self::SmartEngine => "SmartEngine",
            Self::LinuxSysfs => "Linux sysfs/platform drivers",
            Self::LinuxProcfs => "Linux procfs/process APIs",
            Self::LinuxPowercap => "Linux intel_rapl powercap",
            Self::LinuxModemManager => "Linux ModemManager/MBIM",
            Self::LinuxV4l2 => "Linux V4L2/UVC",
            Self::LinuxDrm => "Linux DRM/KMS",
            Self::LinuxUdev => "Linux udev/netlink",
            Self::LinuxDiagnosticTools => "Linux public diagnostic tools",
            Self::LinuxSystemd => "Linux systemd user services",
            Self::LocalFilesystem => "local filesystem",
            Self::LocalDaemon => "vantage daemon",
            Self::AuthenticatedUpdateCatalog => "authenticated update catalog and HTTPS manifest",
        }
    }

    #[must_use]
    pub const fn switch_guidance(self) -> Option<&'static str> {
        match self {
            Self::EnergyDriver => Some(
                "Keep when a selected command requires EnergyDrv; public WMI/OS battery reads can remain independent.",
            ),
            Self::LenovoProcessManagement => Some(
                "Keep for target performance-mode writes; registry readback alone is not a hardware writer.",
            ),
            Self::SmartSense => Some(
                "Keep for presence-sensing commands and IR-camera dependencies; no generic fallback is implied.",
            ),
            Self::DolbyAudioSdk | Self::NoiseCancellationSdk => Some(
                "Keep when the corresponding licensed/vendor audio path is selected; clean-room code does not replace its DSP.",
            ),
            Self::IntelDptf => Some(
                "Do not disable to force RAPL: firmware/DPTF owns thermal policy and Windows raw MSR control is unsupported.",
            ),
            Self::LenovoUtilityService | Self::LenovoFnAndFunctionKeys => Some(
                "Only hotkey/OSD convenience paths use these; direct WMI control does not require them.",
            ),
            Self::LenovoVantageService | Self::LenovoPcManagerService => Some(
                "No direct dependency is inferred for clean-room commands; review official-application use separately.",
            ),
            Self::LnvDispatcherService => Some(
                "This is an optional convenience/alternate path, not a substitute for the exact required command channel.",
            ),
            Self::LenovoDiagnosticsDriver => Some(
                "The proprietary deep-diagnostics driver is excluded; no registry entry should require it.",
            ),
            Self::SmartEngine => Some(
                "Keep only for a separately verified SmartEngine feature; no panel writer is inferred from its presence.",
            ),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CommandDependency {
    pub dependency: Dependency,
    pub usage: DependencyUse,
}

#[must_use]
pub const fn required(dependency: Dependency) -> CommandDependency {
    CommandDependency {
        dependency,
        usage: DependencyUse::Required,
    }
}

#[must_use]
pub const fn optional(dependency: Dependency) -> CommandDependency {
    CommandDependency {
        dependency,
        usage: DependencyUse::Optional,
    }
}
