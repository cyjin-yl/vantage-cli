use lctrl_core::{
    ApplyMode, BacklightState, ChangeReport, EyeCareLevel, PanelMode, PanelRefreshCapability,
    PanelIdentity, PanelRefreshRequest, RefreshMode, Result,
};

pub trait KeyboardControl: Send + Sync {
    fn backlight_state(&self) -> Result<BacklightState>;
    fn set_backlight(
        &self,
        level: u8,
        effect: lctrl_core::LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>>;
}

pub trait TouchpadControl: Send + Sync {
    fn touchpad_state(&self) -> Result<lctrl_core::DeviceState>;
    fn set_touchpad(
        &self,
        state: lctrl_core::DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<lctrl_core::DeviceState>>;

    /// Repair the input leaves of one touchpad: enable the leaves an
    /// interrupted transition left disabled, or disable and re-enable them when
    /// every leaf is already enabled but the pad does not serve input. A
    /// platform without a verified input-leaf channel reports `Unsupported`.
    fn recover_touchpad(&self, _apply: ApplyMode) -> Result<lctrl_core::TouchpadRecoveryReport> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "touchpad.recover".into(),
        })
    }

    /// Read the HID capability descriptor of the admitted touchpad: top-level
    /// usage, declared contacts, surface ranges, button type and the usages the
    /// collection declares. Read-only: a platform without a verified
    /// capability channel reports `Unsupported` instead of synthesising values.
    fn touchpad_caps(&self) -> Result<lctrl_core::TouchpadCapabilities> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "touchpad.caps".into(),
        })
    }
}

pub trait PrivacyControl: Send + Sync {
    fn camera_state(&self) -> Result<lctrl_core::DeviceState>;
    fn set_camera(
        &self,
        state: lctrl_core::DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<lctrl_core::DeviceState>>;

    /// Per-devnode runtime view of the admitted camera group.
    ///
    /// [`Self::camera_state`] refuses a group that is split between enabled
    /// and disabled devnodes; this read attributes the split so an operator can
    /// see which collection is off. Platforms without a verified per-devnode
    /// read keep the channel closed instead of inventing one.
    fn camera_devices(&self) -> Result<Vec<lctrl_core::PrivacyDeviceState>> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "privacy.cam.devices".into(),
        })
    }

    /// What the platform's capability-consent store has *recorded* about camera
    /// consent and last use, per application.
    ///
    /// This is a record read, not an observation of the device: the report
    /// carries its own caveat and the in-use rule it applied. Platforms without
    /// such a store keep the channel closed rather than fabricating an empty
    /// success.
    fn camera_consent(&self) -> Result<lctrl_core::CameraConsentReport> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "privacy.cam.inuse".into(),
        })
    }

    /// Runtime microphone mute through the same PnP channel as the camera.
    /// Platforms without a verified internal-microphone power switch must
    /// report `Unsupported` rather than mutating an unrelated audio device.
    fn microphone_state(&self) -> Result<lctrl_core::DeviceState>;
    fn set_microphone(
        &self,
        state: lctrl_core::DeviceState,
        apply: ApplyMode,
    ) -> Result<ChangeReport<lctrl_core::DeviceState>>;
}

pub trait PanelControl: Send + Sync {
    fn refresh_capability(&self) -> Result<PanelRefreshCapability>;
    fn refresh_mode(&self) -> Result<RefreshMode>;
    fn panel_mode(&self) -> Result<PanelMode>;
    fn set_refresh_rate(
        &self,
        request: PanelRefreshRequest,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PanelMode>>;
}

/// Identity of the embedded panel, decoded from the panel's own EDID.
///
/// Implementations must resolve the *internal* display path themselves and
/// refuse to answer from an arbitrary display record: monitor EDID values are
/// stored per PnP instance, disconnected monitors keep their records, and one
/// instance can carry more than one class record. A platform that cannot name a
/// single internal panel reports `Unsupported`/`ChannelUnavailable` instead of
/// guessing.
pub trait PanelIdentityControl: Send + Sync {
    fn identity(&self) -> Result<PanelIdentity>;
}

/// Brightness of a uniquely identified active internal panel. Implementations
/// must not redirect a transition to an external or newly attached monitor.
pub trait PanelBrightnessControl: Send + Sync {
    fn brightness(&self) -> Result<lctrl_core::PanelBrightness>;
    /// If provided, `expected_current` must match the freshly observed value
    /// before any write is attempted; it is not an atomic cross-process CAS.
    fn set_brightness(
        &self,
        percent: u8,
        expected_current: Option<u8>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<u8>>;
}

/// Panel image-quality features driven through the graphics driver control
/// library (Intel IGCL on the reference target). Gamut switching is not part
/// of this trait: on the reference target the original vendor path requires a
/// Lenovo-delivered monitor-control DLL, which the replacement must not
/// depend on (docs/interfaces.md §15).
pub trait PanelFeaturesControl: Send + Sync {
    /// Current eye-care level as observed from the active 1D LUT.
    /// `Ok(None)` means the driver cannot report the level (no readback);
    /// `Err(Unsupported)` means the panel has no pixel-transformation
    /// pipeline at all.
    fn eye_care(&self) -> Result<Option<EyeCareLevel>>;
    fn set_eye_care(
        &self,
        level: EyeCareLevel,
        apply: ApplyMode,
    ) -> Result<ChangeReport<Option<EyeCareLevel>>>;
    /// `Ok(None)` means the driver cannot report the current state.
    fn super_resolution(&self) -> Result<Option<bool>>;
    fn set_super_resolution(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<Option<bool>>>;
}

/// Read-only display state: attachment, mode, scaling, and advanced colour.
///
/// The three reads have deliberately different scopes and must not be merged:
///
/// - [`Self::displays`] is the calling session's live desktop. Implementations
///   must surface the OS refusal (`PermissionDenied` for a service/SSH logon
///   whose `QueryDisplayConfig` is refused) instead of returning an empty list
///   or this session's synthetic desktop as a panel;
/// - [`Self::desktops`] names that synthetic session desktop, so the fallback is
///   attributable when the live enumeration is unavailable;
/// - [`Self::monitors`] is machine-wide state of connected monitors (driver
///   colour store plus the monitor's own EDID), joined by monitor hardware id
///   and readable without a desktop session.
///
/// No implementation may write: HDR, wide colour, scaling and display modes are
/// reported, never changed and never persisted.
pub trait DisplayStatusControl: Send + Sync {
    /// Session scope of a live display read.
    fn session(&self) -> lctrl_core::DisplaySession;
    /// Live display paths of the calling session.
    fn displays(&self) -> Result<Vec<lctrl_core::DisplayPathState>>;
    /// The calling session's own monitor list, for attribution when
    /// [`Self::displays`] is unavailable.
    fn desktops(&self) -> Result<Vec<lctrl_core::SessionDesktop>>;
    /// Machine-wide colour/HDR state of every connected monitor.
    fn monitors(&self) -> Result<Vec<lctrl_core::MonitorColorState>>;
}
