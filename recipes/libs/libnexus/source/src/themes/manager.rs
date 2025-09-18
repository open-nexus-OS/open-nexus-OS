// themes/manager.rs — main theme manager: loads nexus.toml, resolves icons/backgrounds/colors, caches rendered assets.
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;
use toml;
use log::{debug, warn, error};
use std::sync::atomic::{AtomicBool};

use orbclient::Color;
use orbimage::Image;

use crate::themes::colors::{
    Acrylic, Paint, ThemeColorsToml, ColorEntry, to_color, hex_to_color
};
use crate::themes::svg_icons::{self, IconVariant, scale_nearest};

static SVG_FEATURE_MISSING_WARNED: AtomicBool = AtomicBool::new(false);

/// Light/Dark selection from `nexus.toml`
#[derive(Copy, Clone, Debug)]
pub enum ThemeId { Light, Dark }

#[derive(Debug, Clone, Default, Deserialize)]
struct ThemeSection {
    #[serde(default)]
    pub current: Option<String>,
}

// nexus.toml we care about: [theme], optional [colors], and any other tables (icons, backgrounds)
#[derive(Debug, Clone, Deserialize)]
struct NexusToml {
    #[serde(default)] theme: ThemeSection,
    #[serde(default)] colors: Option<toml::Value>,
    #[serde(flatten)] tables: HashMap<String, toml::Value>,
}

