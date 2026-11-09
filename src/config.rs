use crate::paths::Paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use tracing::warn;

pub const FPS_FLAG: &str = "DFIntTaskSchedulerTargetFps";

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub fflags: BTreeMap<String, serde_json::Value>,
    pub installed_version: Option<String>,
    pub installed_label: Option<String>,
    pub disabled_mods: BTreeSet<String>,
    pub seen_news: BTreeSet<String>,
    pub show_trivia: bool,
    pub activity_tracking: bool,
    pub server_location: bool,
    pub discord_rpc: bool,
    pub rpc_show_game: bool,
    pub rpc_join_button: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fflags: BTreeMap::new(),
            installed_version: None,
            installed_label: None,
            disabled_mods: BTreeSet::new(),
            seen_news: BTreeSet::new(),
            show_trivia: true,
            activity_tracking: true,
            server_location: true,
            discord_rpc: true,
            rpc_show_game: true,
            rpc_join_button: false,
        }
    }
}

impl Settings {
    pub fn load(p: &Paths) -> Self {
        let path = p.config();
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                warn!("config unreadable ({e}); keeping a copy as config.json.bad and starting fresh");
                let _ = fs::rename(&path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    pub fn save(&self, p: &Paths) -> Result<()> {
        let path = p.config();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn fps(&self) -> Option<u64> {
        self.fflags.get(FPS_FLAG).and_then(|v| v.as_u64())
    }

    pub fn set_fps(&mut self, fps: Option<u64>) {
        match fps {
            Some(v) => { self.fflags.insert(FPS_FLAG.into(), serde_json::Value::from(v)); }
            None => { self.fflags.remove(FPS_FLAG); }
        }
    }
}

pub const FLAG_PREFIXES: [&str; 10] = [
    "FFlag", "DFFlag", "SFFlag", "FInt", "DFInt", "FString", "DFString", "FLog", "DFLog", "FPositive",
];

pub fn valid_flag(name: &str) -> bool {
    FLAG_PREFIXES.iter().any(|p| name.starts_with(p))
        && name.len() > 4
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn parse_value(raw: &str) -> serde_json::Value {
    let raw = raw.trim();
    match raw.to_lowercase().as_str() {
        "true" => serde_json::Value::Bool(true),
        "false" => serde_json::Value::Bool(false),
        _ => match raw.parse::<i64>() {
            Ok(n) => serde_json::Value::from(n),
            Err(_) => serde_json::Value::String(raw.to_string()),
        },
    }
}
