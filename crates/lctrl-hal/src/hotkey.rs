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
}
