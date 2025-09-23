//! nexus-settingsd (library stub)
//!
//! Minimal settings service for system-wide UI state:
//! - UIMode (Desktop/Mobile)
//! - ThemeMode (Light/Dark)
//!
//! This stub persists to a single TOML file and keeps an in-process cache.
//! Later you can replace the storage with a daemon + IPC without changing callers.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};

/// UI layout mode used by shell/launcher.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UIMode { Desktop, Mobile }

/// Visual theme mode used by THEME manager.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode { Light, Dark }

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Settings {
    ui_mode: UIMode,
    theme_mode: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self { ui_mode: UIMode::Desktop, theme_mode: ThemeMode::Light }
    }
}

static STATE: Lazy<Mutex<Settings>> = Lazy::new(|| {
    Mutex::new(load_from_disk().unwrap_or_default())
});

/// Path where this stub persists settings (adjust later to a persistent location).
fn settings_path() -> String {
    // Allow override via env if useful in CI/build
    if let Ok(p) = std::env::var("NEXUS_SETTINGS_PATH") {
        return p;
    }
    // Safe temporary default; swap to a persistent path once available.
    "/tmp/nexus-settings.toml".to_string()
}

fn load_from_disk() -> Option<Settings> {
    let path = settings_path();
    let mut f = fs::File::open(&path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    toml::from_str::<Settings>(&s).ok()
}

fn save_to_disk(s: &Settings) {
    let path = settings_path();
    if let Ok(t) = toml::to_string_pretty(s) {
        if let Ok(mut f) = fs::File::create(&path) {
            let _ = f.write_all(t.as_bytes());
            let _ = f.flush();
        }
    }
}

/// Get current UI mode.
pub fn get_ui_mode() -> UIMode {
    STATE.lock().ui_mode
}

/// Set UI mode and persist.
pub fn set_ui_mode(mode: UIMode) {
    let mut st = STATE.lock();
    if st.ui_mode != mode {
        st.ui_mode = mode;
        save_to_disk(&st);
        log::debug!("nexus-settingsd: ui_mode -> {:?}", mode);
    }
}

/// Get current theme mode.
pub fn get_theme_mode() -> ThemeMode {
    STATE.lock().theme_mode
}

/// Set theme mode and persist.
pub fn set_theme_mode(mode: ThemeMode) {
    let mut st = STATE.lock();
    if st.theme_mode != mode {
        st.theme_mode = mode;
        save_to_disk(&st);
        log::debug!("nexus-settingsd: theme_mode -> {:?}", mode);
    }
}

/// Replace whole settings (optional helper).
pub fn set_all(ui_mode: UIMode, theme_mode: ThemeMode) {
    let mut st = STATE.lock();
    st.ui_mode = ui_mode;
    st.theme_mode = theme_mode;
    save_to_disk(&st);
}

/// Read both settings at once.
pub fn get_all() -> (UIMode, ThemeMode) {
    let st = STATE.lock();
    (st.ui_mode, st.theme_mode)
}
