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
