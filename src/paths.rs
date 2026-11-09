use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let root = match std::env::var_os("RUSTBLOX_HOME") {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => dirs::data_local_dir()
                .ok_or_else(|| anyhow!("cannot resolve the local data directory"))?
                .join("RustBlox"),
        };
        let me = Self { root };
        for d in [me.versions(), me.mods(), me.backups(), me.logs(), me.cache()] {
            fs::create_dir_all(&d)?;
        }
        Ok(me)
    }
    pub fn config(&self) -> PathBuf { self.root.join("config.json") }
    pub fn versions(&self) -> PathBuf { self.root.join("Versions") }
    pub fn mods(&self) -> PathBuf { self.root.join("Mods") }
    pub fn backups(&self) -> PathBuf { self.root.join("Backups") }
    pub fn logs(&self) -> PathBuf { self.root.join("Logs") }
    pub fn cache(&self) -> PathBuf { self.root.join("Cache") }
    pub fn diagnostics(&self) -> PathBuf { self.root.join("Diagnostics") }
    pub fn news_cache(&self) -> PathBuf { self.cache().join("news.json") }
    pub fn history(&self) -> PathBuf { self.root.join("activity_history.json") }
    pub fn live(&self) -> PathBuf { self.cache().join("live.json") }
    pub fn applied(&self) -> PathBuf { self.root.join("applied_mods.json") }
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn prev_handler(&self) -> PathBuf { self.root.join("previous_handlers.json") }
    pub fn lock(&self) -> PathBuf { self.root.join("install.lock") }
}

/// Where the Roblox player writes its logs (independent of where it is installed).
pub fn roblox_logs_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Roblox").join("logs"))
}
