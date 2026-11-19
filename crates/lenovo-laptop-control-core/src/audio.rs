use serde::Serialize;

/// Live Dolby audio state read back from the DAX RPC endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DolbyState {
    pub enabled: bool,
    pub active_profile: DolbyProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spatial_supported: Option<bool>,
}

impl std::fmt::Display for DolbyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "enabled={}, profile={:?}",
            if self.enabled { "on" } else { "off" },
            self.active_profile,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DolbyProfile {
    Movie,
    Music,
    Game,
    Voice,
    Personalize,
    Dynamic,
    Off,
}

impl DolbyProfile {
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Movie => 0,
            Self::Music => 1,
            Self::Game => 2,
            Self::Voice => 3,
            Self::Personalize => 4,
            Self::Dynamic => 5,
            Self::Off => 6,
        }
    }

    #[must_use]
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Movie),
            1 => Some(Self::Music),
            2 => Some(Self::Game),
            3 => Some(Self::Voice),
            4 => Some(Self::Personalize),
            5 => Some(Self::Dynamic),
            6 => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseCancellationMode {
    Off,
    Shared,
    Single,
    Spatial,
    VoiceId,
    Unknown(u8),
}

impl NoiseCancellationMode {
    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Off,
            1 => Self::Shared,
            2 => Self::Single,
            3 => Self::Spatial,
            4 => Self::VoiceId,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Shared => 1,
            Self::Single => 2,
            Self::Spatial => 3,
            Self::VoiceId => 4,
            Self::Unknown(raw) => raw,
        }
    }
}

/// Live noise-cancellation state read back from the Elevoc RPC endpoint
/// (docs/interfaces.md §14).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct NoiseCancelState {
    /// Uplink microphone mode (`GetCurrentMode`, SDK domain 0–7; the
    /// legacy `_td` aliases 6/7 are remapped to 1/2 by the SDK before send,
    /// so the wire readback never contains them).
    pub mic_mode: NoiseCancellationMode,
    /// Speaker-side (downlink) state mirrored from the SmartEngine F4006
    /// cache (`noise_speaker_mode`). The authoritative downlink toggle is
    /// `SetDownLinkStatus`; the registry mirror is observation only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker_enabled: Option<bool>,
    /// VoiceID enrollment (`QueryVoiceIDStatus`: 0 = unset, 2 = enrolled);
    /// mode 4 is rejected by the service when not enrolled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_id_enrolled: Option<bool>,
}

impl std::fmt::Display for NoiseCancelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mic mode={:?}", self.mic_mode)?;
        if let Some(enabled) = self.speaker_enabled {
            write!(f, ", speaker={}", if enabled { "on" } else { "off" })?;
        }
        if let Some(enrolled) = self.voice_id_enrolled {
            write!(
                f,
                ", voice-id {}",
                if enrolled { "enrolled" } else { "not enrolled" }
            )?;
        }
        Ok(())
    }
}

/// Direction of one audio endpoint (`EDataFlow`), never merged into the
/// other direction: a render endpoint and a capture endpoint with similar
/// names are different devices with different state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDataFlow {
    Render,
    Capture,
}

impl AudioDataFlow {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Capture => "capture",
        }
    }
}

impl std::fmt::Display for AudioDataFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `DEVICE_STATE_*` of one audio endpoint as the MMDevice API reports it.
///
/// The value is the API answer, not the audio service's registry cache word:
/// the cache carries private flag bits above the documented mask
/// (docs/evidence/re-audio-endpoints-20261105.md §3.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEndpointState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
    /// A `DEVICE_STATE_*` value outside the documented single-bit set.
    Unknown(u32),
}

impl AudioEndpointState {
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Active,
            2 => Self::Disabled,
            4 => Self::NotPresent,
            8 => Self::Unplugged,
            other => Self::Unknown(other),
        }
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            Self::Active => 1,
            Self::Disabled => 2,
            Self::NotPresent => 4,
            Self::Unplugged => 8,
            Self::Unknown(raw) => raw,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::NotPresent => "not_present",
            Self::Unplugged => "unplugged",
            Self::Unknown(_) => "unknown",
        }
    }
}

