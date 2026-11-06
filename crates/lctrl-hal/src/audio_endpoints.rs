use lctrl_core::{ApplyMode, AudioEndpoint, ChangeReport, EndpointVolume, Result};

/// Endpoint selector for the per-endpoint verbs: `None` means the
/// console-role render endpoint (`GetDefaultAudioEndpoint(eRender, eConsole)`),
/// never "all endpoints".
pub type EndpointSelector<'a> = Option<&'a str>;

/// OS audio endpoints and their master volume/mute.
///
/// The inventory comes from the MMDevice API (`IMMDeviceEnumerator`), and the
/// volume/mute from `IAudioEndpointVolume` on the addressed endpoint. This is
/// an operating-system surface: it is independent of the vendor audio stacks
/// (Dolby DAX, Elevoc), so an absent vendor service never blocks it.
/// See `docs/evidence/re-audio-endpoints-20261105.md`.
pub trait AudioEndpointControl: Send + Sync {
    /// List endpoints of both data flows. `include_inactive` adds the
    /// disabled / unplugged / not-present endpoints, which carry their state
    /// but no channel or volume answer (the endpoint volume interface cannot
    /// be activated for them).
    fn endpoints(&self, include_inactive: bool) -> Result<Vec<AudioEndpoint>>;

    /// Master volume and mute of one endpoint. `device_id` of `None` selects
    /// the console-role render endpoint; an unknown id is an error and never
    /// falls back to the default.
    fn endpoint_volume(&self, device_id: EndpointSelector<'_>) -> Result<EndpointVolume>;

    /// Set the master volume percent (0..=100). `DryRun` reports the previous
    /// and requested state without calling any `Set` method. A commit reads
    /// the value back and compares it within the endpoint's step tolerance;
    /// on a mismatch the previous value is restored before the error returns.
    fn set_endpoint_volume(
        &self,
        device_id: EndpointSelector<'_>,
        percent: u8,
        apply: ApplyMode,
    ) -> Result<ChangeReport<EndpointVolume>>;

    /// Set the endpoint-wide mute. `DryRun` reports without writing; a commit
    /// reads the flag back and restores the previous value on a mismatch.
    fn set_endpoint_mute(
        &self,
        device_id: EndpointSelector<'_>,
        mute: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<EndpointVolume>>;
}
