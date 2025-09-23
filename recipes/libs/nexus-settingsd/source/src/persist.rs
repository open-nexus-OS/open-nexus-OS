//! Simple per-user persistence using XDG config. Atomic writes.

use crate::model::Settings;
use std::{fs, io::{self, Write}, path::PathBuf};
use dirs_next as dirs;

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("nexus");
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("nexus")
}

fn state_path() -> PathBuf {
    config_dir().join("state.toml")
}

pub fn load() -> Settings {
    let path = state_path();
    match fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save_atomic(s: &Settings) -> io::Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir).ok();
    let path = state_path();
    let tmp  = path.with_extension("toml.tmp");

    let toml = toml::to_string_pretty(s).unwrap_or_default();
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(toml.as_bytes())?;
        let _ = f.sync_all();
    }
    fs::rename(tmp, path)?;
    Ok(())
}
