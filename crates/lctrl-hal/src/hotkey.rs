//! Global hotkey control.
//!
//! Lenovo ships game-zone and function-key hotkeys that the official Vantage
//! handles (start game zone, switch performance mode, fan mode). Taking them
//! over means registering the same global hotkeys natively and dispatching the
//! press to a scene/control action instead of letting Vantage consume it.
//!
//! A registered global hotkey requires an interactive desktop session with a
//! message pump (RegisterHotKey + a window that receives WM_HOTKEY). A daemon
//! running outside that session must report the gate honestly instead of
//! claiming the hotkey is live.
//!
//! The vendor side of the same question is separate state: the
//! `LenovoFnAndFunctionKeys` package owns the Fn row, the Fn/Ctrl swap and the
//! persisted per-keyboard tables. [`HotkeyControl::status`] reports that state
//! read-only; `docs/evidence/re-hotkeys-20261105.md` pins what the machine can
//! answer and what it cannot.

use lctrl_core::Result;

/// The observed outcome of a hotkey registration attempt.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct HotkeyOutcome {
    /// `registered`, `gated` (interactive session / message pump unavailable),
    /// or `dry_run`.
    pub state: String,
    /// The hotkey id that was (or would be) registered.
    pub id: u32,
    /// A human-readable gate reason when state is `gated`.
    pub detail: String,
}

/// A set of globally registered hotkeys.
pub trait HotkeyControl {
    /// Register a global hotkey (id + virtual key). Returns the observed state.
    fn register(&self, id: u32, vk: u32, dry_run: bool) -> Result<HotkeyOutcome>;

    /// Release a previously registered hotkey.
    fn unregister(&self, id: u32) -> Result<bool>;

    /// The ids currently registered and live.
    fn registered(&self) -> Vec<u32>;

    /// Read the Lenovo Fn hotkey package state: the owner service, the
    /// capability flags it keys its behaviour on, the firmware switches
    /// (FnLock, Fn/Ctrl swap) and the persisted per-keyboard tables.
    ///
    /// Read-only by construction: it never sends a service control code, never
    /// stages a BIOS write and never remaps a key. A machine without the Fn
    /// package answers with `service_running = false` and `unknown` switches
    /// instead of an error.
    fn status(&self) -> Result<HotkeyStatus> {
        Err(lctrl_core::LctrlError::Unsupported {
            feature: "hotkey.status".into(),
        })
    }
}

/// The state of a firmware-owned keyboard switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeySwitchState {
    /// The switch is enabled (Fn-lock on, Fn/Ctrl swapped).
    On,
    /// The switch is disabled.
    Off,
    /// The firmware items are absent or contradict each other; the raw values
    /// are still reported.
    Unknown,
}

impl HotkeySwitchState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for HotkeySwitchState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One firmware setting a switch was derived from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HotkeySetting {
    /// The `Lenovo_BiosSetting` item name, e.g. `HotkeyMode`.
    pub name: String,
    /// The value the firmware reported.
    pub value: String,
}

/// A keyboard switch together with the settings it was derived from, so the
/// reported state is checkable instead of asserted.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HotkeySwitch {
    pub state: HotkeySwitchState,
    /// The firmware items behind the state; empty when none could be read.
    pub settings: Vec<HotkeySetting>,
    /// Why the state is what it is.
    pub detail: String,
}

/// The `Capbility` flags the Fn package keys its hotkey behaviour on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct HotkeyCapabilities {
    /// `SupportFnLockVer` — the package advertises the FnLock feature.
    pub support_fn_lock: bool,
    /// `SupportKeyboardLocationLayout` — keyboard-layout switching is offered.
    pub support_keyboard_layout: bool,
    /// `SupportNumlkKey` — the SKU has a NumLk key (also gates the NumLk OSD).
    pub support_numlock_key: bool,
    /// `IsUserChangedCapsNumLKStatus` — the user has taken the Caps/Num OSD
    /// decision over.
    pub user_changed_caps_numlock: bool,
}

/// A persisted per-keyboard hotkey table.
///
/// The tables are the vendor utility's compatibility list of Lenovo keyboard
/// models; they are not tied to the currently connected keyboard and their
/// payload semantics are not decoded here.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HotkeyTable {
    /// `current_user` (`HKCU\SOFTWARE\Lenovo\Hotkey`) or `machine`
    /// (`HKLM\SOFTWARE\Lenovo\LenovoFnHotkey\HIDKBMicMuteLED`).
    pub scope: &'static str,
    /// The keyboard model key, e.g. `VID_0461&PID_4F2B`.
    pub device: String,
    /// How many mapping values the table holds.
    pub mappings: usize,
    /// The mapping value names as persisted, e.g. `Ex_1A`.
    pub keys: Vec<String>,
}

/// Read-only state of the Lenovo Fn hotkey package.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HotkeyStatus {
    /// The owner service name (`LenovoFnAndFunctionKeys`).
    pub service: &'static str,
    /// Whether that service exists and reports `SERVICE_RUNNING`.
    pub service_running: bool,
    /// The package version the service key records (`UtilityVersion`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility_version: Option<String>,
    /// Whether the interactive renderer `FnHotkeyUtility.exe` is running.
    pub utility_running: bool,
    /// Whether the CapsLk/NumLk helper `FnHotkeyCapsLKNumLK.exe` is running.
    pub caps_led_running: bool,
    pub capabilities: HotkeyCapabilities,
    /// F1-F12 row mode (`HotkeyMode` + `F1-F12AsPrimaryFunction`).
    pub fn_lock: HotkeySwitch,
    /// Fn/Ctrl swap (`FoolProofFnCtrl`, with `FnAndCtrlKeySwap` as the
    /// observed sibling).
    pub fn_ctrl_swap: HotkeySwitch,
    pub tables: Vec<HotkeyTable>,
    /// Sources that could not be read, verbatim.
    pub notes: Vec<String>,
}
