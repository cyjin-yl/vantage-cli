use lctrl_core::{Result, SensorInventory, SensorReading};

/// Read-only access to the operating system's sensors.
///
/// The trait deliberately has no write surface: report intervals, thresholds,
/// and event sinks are owned by the platform's sensor service, and nothing in
/// this CLI needs to subscribe. A platform without a verified sensor channel
/// reports [`lctrl_core::LctrlError::Unsupported`] instead of an empty inventory,
/// so "no such code path here" is never confused with "this machine has no
/// sensors".
pub trait SensorControl: Send + Sync {
    /// Enumerate the sensors this platform exposes and take one report from each.
    ///
    /// Sensors that were enumerated but produced no report are returned in
    /// [`SensorInventory::unavailable`] rather than being dropped or given a
    /// zero value. Ordering is the platform's enumeration order.
    fn list(&self) -> Result<SensorInventory>;

    /// Read one sensor, selected by list index, device path, or friendly name.
    ///
    /// Selection rules are shared: see [`lctrl_core::match_sensor`]. A target
    /// that matches no enumerated sensor is `Unsupported` (the sensor is not
    /// present on this machine); an ambiguous target is `InvalidArgument`.
    fn read(&self, target: &str) -> Result<SensorReading>;
}
