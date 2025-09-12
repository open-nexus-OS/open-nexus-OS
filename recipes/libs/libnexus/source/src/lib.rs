// libnexus

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use orbclient::Color;
use orbimage::Image;

// ===== Theme types =====

#[derive(Copy, Clone, Debug)]
pub enum ThemeId {
    Light,
    Dark,
}

/// Which icon flavor to resolve (and cache under a distinct key).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconVariant {
    Auto,
    Light,
    Dark,
    Symbolic,
}

#[allow(dead_code)]
pub struct IconCache {
    // key: (icon_id, variant)
    cache: Mutex<HashMap<(String, IconVariant), Image>>,
}

#[allow(dead_code)]
impl IconCache {
    pub fn get(&self, id: &str, variant: IconVariant) -> Option<Image> {
        if let Ok(map) = self.cache.lock() {
            if let Some(img) = map.get(&(id.to_string(), variant)) {
                return Some(img.clone());
            }
        }
        None
    }

    pub fn put(&self, id: &str, variant: IconVariant, img: Image) {
        if let Ok(mut map) = self.cache.lock() {
            map.insert((id.to_string(), variant), img);
        }
    }
}

// ===== nexus.toml model =====

#[derive(Debug, Clone, Default, Deserialize)]
struct ThemeSection {
    #[serde(default)]
    current: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ColorsTable {
    // Optional named colors (RGBA 0-255). Example in nexus.toml:
    // [colors]
    // bar_bg = [255, 255, 255, 191]
    // text   = [231, 231, 231, 255]
    #[serde(flatten)]
    items: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Deserialize)]
struct NexusToml {
    #[serde(default)]
    theme: ThemeSection,

    #[serde(default)]
    colors: Option<ColorsTable>,

    // Collect all icon-like tables generically
    #[serde(flatten)]
    tables: HashMap<String, toml::Value>,
}

pub struct ThemeManager {
    theme: ThemeId,
    /// logical id -> relative icon path (e.g. "actions/system-shut-down")
    icons: HashMap<String, String>,
    /// named colors
    colors: HashMap<String, Color>,
    /// small in-process cache so we don't reload icons repeatedly
    cache: Mutex<HashMap<(String, IconVariant), Image>>,
}

