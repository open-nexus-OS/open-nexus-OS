// libnexus

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use orbclient::{Color, Renderer};
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

// ===== Optional public cache (API beibehalten + Size-Variante) =====

#[allow(dead_code)]
pub struct IconCache {
    // key: (icon_id, variant, size)
    cache: Mutex<HashMap<(String, IconVariant, Option<(u32, u32)>), Image>>,
}

#[allow(dead_code)]
impl IconCache {
    /// Use old Signature for compatibility
    pub fn get(&self, id: &str, variant: IconVariant) -> Option<Image> {
        self.get_sized(id, variant, None)
    }
    pub fn put(&self, id: &str, variant: IconVariant, img: Image) {
        self.put_sized(id, variant, None, img)
    }

    /// Size is (width, height) in pixels, or None for original size.
    pub fn get_sized(
        &self,
        id: &str,
        variant: IconVariant,
        size: Option<(u32, u32)>,
    ) -> Option<Image> {
        if let Ok(map) = self.cache.lock() {
            if let Some(img) = map.get(&(id.to_string(), variant, size)) {
                return Some(img.clone());
            }
        }
        None
    }

    pub fn put_sized(
        &self,
        id: &str,
        variant: IconVariant,
        size: Option<(u32, u32)>,
        img: Image,
    ) {
        if let Ok(mut map) = self.cache.lock() {
            map.insert((id.to_string(), variant, size), img);
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

    // Collect all tables generically (icons, backgrounds, etc.)
    #[serde(flatten)]
    tables: HashMap<String, toml::Value>,
}

pub struct ThemeManager {
    theme: ThemeId,
    /// logical id -> relative icon path (e.g. "actions/system-shut-down")
    icons: HashMap<String, String>,
    /// background name -> relative path (e.g. "backgrounds/login")
    backgrounds: HashMap<String, String>,
    /// named colors
    colors: HashMap<String, Color>,
    /// small in-process cache so we don't reload icons repeatedly
    /// key = (icon_id, variant, size)
    cache: Mutex<HashMap<(String, IconVariant, Option<(u32, u32)>), Image>>,
    /// background cache: key = (name, size)
    bg_cache: Mutex<HashMap<(String, Option<(u32, u32)>), Image>>,
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

        // Flatten all nested icon-like keys into "prefix.sub.key" -> "relative/path"
        let mut icons = HashMap::new();
        for (table_name, value) in parsed.tables.iter() {
            if let toml::Value::Table(t) = value {
                Self::collect_icon_ids(&mut icons, table_name, t);
            }
        }

        // Alias: keys starting with "icons." are also available without prefix
        let mut extra = Vec::new();
        for (k, v) in icons.iter() {
            if let Some(stripped) = k.strip_prefix("icons.") {
                extra.push((stripped.to_string(), v.clone()));
            }
        }
        for (k, v) in extra {
            icons.insert(k, v);
        }

        // background mappings
        let mut backgrounds = HashMap::new();
        if let Some(toml::Value::Table(bg)) = parsed.tables.get("backgrounds") {
            for (k, v) in bg {
                if let toml::Value::String(s) = v {
                    backgrounds.insert(k.clone(), s.clone());
                }
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
            backgrounds,
            colors,
            cache: Mutex::new(HashMap::new()),
            bg_cache: Mutex::new(HashMap::new()),
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
        // Important: dots in IDs are *logical names*. Pathes come from mapping.
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
                // Symbolic as last Fallback
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

        // Fallback for other theme (e.g. if icon only exists in dark mode)
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

        // Global, theme-agnostic
        v.push(format!("/ui/icons/{}.svg", rel));
        v.push(format!("/ui/icons/{}.png", rel));

        v
    }

    /// Wrapper for no-size variant
    pub fn load_icon(&self, id: &str, variant: IconVariant) -> Option<Image> {
        self.load_icon_sized(id, variant, None)
    }

    /// SVG-first, PNG fallback. Optional scaling to (width, height).
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

        for path in self.candidates_for(id, variant) {
            if path.ends_with(".svg") {
                #[cfg(feature = "svg")]
                if let Ok(svg_bytes) = fs::read(&path) {
                    if let Some(img) = render_svg_to_image_with_theme(&svg_bytes, self.theme, size) {
                        let _ = self.cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
                        return Some(img);
                    }
                }
            } else if let Ok(mut img) = Image::from_path(&path) {
                if let Some((w, h)) = size {
                    img = scale_nearest(&img, w, h);
                }
                let _ = self.cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
                return Some(img);
            }
        }
        None
    }

    /// load background: only JPG/JPEG, first theme-spezifisch, then global.
    pub fn load_background(&self, name: &str, size: Option<(u32, u32)>) -> Option<Image> {
        let key = (name.to_string(), size);
        if let Some(img) = self.bg_cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return Some(img);
        }

        // Relative Path from nexus.toml, eg "backgrounds/login"
        let rel = self
            .backgrounds
            .get(name)
            .cloned()
            .unwrap_or_else(|| format!("backgrounds/{}", name));

        let theme_name = match self.theme {
            ThemeId::Light => "light",
            ThemeId::Dark => "dark",
        };

        // Priority: themes → global; nur .jpg/.jpeg
        let candidates = [
            format!("/ui/themes/{}/{}.jpg", theme_name, rel),
            format!("/ui/themes/{}/{}.jpeg", theme_name, rel),
            format!("/ui/{}.jpg", rel),
            format!("/ui/{}.jpeg", rel),
        ];

        for p in candidates {
            if let Ok(mut img) = Image::from_path(&p) {
                if let Some((w, h)) = size {
                    img = scale_nearest(&img, w, h);
                }
                let _ = self.bg_cache.lock().map(|mut c| c.insert(key.clone(), img.clone()));
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
fn build_fontdb_for_theme(theme: ThemeId) -> resvg::usvg::fontdb::Database {
    use resvg::usvg::fontdb::Database;

    let mut db = Database::new();

    // Low Priority: System-Fonts
    db.load_system_fonts();

    // Fallback-directories
    db.load_fonts_dir("/ui/font");
    db.load_fonts_dir("/ui/fonts");

    // Theme-Override (highest priority)
    let theme_name = match theme {
        ThemeId::Light => "light",
        ThemeId::Dark => "dark",
    };
    let themed = format!("/ui/themes/{}/fonts", theme_name);
    db.load_fonts_dir(&themed);

    db
}

#[cfg(feature = "svg")]
fn render_svg_to_image_with_theme(
    svg_data: &[u8],
    theme: ThemeId,
    target: Option<(u32, u32)>,
) -> Option<Image> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    // Parsing-Options + themed Fonts
    let mut opt = Options::default();
    *opt.fontdb_mut() = build_fontdb_for_theme(theme);

    // Parse
    let tree = Tree::from_data(svg_data, &opt).ok()?;

    // Init size from SVG (als IntSize)
    let isize = tree.size().to_int_size();
    let mut w = isize.width();
    let mut h = isize.height();

    // sizes (contain-fit), without usvg::ScreenSize dependency
    if let Some((tw, th)) = target {
        if w == 0 || h == 0 {
            w = tw.max(1);
            h = th.max(1);
        } else {
            let scale = (tw as f32 / w as f32).min(th as f32 / h as f32).max(0.0);
            w = (w as f32 * scale).round().max(1.0) as u32;
            h = (h as f32 * scale).round().max(1.0) as u32;
        }
    } else if w == 0 || h == 0 {
        // Default-iconsize,if SVG has no size
        w = 24;
        h = 24;
    }

    // Render with 1px overscan padding to avoid stroke clipping at edges.
    let pad: u32 = 1;
    let bw = w + pad * 2;
    let bh = h + pad * 2;
    let mut pixmap = Pixmap::new(bw, bh)?;
    let mut pm = pixmap.as_mut();
    // Translate by +pad so strokes that bleed over the viewBox are not clipped.
    resvg::render(&tree, Transform::from_translate(pad as f32, pad as f32), &mut pm);

    // Crop center ROI back to requested w×h and convert to orbimage::Image.
    let src = pixmap.data(); // &[u8] RGBA, stride = bw * 4
    let stride = (bw * 4) as usize;
    let mut out = Vec::with_capacity((w * h) as usize);
    for row in 0..h {
        let start = ((row + pad) * bw * 4 + pad * 4) as usize;
        let end = start + (w * 4) as usize;
        for px in src[start..end].chunks_exact(4) {
            out.push(Color::rgba(px[0], px[1], px[2], px[3]));
        }
    }
    Image::from_data(w, h, out.into()).ok()
}

/// simple Nearest-Scale for Raster-Fallback (PNG/JPG)
fn scale_nearest(src: &Image, tw: u32, th: u32) -> Image {
     if tw == 0 || th == 0 {
         return Image::from_data(0, 0, Vec::<Color>::new().into()).unwrap();
     }
     let sw = src.width();
     let sh = src.height();
     if sw == 0 || sh == 0 {
         return Image::from_data(0, 0, Vec::<Color>::new().into()).unwrap();
     }

    let src_clone = src.clone();
    let src_buf: Vec<Color> = src_clone.data().to_vec();
 
    let mut out = Vec::with_capacity((tw * th) as usize);
    for y in 0..th {
        let sy = (y as u64 * sh as u64 / th as u64) as u32;
        for x in 0..tw {
            let sx = (x as u64 * sw as u64 / tw as u64) as u32;
            let idx = (sy * sw + sx) as usize;
            out.push(src_buf[idx]);
        }
    }
    Image::from_data(tw, th, out.into()).unwrap()
}

// Global manager loaded from /ui/nexus.toml once.
pub static THEME: Lazy<ThemeManager> = Lazy::new(|| ThemeManager::load_from("/ui/nexus.toml"));

pub static ICON_CACHE: Lazy<IconCache> = Lazy::new(|| IconCache {
    cache: Mutex::new(HashMap::new()),
});
// ===== End libnexus =====