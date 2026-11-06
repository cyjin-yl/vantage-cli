//! Linux radio service.
//!
//! This backend binds no radio-control plane, so the channel is registered and answers with an
//! explicit [`LctrlError::Unsupported`] instead of leaving `radio` commands looking like a command
//! that this platform silently dropped. The documented Linux radio plane (`/sys/class/rfkill`) is
//! not bound here: no Linux implementation of the radio contract has been recovered and validated
//! in this wave, and inventing one from the sysfs node names alone would claim a soft/hard state
//! model the Windows evidence does not support.

use lctrl_core::{ApplyMode, LctrlError, Result};
use lctrl_hal::{RadioControl, RadioKind, RadioReport, RadioState, RadioTransition};

/// The Linux radio service: every call reports the channel as unsupported.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxRadio;

impl LinuxRadio {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl RadioControl for LinuxRadio {
    fn radio_status(&self) -> Result<RadioReport> {
        Err(LctrlError::Unsupported {
            feature: "radio.status on Linux (no radio-control plane is bound in this backend; \
                      /sys/class/rfkill is not implemented or validated)"
                .to_owned(),
        })
    }

    fn radio_set_state(
        &self,
        _kind: RadioKind,
        _state: RadioState,
        _allow_session_cut: bool,
        _apply: ApplyMode,
    ) -> Result<RadioTransition> {
        Err(LctrlError::Unsupported {
            feature: "radio.set on Linux (no radio-control plane is bound in this backend; \
                      /sys/class/rfkill is not implemented or validated)"
                .to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_radio_reports_the_channel_as_unsupported_rather_than_answering() {
        let radio = LinuxRadio::new();
        let status = radio.radio_status().expect_err("Linux radio status is unsupported");
        assert!(matches!(status, LctrlError::Unsupported { .. }));
        let write = radio
            .radio_set_state(RadioKind::Wifi, RadioState::Off, false, ApplyMode::Commit)
            .expect_err("Linux radio writes are unsupported");
        assert!(matches!(write, LctrlError::Unsupported { .. }));
    }
}
