use lctrl_core::{MagicBayLteStatus, Result};

/// Read-only Mobile Broadband (MagicBay LTE) status.
///
/// The contract is deliberately read-only. Session setup (`connect`,
/// `disconnect`, APN provisioning) needs a modem-control API; on the current
/// target neither the classic MBN API (`mbnapi.dll` is absent from builds that
/// still ship the WWAN service) nor the WWAN Service API (COM-only, interface
/// contract lives in the WDK `wwanapi.h`) can be bound and exercised, so those
/// operations stay unsupported (`docs/evidence/re-magicbay-20261105.md` §3.2).
pub trait MobileBroadbandControl: Send + Sync {
    /// Accessory presence plus the WWAN interface and address state.
    ///
    /// The accessory is identified by the recovered MagiCenter detector
    /// contract (USB `VID_17EF` with a Mobile Broadband PID). A machine that
    /// carries an unrelated WWAN modem but no MagicBay accessory reports
    /// `ChannelUnavailable` rather than an empty success: this channel never
    /// claims another vendor's modem.
    fn lte_status(&self) -> Result<MagicBayLteStatus>;
}
