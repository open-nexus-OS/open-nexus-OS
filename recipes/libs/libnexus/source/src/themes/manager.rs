// themes/manager.rs — main theme manager: loads nexus.toml, resolves icons/backgrounds/colors,
// caches rendered assets, and can switch between Light/Dark at runtime.

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::sync::{Mutex, RwLock};
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(not(feature = "svg"))]
use std::sync::atomic::AtomicBool;
use toml;
use log::{warn, error};

use orbclient::Color;
use orbimage::Image;

use crate::themes::colors::{
    Acrylic, Paint, ThemeColorsToml, ColorEntry, to_color, hex_to_color
};
use crate::themes::svg_icons::IconVariant;

#[cfg(not(feature = "svg"))]
static SVG_FEATURE_MISSING_WARNED: AtomicBool = AtomicBool::new(false);

/// Theme selector used across icon/color resolution
#[derive(Copy, Clone, Debug)]
pub enum ThemeId { Light, Dark }

impl ThemeId {
    #[inline] fn as_u8(self) -> u8 { match self { ThemeId::Light => 0, ThemeId::Dark => 1 } }
    #[inline] fn from_u8(v: u8) -> Self { if v == 1 { ThemeId::Dark } else { ThemeId::Light } }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ThemeSection {
    #[serde(default)]
    pub current: Option<String>,
}

// nexus.toml we care about: [theme], optional [colors], and any other tables (icons, backgrounds)
#[derive(Debug, Clone, Deserialize)]
struct NexusToml {
    #[serde(default)] theme: ThemeSection,
    #[allow(dead_code)]
    #[serde(default)] colors: Option<toml::Value>,
    #[serde(flatten)] tables: BTreeMap<String, toml::Value>,
}

fn collect_icon_ids(
    out: &mut BTreeMap<String, String>,
    prefix: &str,
    table: &toml::map::Map<String, toml::Value>,
) {
    for (k, v) in table {
        let full = format!("{}.{}", prefix, k);
        match v {
            toml::Value::String(s) => { out.insert(full, s.clone()); }
            toml::Value::Table(t)  => { collect_icon_ids(out, &full, t); }
            _ => {}
        }
    }
}

/// Thread-safe theme manager:
/// - Current ThemeId stored in AtomicU8
/// - Paints and legacy color map behind RwLock (hot-swappable)
/// - Icon and background caches behind Mutex
pub struct ThemeManager {
    theme: AtomicU8, // 0 = Light, 1 = Dark

    /// logical name -> relative icon path (e.g. "actions/system-shut-down")
    icons: BTreeMap<String, String>,
    /// background name -> relative path (e.g. "backgrounds/login")
    backgrounds: BTreeMap<String, String>,

    /// legacy color map (flat Color, no acrylic), derived from paints
    pub(crate) colors_legacy: RwLock<BTreeMap<String, Color>>,
    /// new paint map (Color + optional Acrylic)
    paints: RwLock<BTreeMap<String, Paint>>,

    /// in-process cache for icons
    cache: Mutex<BTreeMap<(String, IconVariant, Option<(u32,u32)>), Image>>,
    /// in-process cache for backgrounds
    bg_cache: Mutex<BTreeMap<(String, Option<(u32,u32)>), Image>>,
}

impl ThemeManager {
    /// Current ThemeId (lock-free)
    #[inline]
    pub fn theme_id(&self) -> ThemeId {
        ThemeId::from_u8(self.theme.load(Ordering::Relaxed))
    }

    #[inline]
    fn set_theme_id(&self, id: ThemeId) {
        self.theme.store(id.as_u8(), Ordering::Relaxed);
    }

    /// Build icon candidates for a given logical id and variant.
    fn candidates_for(&self, id: &str, variant: IconVariant) -> Vec<String> {
        let rel: String = if let Some(s) = self.icons.get(id) {
            s.clone()
        } else if let Some(s) = self.icons.get(&format!("icons.{}", id)) {
            s.clone()
        } else {
            id.to_string()
        };
        crate::themes::svg_icons::icon_candidates(&rel, self.theme_id(), variant)
    }