fn collect_icon_ids(
    out: &mut HashMap<String, String>,
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

/// Main entry point: resolves icons/backgrounds/colors and caches rendered assets.
pub struct ThemeManager {
    theme: ThemeId,
    /// logical name -> relative icon path (e.g. "actions/system-shut-down")
    icons: HashMap<String, String>,
    /// background name -> relative path (e.g. "backgrounds/login")
    backgrounds: HashMap<String, String>,
    /// legacy color map (flat Color, no acrylic), derived from paints
    pub(crate) colors_legacy: HashMap<String, Color>,
    /// new paint map (Color + optional Acrylic)
    paints: HashMap<String, Paint>,
    /// in-process cache for icons
    cache: Mutex<HashMap<(String, IconVariant, Option<(u32,u32)>), Image>>,
    /// in-process cache for backgrounds
    bg_cache: Mutex<HashMap<(String, Option<(u32,u32)>), Image>>,
}

impl ThemeManager {
    fn candidates_for(&self, id: &str, variant: IconVariant) -> Vec<String> {
        let rel: String = if let Some(s) = self.icons.get(id) {
            s.clone()
        } else if let Some(s) = self.icons.get(&format!("icons.{}", id)) {
            s.clone()
        } else {
            id.to_string()
        };
        crate::themes::svg_icons::icon_candidates(&rel, self.theme, variant)
    }

    pub fn load_icon_sized(
        &self,
        id: &str,
        variant: IconVariant,
        size: Option<(u32, u32)>,
    ) -> Option<Image> {
        let key = (id.to_string(), variant, size);

        if let Some(img) = self.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(img);
        }

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
                                &svg_bytes, self.theme, size
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

        // log error
        error!(
            "icon NOT FOUND: id={id} variant={variant:?} size={size:?}\n  tried:\n    - {}",
            candidates.join("\n    - ")
        );
        None
    }

    /// Compatibility wrapper as before: without size argument.
    pub fn load_icon(&self, id: &str, variant: IconVariant) -> Option<Image> {
        self.load_icon_sized(id, variant, None)
    }

    /// Compatibility wrapper for named colors (legacy fallback).
    pub fn color(&self, name: &str, fallback: Color) -> Color {
        self.colors_legacy.get(name).copied().unwrap_or(fallback)
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

        let theme_name = match self.theme {
            ThemeId::Light => "light",
            ThemeId::Dark => "dark",
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

    /// Optional: fast Diagnose, which candidates *exist*.
    pub fn debug_icon_candidates(&self, id: &str, variant: IconVariant) {
        let cands = self.candidates_for(id, variant);
        if cands.is_empty() {
            error!("debug: no candidates for id={id} variant={variant:?}");
            return;
        }
        for p in cands {
            let exists = std::fs::metadata(&p).is_ok();
            let kind = if p.ends_with(".svg") { "svg" } else { "ras" };
        }
    }
}

impl ThemeManager {
    /// Loads configuration and builds the manager (excerpt relevant section).
    /// If you already have the function: just insert the marked alias block.
    pub fn load_from(path: &str) -> Self {
        // 1) Read + parse nexus.toml
        let txt = fs::read_to_string(path).unwrap_or_default();
        let parsed: NexusToml = toml::from_str(&txt).unwrap_or(NexusToml {
            theme: ThemeSection { current: Some("light".into()) },
            colors: None,
            tables: HashMap::new(),
        });

        // 2) Resolve theme id
        let theme = match parsed.theme.current.as_deref() {
            Some("dark") => ThemeId::Dark,
            _            => ThemeId::Light,
        };

        // 3) Collect icon mappings from all tables (icons, system, places, ...)
        //    + auch Root-Level-Stringkeys berücksichtigen (robuster)
        let mut icons: HashMap<String, String> = HashMap::new();
        for (table_name, value) in parsed.tables.iter() {
            match value {
                toml::Value::Table(t)  => collect_icon_ids(&mut icons, table_name, t),
                toml::Value::String(s) => { icons.insert(table_name.clone(), s.clone()); },
                _ => {}
            }
        }

        // --- Alias ‘icons.’ -> ‘’ (keys under [icons] can also be used without prefix) ---
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
        // --- End Alias-Block ---

        // 4) Backgrounds table (optional)
        let mut backgrounds: HashMap<String, String> = HashMap::new();
        if let Some(toml::Value::Table(bg)) = parsed.tables.get("backgrounds") {
            for (k, v) in bg {
                if let toml::Value::String(s) = v {
                    backgrounds.insert(k.clone(), s.clone());
                }
            }
        }

        // 5) Colors: try to load paints (color + optional acrylic) from colors.toml
        let mut paints: HashMap<String, Paint> = HashMap::new();
        let mut colors_legacy: HashMap<String, Color> = HashMap::new();
        let theme_name = match theme { ThemeId::Light => "light", ThemeId::Dark => "dark" };
        let colors_path = format!("/ui/themes/{}/colors.toml", theme_name);
        if let Ok(txt) = fs::read_to_string(&colors_path) {
            match toml::from_str::<ThemeColorsToml>(&txt) {
                Ok(doc) => {
                    // Defaults for acrylic if not specified per key
                    let def = doc.defaults.acrylic.unwrap_or_default();
                    let d_enabled    = def.enabled.unwrap_or(false);
                    let d_downscale  = def.downscale.unwrap_or(4);
                    let d_tint       = def.tint
                        .as_deref()
                        .and_then(|s| hex_to_color(s))
                        .unwrap_or(Color::rgba(255,255,255,0));
                    let d_noise      = def.noise_alpha.unwrap_or(0);

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
                Err(e) => {
                    warn!("failed to parse colors.toml: {e}");
                }
            }
        }

        // 6) Build manager
        let manager = ThemeManager {
            theme,
            icons,
            backgrounds,
            colors_legacy,
            paints,
            cache: Mutex::new(HashMap::new()),
            bg_cache: Mutex::new(HashMap::new()),
        };


        manager
    }
}

impl ThemeManager {
    /// Get a full paint (color + optional acrylic). Falls back to provided `fallback`.
    pub fn paint(&self, name: &str, fallback: Paint) -> Paint {
        if let Some(p) = self.paints.get(name) {
            *p
        } else {
            if let Some(color) = self.colors_legacy.get(name) {
                Paint { color: *color, acrylic: fallback.acrylic }
            } else {
                Paint { color: fallback.color, acrylic: fallback.acrylic }
            }
        }
    }

    /// Convenience: just the acrylic part (if present).
    pub fn acrylic(&self, name: &str) -> Option<Acrylic> {
        self.paints.get(name).and_then(|p| p.acrylic)
    }
}

// Global singleton, same as before:
pub static THEME: Lazy<ThemeManager> = Lazy::new(|| ThemeManager::load_from("/ui/nexus.toml"));
