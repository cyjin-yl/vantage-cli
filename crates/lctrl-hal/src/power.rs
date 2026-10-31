use lctrl_core::{
    ApplyMode, ChangeReport, PowerMutation, PowerScheme, PowerSchemeId, PowerSettingKey,
    PowerSettingObservation, PowerSourceValues, PowerValueRange, ProcessorBoostMode, Result,
    UsbChargingState,
};

/// Runtime USB charging control (AlwaysOnUSB AC mode and charge-from-USB in
/// battery mode). The RE-proven EnergyDrv `0x831020e8` contract backs the
/// Windows implementation (docs/RE-interfaces.md §11); BIOS-persistent
/// variants are separate `BIOS_SET` transactions, not part of this trait.
pub trait UsbControl: Send + Sync {
    fn usb_charging(&self) -> Result<UsbChargingState>;
    fn set_usb_ac_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<UsbChargingState>>;
    fn set_usb_battery_charging(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<UsbChargingState>>;
}

pub trait PowerControl: Send + Sync {
    fn power_schemes(&self) -> Result<Vec<PowerScheme>>;
    fn active_power_scheme(&self) -> Result<PowerScheme>;
    fn power_value_range(&self, key: &PowerSettingKey) -> Result<PowerValueRange>;
    /// Read a setting from the specified scheme without changing it. Values,
    /// range, and policy overrides are sampled separately, not atomically.
    fn read_power_setting(
        &self,
        scheme: &PowerSchemeId,
        key: &PowerSettingKey,
    ) -> Result<PowerSettingObservation>;
    fn apply_power_mutation(
        &self,
        mutation: PowerMutation,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerMutation>>;
}

/// Source-aware processor policies stored in the active Windows power scheme.
///
/// EPP methods use the native `PERFEPP` unit: an integer percentage in
/// `0..=100`. This keeps that API distinct from `TuningControl`'s raw HWP
/// `0..=255` unit. Snapshot methods preserve AC/DC and boost-mode differences.
pub trait ProcessorPowerControl: Send + Sync {
    fn epp_percent_ac_dc(&self) -> Result<PowerSourceValues<u32>>;
    fn set_epp_percent_ac_dc(
        &self,
        values: PowerSourceValues<u32>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<u32>>>;

    fn boost_ac_dc(&self) -> Result<PowerSourceValues<ProcessorBoostMode>>;
    fn set_boost_ac_dc(
        &self,
        values: PowerSourceValues<ProcessorBoostMode>,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<ProcessorBoostMode>>>;

    /// Select `Disabled` (0) or `Enabled` (1) for both power sources.
    /// Aggressive and efficiency variants are preserved in snapshots but are
    /// never silently selected by a boolean turbo request.
    fn set_turbo(
        &self,
        enabled: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<PowerSourceValues<ProcessorBoostMode>>> {
        let requested = if enabled {
            ProcessorBoostMode::ENABLED
        } else {
            ProcessorBoostMode::DISABLED
        };
        self.set_boost_ac_dc(PowerSourceValues::same(requested), apply)
    }
}
