//! On-screen display (OSD) control.
//!
//! Two channels live behind this trait:
//!
//! * the replacement's own overlay — a transient, centered, always-on-top GDI
//!   window that announces a state change. It requires the active interactive
//!   desktop session; a daemon running outside that session (e.g. a
//!   non-interactive SSH service) must report the gate honestly instead of
//!   claiming it painted anything.
//! * the Lenovo Fn on-screen display (the indicator `FnHotkeyUtility.exe` paints
//!   on CapsLk/NumLk). Its persisted preference is owned by the
//!   `LenovoFnAndFunctionKeys` service: user-defined service control codes write
//!   `VantageToast\ShowCapslkOSD` / `ShowNumlkOSD`, and reading those values back
//!   is the verification the vendor UI itself performs. See
//!   `docs/evidence/re-osd-20261105.md` for the pinned contract.

use lctrl_core::Result;

/// A transient on-screen overlay plus the Lenovo Fn OSD preference.
pub trait OsdControl {
    /// Show a centered, always-on-top overlay with the given lines for a bounded
    /// duration. Returns the observed state after the attempt (painted / gated /
    /// hidden). During a dry-run no window is created.
    fn show(&self, lines: &[&str], dry_run: bool) -> Result<OsdOutcome>;

    /// Immediately hide any active overlay.
    fn hide(&self) -> Result<bool>;

    /// Whether an overlay is currently visible.
    fn visible(&self) -> bool;

    /// Read the Lenovo Fn OSD preference for each indicator. Read-only: it never
    /// talks to the service, so it is safe on any machine (indicators the
    /// installed hotkey package does not control report `NotSupported`).
    fn fn_osd(&self) -> Result<FnOsdStatus>;

    /// Enable or disable the Lenovo Fn OSD. Only indicators whose state is
    /// `On`/`Off` are driven — a `NotSupported` indicator is reported as such and
    /// no control code is sent for it. Each driven indicator is verified by
    /// reading its registry value back; on a mismatch the captured previous
    /// state is restored and `VerifyMismatch` is returned.
    ///
    /// During a dry-run the observed status is reported and no control code is
    /// sent.
    fn set_fn_osd(&self, enabled: bool, dry_run: bool) -> Result<FnOsdChange>;
}

/// The observed outcome of an OSD show attempt.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct OsdOutcome {
    /// `painted`, `gated` (interactive session unavailable), or `dry_run`.
    pub state: String,
    /// The lines that would be (or were) shown.
    pub lines: Vec<String>,
    /// A human-readable gate reason when state is `gated`.
    pub detail: String,
}

/// State of one Lenovo Fn OSD indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OsdIndicatorState {
    /// The indicator's OSD is shown.
    On,
    /// The indicator's OSD is suppressed.
    Off,
    /// The installed hotkey package does not expose this indicator (no
    /// capability flag, or no preference value to read).
    NotSupported,
}

impl OsdIndicatorState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::NotSupported => "not_supported",
        }
    }
}

impl std::fmt::Display for OsdIndicatorState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The observed preference for one Fn OSD indicator, together with the control
/// codes that drive it. Carrying the contract in the status keeps the reported
/// result self-describing: a caller can see which value was read and which codes
/// a commit would send.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FnOsdIndicator {
    /// `capslock` or `numlock`.
    pub indicator: &'static str,
    pub state: OsdIndicatorState,
    /// Registry value name under the service's `VantageToast` key.
    pub value_name: &'static str,
    /// The raw DWORD read back, when the value exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_value: Option<u32>,
    /// User-defined service control code that turns the indicator's OSD on.
    pub enable_control: u32,
    /// User-defined service control code that turns the indicator's OSD off.
    pub disable_control: u32,
    /// Why the state is what it is (gate reason, or the value it was read from).
    pub detail: String,
}

/// The Lenovo Fn OSD preference.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FnOsdStatus {
    /// The service that owns the preference.
    pub service: &'static str,
    /// Whether that service exists and reports `SERVICE_RUNNING`.
    pub service_running: bool,
    pub capslock: FnOsdIndicator,
    pub numlock: FnOsdIndicator,
}

/// The before/after observation of a `set_fn_osd` call.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FnOsdChange {
    /// The state that was requested for every driven indicator.
    pub requested: OsdIndicatorState,
    /// `dry_run` (nothing was sent) or `committed`.
    pub mode: &'static str,
    pub before: FnOsdStatus,
    pub after: FnOsdStatus,
}
