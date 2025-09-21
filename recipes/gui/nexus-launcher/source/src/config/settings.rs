// config/settings.rs - Configuration settings and constants

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode { Desktop = 0, Mobile = 1 }

static MODE: AtomicU8 = AtomicU8::new(Mode::Desktop as u8);

pub fn set_mode(mode: Mode) { MODE.store(mode as u8, Ordering::Relaxed); }
pub fn mode() -> Mode { if MODE.load(Ordering::Relaxed) == 1 { Mode::Mobile } else { Mode::Desktop } }

#[derive(Copy, Clone, Debug)]
pub struct StartMenuConfig {
    /// Whether the desktop menu starts in small (panel) or large (expanded) mode
    pub desktop_large: bool,
}

impl Default for StartMenuConfig {
    fn default() -> Self { Self { desktop_large: false } }
}

static DESKTOP_LARGE: AtomicU8 = AtomicU8::new(0);

pub fn set_desktop_large(enabled: bool) { DESKTOP_LARGE.store(if enabled {1} else {0}, Ordering::Relaxed); }
pub fn desktop_large() -> bool { DESKTOP_LARGE.load(Ordering::Relaxed) == 1 }

/// -------- GLOBAL INSETS --------
/// Top inset in pixels reserved by the ActionBar (so large menus don't cover it).
/// Set from main.rs via `set_top_inset(insets.top)` and read in desktop/mobile menus.
static TOP_INSET: AtomicU32 = AtomicU32::new(0);

pub fn set_top_inset(px: u32) { TOP_INSET.store(px, Ordering::Relaxed); }
pub fn top_inset() -> u32 { TOP_INSET.load(Ordering::Relaxed) }

// -------- UI CONSTANTS --------
pub const BAR_HEIGHT: u32 = 54;         // bar height (original: 54px)
pub const ICON_SCALE: f32 = 0.685;      // 37/54 = 0.685
pub const ICON_SMALL_SCALE: f32 = 0.75; // 75% of bar height for small icons