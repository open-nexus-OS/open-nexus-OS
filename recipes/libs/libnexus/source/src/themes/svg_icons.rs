// themes/svg_icons.rs --- SVG icon loading, caching, and rendering.
use once_cell::sync::Lazy;
use orbimage::Image;
use std::collections::BTreeMap;
use std::sync::Mutex;
use orbclient::{Color, Renderer};

use super::manager::ThemeId;

/// Which icon flavor to resolve (and cache separately).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IconVariant { Auto, Light, Dark, Symbolic }

/// Optional shared cache for icons (if you want a global cache in addition to ThemeManager's).
pub static ICON_CACHE: Lazy<Mutex<BTreeMap<(String, IconVariant, Option<(u32,u32)>), Image>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Cheap nearest-neighbor scaler for raster images (PNG/JPG).
pub fn scale_nearest(src: &Image, tw: u32, th: u32) -> Image {
    use orbclient::Color;
    if tw == 0 || th == 0 { return Image::from_data(0, 0, Vec::<Color>::new().into()).unwrap(); }
    let sw = src.width();
    let sh = src.height();
    if sw == 0 || sh == 0 { return Image::from_data(0, 0, Vec::<Color>::new().into()).unwrap(); }

    let buf: Vec<Color> = src.clone().data().to_vec();
    let mut out = Vec::with_capacity((tw*th) as usize);
    for y in 0..th {
        let sy = (y as u64 * sh as u64 / th as u64) as u32;
        for x in 0..tw {
            let sx = (x as u64 * sw as u64 / tw as u64) as u32;
            out.push(buf[(sy * sw + sx) as usize]);
        }
    }
    Image::from_data(tw, th, out.into()).unwrap()
}

/// Build candidate file paths for a logical icon id + variant.
pub fn icon_candidates(rel: &str, theme: ThemeId, variant: IconVariant) -> Vec<String> {
    let theme_name = match theme { ThemeId::Light => "light", ThemeId::Dark => "dark" };
    let mut v = Vec::new();

    match variant {
        IconVariant::Symbolic => {
            v.push(format!("/ui/themes/{}/icons/{}.symbolic.svg", theme_name, rel));
            v.push(format!("/ui/themes/{}/icons/{}.symbolic.png", theme_name, rel));
        }
        IconVariant::Light => {
            v.push(format!("/ui/themes/{}/icons/{}.light.svg", theme_name, rel));
            v.push(format!("/ui/themes/{}/icons/{}.light.png", theme_name, rel));
        }
        IconVariant::Dark => {
            v.push(format!("/ui/themes/{}/icons/{}.dark.svg", theme_name, rel));
            v.push(format!("/ui/themes/{}/icons/{}.dark.png", theme_name, rel));
        }
        IconVariant::Auto => {
            // theme-default
            v.push(format!("/ui/themes/{}/icons/{}.svg", theme_name, rel));
            v.push(format!("/ui/themes/{}/icons/{}.png", theme_name, rel));
            // theme-suffixed
            let suffix = match theme { ThemeId::Light => "light", ThemeId::Dark => "dark" };
            v.push(format!("/ui/themes/{}/icons/{}.{}.svg", theme_name, rel, suffix));
            v.push(format!("/ui/themes/{}/icons/{}.{}.png", theme_name, rel, suffix));
            // symbolic fallback
            v.push(format!("/ui/themes/{}/icons/{}.symbolic.svg", theme_name, rel));
            v.push(format!("/ui/themes/{}/icons/{}.symbolic.png", theme_name, rel));
        }
    }

    // Same candidates under the "other" theme as fallback.
    let other = match theme { ThemeId::Light => "dark", ThemeId::Dark => "light" };
    let mut alt = v.iter().map(|p|
        p.replace(&format!("/themes/{}/", theme_name), &format!("/themes/{}/", other))
    ).collect::<Vec<_>>();
    v.append(&mut alt);

    // Global non-themed fallback:
    v.push(format!("/ui/icons/{}.svg", rel));
    v.push(format!("/ui/icons/{}.png", rel));
    // App icons fallback:
    v.push(format!("/ui/icons/apps/{}.svg", rel));
    v.push(format!("/ui/icons/apps/{}.png", rel));

    // Debug: List what's in /ui/icons/apps/
    println!("🔍 DEBUG: Looking for icon '{}' in /ui/icons/apps/", rel);
    if let Ok(entries) = std::fs::read_dir("/ui/icons/apps/") {
        println!("📁 DEBUG: Contents of /ui/icons/apps/:");
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                println!("  - {}", name);
            }
        }
    } else {
        println!("❌ DEBUG: Cannot read /ui/icons/apps/ directory");
    }
    v
}

#[cfg(feature = "svg")]
pub fn build_fontdb_for_theme(theme: ThemeId) -> resvg::usvg::fontdb::Database {
    use resvg::usvg::fontdb::Database;
    let mut db = Database::new();
    // System + common directories:
    db.load_system_fonts();
    db.load_fonts_dir("/ui/font");
    db.load_fonts_dir("/ui/fonts");
    // Theme override (highest priority):
    let theme_name = match theme { ThemeId::Light => "light", ThemeId::Dark => "dark" };
    db.load_fonts_dir(&format!("/ui/themes/{}/fonts", theme_name));
    db
}

#[cfg(feature = "svg")]
pub fn render_svg_to_image_with_theme(
    svg_data: &[u8],
    theme: ThemeId,
    target: Option<(u32, u32)>,
) -> Option<Image> {
    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::{Options, Tree};

    // Parsing options with themed fonts:
    let mut opt = Options::default();
    *opt.fontdb_mut() = build_fontdb_for_theme(theme);

    let tree = Tree::from_data(svg_data, &opt).ok()?;
    let isize = tree.size().to_int_size();
    let (mut w, mut h) = (isize.width(), isize.height());

    println!("🔍 SVG original size: {}x{}", isize.width(), isize.height());

    // Scale SVG only to target height, let width adjust automatically
    if let Some((_tw, th)) = target {
        // Use target height, calculate width based on aspect ratio
        let scale = if isize.height() > 0 { th as f32 / isize.height() as f32 } else { 1.0 };
        w = (isize.width() as f32 * scale) as u32;
        h = th;
        println!("🎯 Target height: {}px, calculated width: {}px (scale: {:.2})", th, w, scale);
    } else if w == 0 || h == 0 {
        w = 24; h = 24; // reasonable default if the SVG has no explicit size
    }

    println!("📏 Final size: {}x{}", w, h);

    // Create pixmap with calculated size (height-based)
    let mut pm = Pixmap::new(w, h)?;
    let mut pmut = pm.as_mut();

    // Calculate scale based on height only
    let scale = if isize.height() > 0 { h as f32 / isize.height() as f32 } else { 1.0 };

    println!("🔧 Transform: scale={:.2} (height-based only)", scale);

    // Create transform that scales the SVG based on height only
    let transform = Transform::from_scale(scale, scale);

    // Render the SVG with the transform
    resvg::render(&tree, transform, &mut pmut);

    // Verify the final pixmap size
    println!("🔍 Final pixmap size after render: {}x{}", pm.width(), pm.height());

    println!("🎨 Rendered to: {}x{}", pm.width(), pm.height());

    // Convert to orbimage::Image
    let src = pm.data();
    let mut out = Vec::with_capacity((w*h) as usize);
    for px in src.chunks_exact(4) {
        out.push(Color::rgba(px[0], px[1], px[2], px[3]));
    }
    Image::from_data(w, h, out.into()).ok()
}
