use lctrl_core::{ApplyMode, ChangeReport, Result, SenseDistance, SenseState, SenseWait};

/// One SmartSense setting. HPD settings use the recovered RPC interface;
/// privacy alert and attention tracking retain their separate legacy pipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenseFeature {
    /// HPD RPC `SetPresenceLeaveEnabled`, op 11, 0/1.
    LockOnLeave(bool),
    /// HPD RPC `SetPresenceLeaveDistance`, op 14, Near/Middle/Far.
    LeaveDistance(SenseDistance),
    /// HPD RPC `SetPresenceLeaveWaitTime`, op 12, Short/Middle/Long.
    LeaveWait(SenseWait),
    /// HPD RPC `SetApproachEnabled`, op 8, 0/1.
    WakeOnApproach(bool),
    /// HPD RPC `SetApproachDistance`, op 9, Near/Middle/Far.
    ApproachDistance(SenseDistance),
    /// HPD RPC `SetVideoAutoPauseResume`, op 16, 0/1 (not pipe 1/2).
    PauseVideo(bool),
    /// HPD RPC `SetApproachDistanceAutoAdjust`, op 10, 0/1.
    ApproachAutoAdjust(bool),
    /// HPD RPC `SetPresenceLeaveCameraAssist`, op 13, 0/1. Persists the
    /// shared `CameraDetect` DWORD as `arg + 1`.
    CameraAssist(bool),
    /// HPD RPC `SetPresenceLeaveDistanceAutoAdjust`, op 15, 0/1.
    LeaveAutoAdjust(bool),
    /// `ENABLE_CAMERADETECT` (0x000D0201), shoulder-surf privacy alert.
    PrivacyAlert(bool),
    /// `ENABLE_ATTENTION_TRACKING` (0x00160101), 0/1.
    AttentionTracking(bool),
}

impl SenseFeature {
    /// `(wire opnum, argument)` for the HPD RPC settings surface.
    #[must_use]
    pub const fn rpc_command(self) -> Option<(u32, u32)> {
        match self {
            Self::LockOnLeave(enabled) => Some((11, enabled as u32)),
            Self::LeaveDistance(distance) => Some((14, distance.raw())),
            Self::LeaveWait(wait) => Some((12, wait.raw())),
            Self::WakeOnApproach(enabled) => Some((8, enabled as u32)),
            Self::ApproachDistance(distance) => Some((9, distance.raw())),
            Self::PauseVideo(enabled) => Some((16, enabled as u32)),
            Self::ApproachAutoAdjust(enabled) => Some((10, enabled as u32)),
            Self::CameraAssist(enabled) => Some((13, enabled as u32)),
            Self::LeaveAutoAdjust(enabled) => Some((15, enabled as u32)),
            Self::PrivacyAlert(_) | Self::AttentionTracking(_) => None,
        }
    }

    /// Legacy pipe commands that have no corresponding recovered HPD RPC
    /// operation. HPD operations never silently fall back to this channel.
    #[must_use]
    pub const fn pipe_command(self) -> Option<(u32, u32)> {
        match self {
            Self::PrivacyAlert(enabled) => Some((0x000D_0201, if enabled { 1 } else { 2 })),
            Self::AttentionTracking(enabled) => Some((0x0016_0101, enabled as u32)),
            _ => None,
        }
    }
}

/// Encode one settings-pipe request frame exactly like the vendor
/// `CMessageBuilder`: `u32 code` then `u32 arg`, both little-endian,
/// nothing else. There is no length prefix and no handshake.
#[must_use]
pub fn encode_request(code: u32, arg: u32) -> [u8; 8] {
    let mut frame = [0u8; 8];
    frame[..4].copy_from_slice(&code.to_le_bytes());
    frame[4..].copy_from_slice(&arg.to_le_bytes());
    frame
}

/// SmartSense human-presence settings control.
///
/// HPD transport: `ncalrpc:[HumanPresenceDetectionEndpoint]`, with a context
/// handle owned by the client. Independent persistence/readback is the
/// `HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`
/// DWORD store. The sensing pipeline itself (VL53L5 ToF + IR camera) stays
/// inside the vendor service — this surface only toggles settings.
pub trait SenseControl: Send + Sync {
    fn sense_state(&self) -> Result<SenseState>;

    /// Apply one settings change; commit mode writes the recovered channel
    /// and polls the independent registry value for that feature (bounded).
    fn set_sense_feature(
        &self,
        feature: SenseFeature,
        apply: ApplyMode,
    ) -> Result<ChangeReport<SenseState>>;
}

#[cfg(test)]
mod tests {
    use super::{SenseFeature, encode_request};
    use lctrl_core::{SenseDistance, SenseWait};

    #[test]
    fn hpd_settings_use_wire_opnums_not_export_order() {
        assert_eq!(SenseFeature::LockOnLeave(true).rpc_command(), Some((11, 1)));
        assert_eq!(
            SenseFeature::WakeOnApproach(false).rpc_command(),
            Some((8, 0))
        );
        assert_eq!(SenseFeature::PauseVideo(true).rpc_command(), Some((16, 1)));
        assert_eq!(SenseFeature::PauseVideo(false).rpc_command(), Some((16, 0)));
        assert_eq!(
            SenseFeature::LeaveDistance(SenseDistance::Far).rpc_command(),
            Some((14, 2))
        );
        assert_eq!(
            SenseFeature::LeaveWait(SenseWait::Long).rpc_command(),
            Some((12, 2))
        );
        assert_eq!(
            SenseFeature::ApproachDistance(SenseDistance::Middle).rpc_command(),
            Some((9, 1))
        );
        assert_eq!(SenseFeature::PauseVideo(true).pipe_command(), None);
    }

    #[test]
    fn separate_legacy_features_keep_their_pipe_encoding() {
        let (code, arg) = SenseFeature::PrivacyAlert(false).pipe_command().unwrap();
        assert_eq!(encode_request(code, arg), [1, 2, 13, 0, 2, 0, 0, 0]);
        assert_eq!(
            SenseFeature::AttentionTracking(true).pipe_command(),
            Some((0x0016_0101, 1))
        );
        assert_eq!(SenseFeature::PrivacyAlert(true).rpc_command(), None);
    }
}