impl ThemeManager {
    pub fn load_from(path: &str) -> Self {
        let txt = fs::read_to_string(path).unwrap_or_default();
        let parsed: NexusToml = toml::from_str(&txt).unwrap_or(NexusToml {
            theme: ThemeSection {
                current: Some("light".into()),
            },
            colors: None,
            tables: HashMap::new(),
        });

        let theme = match parsed.theme.current.as_deref() {
            Some("dark") => ThemeId::Dark,
            _ => ThemeId::Light,
        };

        // Flatten all nested icon keys into "prefix.sub.key" -> "relative/path"
        let mut icons = HashMap::new();
        for (table_name, value) in parsed.tables.iter() {
            if let toml::Value::Table(t) = value {
                Self::collect_icon_ids(&mut icons, table_name, t);
            }
        }

        // Optional named colors
        let mut colors = HashMap::new();
        if let Some(ct) = parsed.colors {
            for (k, v) in ct.items {
                let c = to_color(&v);
                colors.insert(k, c);
            }
        }

        ThemeManager {
            theme,
            icons,
            colors,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn collect_icon_ids(
        out: &mut HashMap<String, String>,
        prefix: &str,
        table: &toml::map::Map<String, toml::Value>,
    ) {
        for (k, v) in table {
            let full_key = format!("{}.{}", prefix, k);
            match v {
                toml::Value::String(s) => {
                    out.insert(full_key, s.clone());
                }
                toml::Value::Table(t) => {
                    Self::collect_icon_ids(out, &full_key, t);
                }
                _ => {}
            }
        }
    }

    pub fn theme(&self) -> ThemeId {
        self.theme
    }

    pub fn color(&self, name: &str, fallback: Color) -> Color {
        self.colors.get(name).copied().unwrap_or(fallback)
    }

    /// Build an ordered list of file candidates for the given logical icon id + variant.
    fn candidates_for(&self, id: &str, variant: IconVariant) -> Vec<String> {
        let rel = self.icons.get(id).map(|s| s.as_str()).unwrap_or(id);

        let theme_name = match self.theme {
            ThemeId::Light => "light",
            ThemeId::Dark => "dark",
        };

        let mut v = Vec::new();

        match variant {
            IconVariant::Symbolic => {
                v.push(format!(
                    "/ui/themes/{}/icons/{}.symbolic.svg",
                    theme_name, rel
                ));
                v.push(format!(
                    "/ui/themes/{}/icons/{}.symbolic.png",
                    theme_name, rel
                ));
            }
            IconVariant::Light => {
                v.push(format!(
                    "/ui/themes/{}/icons/{}.light.svg",
                    theme_name, rel
                ));
                v.push(format!(
                    "/ui/themes/{}/icons/{}.light.png",
                    theme_name, rel
                ));
            }
            IconVariant::Dark => {
                v.push(format!(
                    "/ui/themes/{}/icons/{}.dark.svg",
                    theme_name, rel
                ));
                v.push(format!(
                    "/ui/themes/{}/icons/{}.dark.png",
                    theme_name, rel
                ));
            }
            IconVariant::Auto => {
                // Theme-generic
                v.push(format!("/ui/themes/{}/icons/{}.svg", theme_name, rel));
                v.push(format!("/ui/themes/{}/icons/{}.png", theme_name, rel));
                // Theme-suffixed
                let suffix = match self.theme {
                    ThemeId::Light => "light",
                    ThemeId::Dark => "dark",
                };
                v.push(format!(
                    "/ui/themes/{}/icons/{}.{}.svg",
                    theme_name, rel, suffix
                ));
                v.push(format!(
                    "/ui/themes/{}/icons/{}.{}.png",
                    theme_name, rel, suffix
                ));
                // Symbolic as fallback
                v.push(format!(
                    "/ui/themes/{}/icons/{}.symbolic.svg",
                    theme_name, rel
                ));
                v.push(format!(
                    "/ui/themes/{}/icons/{}.symbolic.png",
                    theme_name, rel
                ));
            }
        }

        // Fallback to the other theme variant
        let other = match self.theme {
            ThemeId::Light => "dark",
            ThemeId::Dark => "light",
        };
        let mut alt = v
            .iter()
            .map(|p| {
                p.replace(
                    &format!("/themes/{}/", theme_name),
                    &format!("/themes/{}/", other),
                )
            })
            .collect::<Vec<_>>();
        v.append(&mut alt);

        // Global, theme-agnostic fallback
        v.push(format!("/ui/icons/{}.svg", rel));
        v.push(format!("/ui/icons/{}.png", rel));

        v
    }

    /// Resolve and load the icon image (SVG first, then PNG), caching the result.
    pub fn load_icon(&self, id: &str, variant: IconVariant) -> Option<Image> {
        // Check small cache first
        if let Some(img) = self
            .cache
            .lock()
            .ok()
            .and_then(|c| c.get(&(id.to_string(), variant)).cloned())
        {
            return Some(img);
        }

        // Try SVG first, then PNG across candidates
        for path in self.candidates_for(id, variant) {
            if path.ends_with(".svg") {
                #[cfg(feature = "svg")]
                if let Ok(svg_bytes) = fs::read(&path) {
                    if let Some(img) = render_svg_to_image(&svg_bytes) {
                        let _ = self
                            .cache
                            .lock()
                            .map(|mut c| c.insert((id.to_string(), variant), img.clone()));
                        return Some(img);
                    }
                }
            } else if let Ok(img) = Image::from_path(&path) {
                let _ = self
                    .cache
                    .lock()
                    .map(|mut c| c.insert((id.to_string(), variant), img.clone()));
                return Some(img);
            }
        }
        None
    }
}

// Convert [r,g,b,a] (0..=255) to orbclient::Color
fn to_color(v: &[u8]) -> Color {
    match v {
        [r, g, b, a] => Color::rgba(*r, *g, *b, *a),
        [r, g, b] => Color::rgba(*r, *g, *b, 255),
        _ => Color::rgba(255, 255, 255, 255),
    }
}

#[cfg(feature = "svg")]
fn render_svg_to_image(svg_data: &[u8]) -> Option<Image> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    // Build parsing options and load system fonts (for text in SVGs)
    let mut opt = Options::default();
    opt.fontdb_mut().load_system_fonts();

    // Parse + render
    let tree = Tree::from_data(svg_data, &opt).ok()?;
    let size = tree.size().to_int_size();
    let mut pixmap = Pixmap::new(size.width(), size.height())?;

    // IMPORTANT: take a PixmapMut first, then pass &mut to render()
    let mut pm = pixmap.as_mut();
    resvg::render(&tree, Transform::default(), &mut pm);

    // Convert RGBA bytes -> orbimage::Image
    let width = pixmap.width();
    let height = pixmap.height();
    let raw = pixmap.take(); // Vec<u8> RGBA
    let mut buf = Vec::with_capacity((width * height) as usize);
    for rgba in raw.chunks_exact(4) {
        buf.push(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]));
    }
    Image::from_data(width, height, buf.into()).ok()
}

// Global manager loaded from /ui/nexus.toml once.
pub static THEME: Lazy<ThemeManager> = Lazy::new(|| ThemeManager::load_from("/ui/nexus.toml"));
pub static ICON_CACHE: Lazy<IconCache> = Lazy::new(|| IconCache {
    cache: Mutex::new(HashMap::new()),
});