    /// Load an icon, possibly SVG-rendered, with an optional target size.
    pub fn load_icon_sized(
        &self,
        id: &str,
        variant: IconVariant,
        size: Option<(u32, u32)>,
    ) -> Option<Image> {
        let key = (id.to_string(), variant, size);

        // Cache lookup
        if let Some(img) = self.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(img);
        }

        let theme_now = self.theme_id();
        let candidates = self.candidates_for(id, variant);
        if candidates.is_empty() {
            error!("no icon candidates for id={id} variant={variant:?}");
            return None;
        }

        for path in &candidates {
            if path.ends_with(".svg") {
                // ---- SVG branch ----
                #[cfg(feature = "svg")]
                {
                    match std::fs::read(path) {
                        Ok(svg_bytes) => {
                            match crate::themes::svg_icons::render_svg_to_image_with_theme(
                                &svg_bytes, theme_now, size
                            ) {
                                Some(img) => {
                                    let _ = self.cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
                                    return Some(img);
                                }
                                None => {
                                    error!("SVG render failed: id={id} path={path} size={size:?}");
                                }
                            }
                        }
                        Err(e) => {
                            warn!("cannot read SVG: id={id} path={path} err={e}");
                        }
                    }
                }
                #[cfg(not(feature = "svg"))]
                {
                    if !SVG_FEATURE_MISSING_WARNED.swap(true, Ordering::Relaxed) {
                        error!(
                            "libnexus was built WITHOUT `svg` feature; cannot load SVG icons (first hit: id={id}, path={path})"
                        );
                        error!("→ Build fix: enable the feature either by making it default in libnexus or by setting features=[\"svg\"] in every consumer (e.g. nexus).");
                    }
                }
            } else {
                // ---- Raster branch (optional) ----
                match Image::from_path(path) {
                    Ok(mut img) => {
                        if let Some((w, h)) = size {
                            img = crate::themes::svg_icons::scale_nearest(&img, w, h);
                        }
                        let _ = self.cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
                        return Some(img);
                    }
                    Err(e) => {
                        warn!("cannot read raster icon: id={id} path={path} err={e}");
                    }
                }
            }
        }

        error!(
            "icon NOT FOUND: id={id} variant={variant:?} size={size:?}\n  tried:\n    - {}",
            candidates.join("\n    - ")
        );
        None
    }

    /// Compatibility wrapper as before: without size argument.
    #[inline]
    pub fn load_icon(&self, id: &str, variant: IconVariant) -> Option<Image> {
        self.load_icon_sized(id, variant, None)
    }

    /// Compatibility wrapper for named colors (legacy fallback).
    pub fn color(&self, name: &str, fallback: Color) -> Color {
        self.colors_legacy
            .read()
            .ok()
            .and_then(|m| m.get(name).copied())
            .unwrap_or(fallback)
    }

    /// Load a themed background (JPG/JPEG), first try theme-specific, then global.
    /// Caches by (name, size) in `bg_cache`.
    pub fn load_background(&self, name: &str, size: Option<(u32, u32)>) -> Option<Image> {
        let key = (name.to_string(), size);
        if let Some(img) = self.bg_cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(img);
        }

        // Relative path from nexus.toml, e.g. "backgrounds/login"
        let rel = self
            .backgrounds
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("backgrounds/{}", name));

        let theme_name = match self.theme_id() {
            ThemeId::Light => "light",
            ThemeId::Dark  => "dark",
        };

        // Priority: themes → global; only .jpg/.jpeg
        let candidates = [
            format!("/ui/themes/{}/{}.jpg", theme_name, rel),
            format!("/ui/themes/{}/{}.jpeg", theme_name, rel),
            format!("/ui/{}.jpg", rel),
            format!("/ui/{}.jpeg", rel),
        ];

        for p in candidates {
            if let Ok(mut img) = Image::from_path(&p) {
                if let Some((w, h)) = size {
                    img = crate::themes::svg_icons::scale_nearest(&img, w, h);
                }
                let _ = self.bg_cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
                return Some(img);
            }
        }
        None
    }

    /// Optional: quick diagnose which candidates exist
    pub fn debug_icon_candidates(&self, id: &str, variant: IconVariant) {
        let cands = self.candidates_for(id, variant);
        if cands.is_empty() {
            error!("debug: no candidates for id={id} variant={variant:?}");
            return;
        }
        for p in cands {
            let _exists = std::fs::metadata(&p).is_ok();
            let _kind = if p.ends_with(".svg") { "svg" } else { "ras" };
        }
    }
}

