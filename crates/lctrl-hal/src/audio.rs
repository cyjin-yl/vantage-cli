use lctrl_core::{
    ApplyMode, ChangeReport, DolbyProfile, DolbyState, NoiseCancelState, NoiseCancellationMode,
    Result,
};

/// Dolby audio control through the vendor DAX RPC endpoint.
///
/// The transport is a pure-Rust `rpcrt4.dll` NDR client (ALPC
/// `ncalrpc:[DaxRpcEndpoint]` against DAX3API.exe); no Lenovo-delivered
/// binary is loaded. Only toggle/profile operations are exposed; the
/// licensed APO DSP is never touched. See `docs/interfaces.md` §12.
pub trait AudioControl: Send + Sync {
    fn dolby_state(&self) -> Result<DolbyState>;

    /// Apply a Dolby profile. `DolbyProfile::Off` disables Dolby; any other
    /// profile enables Dolby and selects the profile index. Dry-run reports
    /// the requested state without calling the RPC write exports.
    fn set_dolby_profile(
        &self,
        profile: DolbyProfile,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DolbyState>>;
}

/// Noise-cancellation control through the vendor Elevoc RPC endpoint
/// (docs/interfaces.md §14). Only the microphone mode is exposed; the
/// DSP pipeline stays inside ElevocControlService.
pub trait NoiseCancelControl: Send + Sync {
    fn noise_cancel_state(&self) -> Result<NoiseCancelState>;

    /// Set the uplink microphone mode. `NoiseCancellationMode::Off` selects
    /// SDK mode 0 (`environmental`), which is the closest the endpoint has
    /// to "off" — there is no dedicated off value on the wire.
    fn set_noise_cancel_mode(
        &self,
        mode: NoiseCancellationMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<NoiseCancelState>>;
}
