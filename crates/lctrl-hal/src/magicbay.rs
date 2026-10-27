use lctrl_core::{MagicBayInventory, Result};

pub trait MagicBayControl: Send + Sync {
    fn detect_magicbay(&self) -> Result<MagicBayInventory>;
}