/// Load paints (colors + optional acrylic) for a given theme.
/// Returns (paints, legacy_colors) ready to swap into the manager.
fn load_paints_for(theme: ThemeId) -> (BTreeMap<String, Paint>, BTreeMap<String, Color>) {
    let mut paints: BTreeMap<String, Paint> = BTreeMap::new();
    let mut colors_legacy: BTreeMap<String, Color> = BTreeMap::new();

    let theme_name = match theme { ThemeId::Light => "light", ThemeId::Dark => "dark" };
    let colors_path = format!("/ui/themes/{}/colors.toml", theme_name);

    if let Ok(txt) = fs::read_to_string(&colors_path) {
        match toml::from_str::<ThemeColorsToml>(&txt) {
            Ok(doc) => {
                // Defaults for acrylic if not specified per key
                let def = doc.defaults.acrylic.unwrap_or_default();
                let d_enabled   = def.enabled.unwrap_or(false);
                let d_downscale = def.downscale.unwrap_or(4);
                let d_tint      = def.tint
                    .as_deref()
                    .and_then(|s| hex_to_color(s))
                    .unwrap_or(Color::rgba(255,255,255,0));
                let d_noise     = def.noise_alpha.unwrap_or(0);

                for (name, entry) in doc.colors {
                    let (color, acrylic) = match entry {
                        ColorEntry::Array(v) => (to_color(&v), None),
                        ColorEntry::Hex(h)   => (hex_to_color(&h).unwrap_or(Color::rgba(255,255,255,255)), None),
                        ColorEntry::Table(t) => {
                            let color = t.rgba
                                .as_ref().map(|v| to_color(v))
                                .or_else(|| t.hex.as_deref().and_then(|s| hex_to_color(s)))
                                .unwrap_or(Color::rgba(255,255,255,255));
                            let acrylic = t.acrylic.as_ref().and_then(|a| {
                                let enabled = a.enabled.unwrap_or(d_enabled);
                                if !enabled { return None; }
                                Some(Acrylic {
                                    downscale:   a.downscale.unwrap_or(d_downscale),
                                    tint:        a.tint.as_deref().and_then(|s| hex_to_color(s)).unwrap_or(d_tint),
                                    noise_alpha: a.noise_alpha.unwrap_or(d_noise),
                                })
                            });
                            (color, acrylic)
                        }
                    };
                    paints.insert(name.clone(), Paint { color, acrylic });
                    colors_legacy.insert(name, color);
                }
            }
            Err(e) => warn!("failed to parse colors.toml: {e}"),
        }
    }

    (paints, colors_legacy)
}