impl std::fmt::Display for AudioEndpointState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(raw) => write!(f, "unknown(0x{raw:08x})"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// One endpoint from `IMMDeviceEnumerator::EnumAudioEndpoints`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AudioEndpoint {
    /// MMDevice endpoint id, `{0.0.0.00000000}.{guid}` / `{0.0.1.…}`.
    pub id: String,
    /// `PKEY_Device_FriendlyName`; the device description is used only when
    /// the friendly name is absent, and an empty string when both are.
    pub friendly_name: String,
    pub flow: AudioDataFlow,
    pub state: AudioEndpointState,
    /// Channel count of the endpoint's shared-mode mix format. Absent when
    /// the endpoint is not active, because `IAudioEndpointVolume` cannot be
    /// activated for it — `0` would be an invented answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    /// Master volume percent (whole percent, quantized by the endpoint's
    /// volume curve). Absent when the endpoint volume interface is not
    /// reachable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    /// True for the console-role default of this endpoint's flow
    /// (`GetDefaultAudioEndpoint(flow, eConsole)`).
    pub default: bool,
}

impl std::fmt::Display for AudioEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} ", self.flow, self.id)?;
        if self.friendly_name.is_empty() {
            write!(f, "\"\"")?;
        } else {
            write!(f, "\"{}\"", self.friendly_name)?;
        }
        write!(f, ": state={}", self.state)?;
        if let Some(channels) = self.channels {
            write!(f, ", channels={channels}")?;
        }
        if let Some(percent) = self.volume_percent {
            write!(f, ", volume={percent}%")?;
        }
        if let Some(mute) = self.mute {
            write!(f, ", mute={}", if mute { "on" } else { "off" })?;
        }
        if self.default {
            write!(f, ", default")?;
        }
        Ok(())
    }
}

/// Master volume and mute of one endpoint, read through
/// `IAudioEndpointVolume`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EndpointVolume {
    pub device_id: String,
    pub friendly_name: String,
    pub percent: u8,
    pub mute: bool,
    /// `GetVolumeStepInfo` step count; `0` when the endpoint reports none.
    /// A hardware volume curve quantizes a write to `100 / steps` percent.
    pub steps: u32,
    pub channels: u32,
    /// `QueryHardwareSupport` mask has `ENDPOINT_HARDWARE_SUPPORT_VOLUME`.
    pub hardware_volume: bool,
    /// `QueryHardwareSupport` mask has `ENDPOINT_HARDWARE_SUPPORT_MUTE`.
    pub hardware_mute: bool,
}

impl std::fmt::Display for EndpointVolume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = if self.friendly_name.is_empty() {
            self.device_id.as_str()
        } else {
            self.friendly_name.as_str()
        };
        write!(
            f,
            "{name}: volume={}% mute={} channels={}",
            self.percent,
            if self.mute { "on" } else { "off" },
            self.channels,
        )
    }
}

/// Convert the endpoint volume scalar (`GetMasterVolumeLevelScalar`, which the
/// API documents as a normalized tape value in `[0,1]`) to whole percent.
///
/// Returns `None` for a non-finite scalar: reporting `0 %` for a broken read
/// would be an invented answer.
#[must_use]
pub fn volume_percent_from_scalar(scalar: f32) -> Option<u8> {
    if !scalar.is_finite() {
        return None;
    }
    let percent = (scalar.clamp(0.0, 1.0) * 100.0).round();
    u8::try_from(percent as i32).ok()
}

/// Convert whole percent to the scalar accepted by
/// `SetMasterVolumeLevelScalar`. `None` for an out-of-domain percentage, so a
/// caller cannot send `1.5` for `150 %`.
#[must_use]
pub fn percent_to_scalar(percent: u8) -> Option<f32> {
    (percent <= 100).then(|| f32::from(percent) / 100.0)
}

