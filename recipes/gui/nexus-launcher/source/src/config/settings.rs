// src/config/settings.rs
// User-facing settings and small global switches with atomic storage.
// We keep these atomics lightweight; anything heavier (persistence, IPC)
// should live in a dedicated settings service/daemon.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// UI layout mode: Desktop or Mobile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode { Desktop = 0, Mobile = 1 }

static MODE: AtomicU8 = AtomicU8::new(Mode::Desktop as u8);

/// Set current launcher mode.
pub fn set_mode(mode: Mode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}
/// Get current launcher mode.
pub fn mode() -> Mode {
    if MODE.load(Ordering::Relaxed) == 1 { Mode::Mobile } else { Mode::Desktop }
}

/// Theme preference (Light/Dark). This mirrors libnexus::ThemeId and
/// calls into THEME.switch_theme so the new palette applies immediately.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThemeMode { Light = 0, Dark = 1 }

static THEME_MODE: AtomicU8 = AtomicU8::new(ThemeMode::Light as u8);

/// Set current theme and switch libnexus THEME at runtime.
pub fn set_theme_mode(theme: ThemeMode) {
    THEME_MODE.store(theme as u8, Ordering::Relaxed);

    // Propagate to libnexus theme manager (hot-swap colors + clear caches)
    let id = match theme {
        ThemeMode::Light => libnexus::themes::manager::ThemeId::Light,
        ThemeMode::Dark  => libnexus::themes::manager::ThemeId::Dark,
    };
    libnexus::themes::THEME.switch_theme(id);
}

/// Get current theme mode.
pub fn theme_mode() -> ThemeMode {
    if THEME_MODE.load(Ordering::Relaxed) == ThemeMode::Dark as u8 {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

/// Start menu preferences.
#[derive(Copy, Clone, Debug)]
pub struct StartMenuConfig {
    /// Whether the desktop menu starts in large (expanded) mode (false = small panel).
    pub desktop_large: bool,
}
impl Default for StartMenuConfig {
    fn default() -> Self { Self { desktop_large: false } }
}

static DESKTOP_LARGE: AtomicU8 = AtomicU8::new(0);

pub fn set_desktop_large(enabled: bool) {
    DESKTOP_LARGE.store(if enabled {1} else {0}, Ordering::Relaxed);
}
pub fn desktop_large() -> bool {
    DESKTOP_LARGE.load(Ordering::Relaxed) == 1
}

/// -------- GLOBAL INSETS --------
/// Top inset in pixels reserved by the ActionBar (so large menus don't cover it).
/// Set from the ActionBar path via `set_top_inset()` and read in desktop/mobile menus.
static TOP_INSET: AtomicU32 = AtomicU32::new(0);

pub fn set_top_inset(px: u32) { TOP_INSET.store(px, Ordering::Relaxed); }
pub fn top_inset() -> u32 { TOP_INSET.load(Ordering::Relaxed) }

/// -------- UI CONSTANTS --------
/// Keep these as constants so hot paths don’t need atomics.
pub const BAR_HEIGHT: u32 = 54;         // original bar height
pub const ICON_SCALE: f32 = 0.685;      // 37/54 = 0.685
pub const ICON_SMALL_SCALE: f32 = 0.75; // 75% of bar height for small icons
