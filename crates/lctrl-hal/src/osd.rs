//! On-screen display (OSD) control.
//!
//! The Lenovo game-zone OSD is a transient, centered, always-on-top overlay that
//! announces a state change (game zone started, performance mode changed, fan
//! mode changed). Taking it over means rendering that overlay natively instead
//! of letting the official Vantage own it.
//!
//! The overlay requires the active interactive desktop session: it is a real
//! top-level window painted with GDI. A daemon running outside that session
//! (e.g. a non-interactive SSH service) must report the gate honestly instead
//! of claiming it painted anything.

use lctrl_core::Result;

/// A transient on-screen overlay.
pub trait OsdControl {
    /// Show a centered, always-on-top overlay with the given lines for a bounded
    /// duration. Returns the observed state after the attempt (painted / gated /
    /// hidden). During a dry-run no window is created.
    fn show(&self, lines: &[&str], dry_run: bool) -> Result<OsdOutcome>;

    /// Immediately hide any active overlay.
    fn hide(&self) -> Result<bool>;

    /// Whether an overlay is currently visible.
    fn visible(&self) -> bool;
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
