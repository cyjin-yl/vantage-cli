use lctrl_core::{BiosChange, BiosItem, BiosPasswordStatus, Result};

/// Read and stage the verified subset of Lenovo's BIOS WMI contract.
///
/// Staging and saving acknowledge only the WMI transaction state. They do not
/// claim that a setting is effective before a reboot or other firmware action.
pub trait BiosControl: Send + Sync {
    fn list(&self) -> Result<Vec<BiosItem>>;
    fn get(&self, name: &str) -> Result<Option<BiosItem>>;
    fn selections(&self, name: &str) -> Result<Vec<String>>;
    fn stage(&self, change: BiosChange) -> Result<()>;
    fn save(&self) -> Result<()>;
    fn password_status(&self) -> Result<BiosPasswordStatus>;
}
