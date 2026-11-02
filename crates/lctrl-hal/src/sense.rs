use lctrl_core::{ApplyMode, ChangeReport, Result, SenseDistance, SenseState, SenseWait};

/// One SmartSense settings-pipe command, mapped 1:1 to the recovered
/// command table (docs/interfaces.md §13).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SenseFeature {
    /// `ENABLE_PRESENTLEAVE` (0x00090101), legacy 0/1 boolean.
    LockOnLeave(bool),
    /// `SET_LEAVE_DISTANCE` (0x00110101), Near/Middle/Far.
    LeaveDistance(SenseDistance),
    /// `SET_LEAVE_WAIT` (0x000A0101), Short/Middle/Long.
    LeaveWait(SenseWait),
    /// `ENABLE_APPROACH` (0x00070101), legacy 0/1 boolean.
    WakeOnApproach(bool),
    /// `SET_APPROACH_DISTANCE` (0x00080101), Near/Middle/Far.
    ApproachDistance(SenseDistance),
    /// `ENABLE_VIDEOSTOP` (0x000D0101), 1=enable/2=disable convention.
    PauseVideo(bool),
    /// `ENABLE_CAMERADETECT` (0x000D0201), shoulder-surf privacy alert.
    PrivacyAlert(bool),
    /// `ENABLE_ATTENTION_TRACKING` (0x00160101), 0/1.
    AttentionTracking(bool),
}

impl SenseFeature {
    /// `(command code, argument)` pair for the settings pipe.
    #[must_use]
    pub const fn command(self) -> (u32, u32) {
        match self {
            Self::LockOnLeave(enabled) => (0x0009_0101, enabled as u32),
            Self::LeaveDistance(distance) => (0x0011_0101, distance.raw()),
            Self::LeaveWait(wait) => (0x000A_0101, wait.raw()),
            Self::WakeOnApproach(enabled) => (0x0007_0101, enabled as u32),
            Self::ApproachDistance(distance) => (0x0008_0101, distance.raw()),
            Self::PauseVideo(enabled) => (0x000D_0101, if enabled { 1 } else { 2 }),
            Self::PrivacyAlert(enabled) => (0x000D_0201, if enabled { 1 } else { 2 }),
            Self::AttentionTracking(enabled) => (0x0016_0101, enabled as u32),
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
/// Transport: the `\\.\pipe\com.lenovo.smartsense.settings` named pipe owned
/// by the SmartSense service; persistence/readback is the
/// `HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`
/// DWORD store. The sensing pipeline itself (VL53L5 ToF + IR camera) stays
/// inside the vendor service — this surface only toggles settings.
pub trait SenseControl: Send + Sync {
    fn sense_state(&self) -> Result<SenseState>;

    /// Apply one settings change; commit mode writes the pipe and polls the
    /// registry store until the modeled state matches (bounded).
    fn set_sense_feature(
        &self,
        feature: SenseFeature,
        apply: ApplyMode,
    ) -> Result<ChangeReport<SenseState>>;
}

#[cfg(test)]
mod tests {
    use super::{SenseFeature, encode_request};
    use lctrl_core::SenseDistance;

    // Byte-exact frames verified against the ThinkSmartSenseAddin IL:
    // `u32 code` + `u32 arg`, little-endian, no trailing bytes.
    #[test]
    fn lock_on_leave_frames_match_the_vendor_encoding() {
        let (code, arg) = SenseFeature::LockOnLeave(true).command();
        assert_eq!(code, 0x0009_0101);
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x09, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        let (code, arg) = SenseFeature::LockOnLeave(false).command();
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn wake_on_approach_frames_match_the_vendor_encoding() {
        let (code, arg) = SenseFeature::WakeOnApproach(true).command();
        assert_eq!(code, 0x0007_0101);
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x07, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        let (code, arg) = SenseFeature::WakeOnApproach(false).command();
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn pause_video_uses_the_one_two_convention_not_zero_one() {
        let (code, arg) = SenseFeature::PauseVideo(true).command();
        assert_eq!(code, 0x000D_0101);
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x0D, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
        let (code, arg) = SenseFeature::PauseVideo(false).command();
        assert_eq!(
            encode_request(code, arg),
            [0x01, 0x01, 0x0D, 0x00, 0x02, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn distance_and_wait_commands_encode_their_raw_selector() {
        let (code, arg) = SenseFeature::LeaveDistance(SenseDistance::Far).command();
        assert_eq!(code, 0x0011_0101);
        assert_eq!(
            encode_request(code, arg),
            [
                0x01,
                0x01,
                0x11,
                0x00,
                SenseDistance::Far.raw() as u8,
                0x00,
                0x00,
                0x00
            ]
        );
    }
}
