use lctrl_core::{MagicBayDevice, Result};

pub trait MagicBayControl: Send + Sync {
    fn detect_magicbay(&self) -> Result<Vec<MagicBayDevice>>;
}
