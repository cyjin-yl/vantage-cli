use lctrl_core::{ApplyMode, ChangeReport, DolbyProfile, DolbyState, Result};

/// Dolby audio control through the vendor DAX RPC endpoint.
///
/// The transport is the vendored `DolbyRpcClient.dll` C exports (ALPC
/// `ncalrpc:[DaxRpcEndpoint]` against DAX3API.exe). Only toggle/profile
/// operations are exposed; the licensed APO DSP is never touched.
/// See `docs/RE-interfaces.md` §12.
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