/// Whole-percent tolerance for a volume readback, derived from the endpoint's
/// `GetVolumeStepInfo` step count.
///
/// A hardware volume curve quantizes the request to one step
/// (`100 / step_count` percent), so the readback may legitimately land one
/// step away from the requested value. An endpoint that reports no steps is
/// compared within one percent.
#[must_use]
pub fn volume_readback_tolerance_percent(step_count: u32) -> u8 {
    if step_count == 0 {
        return 1;
    }
    let step = 100u32.div_ceil(step_count);
    u8::try_from(step).unwrap_or(u8::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_state_maps_documented_bits_and_keeps_unknown_raw() {
        assert_eq!(AudioEndpointState::from_raw(1), AudioEndpointState::Active);
        assert_eq!(AudioEndpointState::from_raw(2), AudioEndpointState::Disabled);
        assert_eq!(
            AudioEndpointState::from_raw(4),
            AudioEndpointState::NotPresent
        );
        assert_eq!(
            AudioEndpointState::from_raw(8),
            AudioEndpointState::Unplugged
        );
        // The audiosrv cache word, not a DEVICE_STATE value.
        assert_eq!(
            AudioEndpointState::from_raw(0x2100_0004),
            AudioEndpointState::Unknown(0x2100_0004)
        );
        assert_eq!(
            AudioEndpointState::Unknown(0x2100_0004).to_string(),
            "unknown(0x21000004)"
        );
        assert_eq!(AudioEndpointState::Active.raw(), 1);
    }

    #[test]
    fn flow_names_are_distinct_and_serialized_apart() {
        assert_eq!(AudioDataFlow::Render.as_str(), "render");
        assert_eq!(AudioDataFlow::Capture.as_str(), "capture");
        let endpoint = AudioEndpoint {
            id: "{0.0.0.00000000}.{2a8e3cb6-b4cf-4f88-877d-0931eec0f848}".into(),
            friendly_name: "Speakers".into(),
            flow: AudioDataFlow::Render,
            state: AudioEndpointState::Active,
            channels: Some(2),
            volume_percent: Some(42),
            mute: Some(false),
            default: true,
        };
        let json = serde_json::to_value(&endpoint).unwrap();
        assert_eq!(json["flow"], "render");
        assert_eq!(json["state"], "active");
        assert_eq!(json["channels"], 2);
        assert_eq!(json["default"], true);

        let inactive = AudioEndpoint {
            channels: None,
            volume_percent: None,
            mute: None,
            state: AudioEndpointState::NotPresent,
            ..endpoint
        };
        let json = serde_json::to_value(&inactive).unwrap();
        assert!(json.get("channels").is_none());
        assert!(json.get("volume_percent").is_none());
        assert!(json.get("mute").is_none());
        assert_eq!(json["state"], "not_present");
    }

    #[test]
    fn scalar_and_percent_round_trip_without_inventing_values() {
        assert_eq!(volume_percent_from_scalar(0.0), Some(0));
        assert_eq!(volume_percent_from_scalar(1.0), Some(100));
        assert_eq!(volume_percent_from_scalar(0.424), Some(42));
        assert_eq!(volume_percent_from_scalar(f32::NAN), None);
        assert_eq!(volume_percent_from_scalar(f32::INFINITY), None);
        assert_eq!(percent_to_scalar(0), Some(0.0));
        assert_eq!(percent_to_scalar(100), Some(1.0));
        assert_eq!(percent_to_scalar(101), None);
        for percent in 0..=100u8 {
            let scalar = percent_to_scalar(percent).unwrap();
            assert_eq!(volume_percent_from_scalar(scalar), Some(percent));
        }
    }

    #[test]
    fn readback_tolerance_follows_the_endpoint_step_curve() {
        // 50 steps: one step is 2 percent.
        assert_eq!(volume_readback_tolerance_percent(50), 2);
        assert_eq!(volume_readback_tolerance_percent(100), 1);
        // No reported steps: compare within one percent, never exact equality
        // on a float-derived value.
        assert_eq!(volume_readback_tolerance_percent(0), 1);
        // A degenerate one-step curve implies a full-range tolerance.
        assert_eq!(volume_readback_tolerance_percent(1), 100);
    }

    #[test]
    fn endpoint_volume_display_names_the_endpoint() {
        let volume = EndpointVolume {
            device_id: "{0.0.0.00000000}.{2a8e3cb6-b4cf-4f88-877d-0931eec0f848}".into(),
            friendly_name: "Speakers".into(),
            percent: 42,
            mute: false,
            steps: 50,
            channels: 2,
            hardware_volume: true,
            hardware_mute: true,
        };
        assert_eq!(
            volume.to_string(),
            "Speakers: volume=42% mute=off channels=2"
        );
    }
}