impl ThemeManager {
    /// Loads configuration and builds the manager (icons, backgrounds, and initial paints).
    /// Keep the 'icons.' alias block so keys under [icons] can be referenced without the prefix.
    pub fn load_from(path: &str) -> Self {
        // 1) Read + parse nexus.toml
        let txt = fs::read_to_string(path).unwrap_or_default();
        let parsed: NexusToml = toml::from_str(&txt).unwrap_or(NexusToml {
            theme: ThemeSection { current: Some("light".into()) },
            colors: None,
            tables: BTreeMap::new(),
        });

        // 2) Resolve theme id
        let theme = match parsed.theme.current.as_deref() {
            Some("dark") => ThemeId::Dark,
            _            => ThemeId::Light,
        };

        // 3) Collect icon mappings from all tables (icons, system, places, ...)
        //    + robustly handle root-level string keys
        let mut icons: BTreeMap<String, String> = BTreeMap::new();
        for (table_name, value) in parsed.tables.iter() {
            match value {
                toml::Value::Table(t)  => collect_icon_ids(&mut icons, table_name, t),
                toml::Value::String(s) => { icons.insert(table_name.clone(), s.clone()); },
                _ => {}
            }
        }

        // --- Alias ‘icons.’ -> ‘’ (allow keys under [icons] without prefix) ---
        {
            let mut extra = Vec::new();
            for (k, v) in icons.iter() {
                if let Some(stripped) = k.strip_prefix("icons.") {
                    extra.push((stripped.to_string(), v.clone()));
                }
            }
            for (k, v) in extra {
                icons.insert(k, v);
            }
        }
        // --- End alias block ---

        // 4) Backgrounds table (optional)
        let mut backgrounds: BTreeMap<String, String> = BTreeMap::new();
        if let Some(toml::Value::Table(bg)) = parsed.tables.get("backgrounds") {
            for (k, v) in bg {
                if let toml::Value::String(s) = v {
                    backgrounds.insert(k.clone(), s.clone());
                }
            }
        }

        // 5) Colors/Paints for the initial theme
        let (paints, colors_legacy) = load_paints_for(theme);

        // 6) Build manager
        ThemeManager {
            theme: AtomicU8::new(theme.as_u8()),
            icons,
            backgrounds,
            colors_legacy: RwLock::new(colors_legacy),
            paints: RwLock::new(paints),
            cache: Mutex::new(BTreeMap::new()),
            bg_cache: Mutex::new(BTreeMap::new()),
        }
    }

    /// Get a full paint (color + optional acrylic). Falls back to provided `fallback`.
    pub fn paint(&self, name: &str, fallback: Paint) -> Paint {
        if let Some(p) = self.paints.read().ok().and_then(|m| m.get(name).copied()) {
            p
        } else if let Some(color) = self.colors_legacy.read().ok().and_then(|m| m.get(name).copied()) {
            Paint { color, acrylic: fallback.acrylic }
        } else {
            Paint { color: fallback.color, acrylic: fallback.acrylic }
        }
    }

    /// Convenience: just the acrylic part (if present).
    pub fn acrylic(&self, name: &str) -> Option<Acrylic> {
        self.paints
            .read()
            .ok()
            .and_then(|m| m.get(name).copied())
            .and_then(|p| p.acrylic)
    }

    /// Runtime theme switch:
    /// - updates current ThemeId
    /// - reloads paints/colors from /ui/themes/<light|dark>/colors.toml
    /// - clears icon & background caches to force re-render with new palette
    pub fn switch_theme(&self, id: ThemeId) {
        self.set_theme_id(id);

        // Reload palettes
        let (new_paints, new_colors) = load_paints_for(id);
        if let Ok(mut p) = self.paints.write() {
            *p = new_paints;
        } else {
            warn!("ThemeManager: paints RwLock poisoned; keeping previous paints");
        }
        if let Ok(mut c) = self.colors_legacy.write() {
            *c = new_colors;
        } else {
            warn!("ThemeManager: colors_legacy RwLock poisoned; keeping previous colors");
        }

        // Flush caches so subsequent lookups re-render with the new theme
        if let Ok(mut ic) = self.cache.lock() { ic.clear(); }
        if let Ok(mut bg) = self.bg_cache.lock() { bg.clear(); }
    }
}

// Global singleton, same as before:
pub static THEME: Lazy<ThemeManager> = Lazy::new(|| ThemeManager::load_from("/ui/nexus.toml"));
