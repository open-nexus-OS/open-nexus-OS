// themes/svg_icons.rs --- SVG icon loading, caching, and rendering.
use once_cell::sync::Lazy;
use orbimage::Image;
use std::collections::HashMap;
use std::sync::Mutex;
use orbclient::{Color, Renderer};

use super::manager::ThemeId;

/// Which icon flavor to resolve (and cache separately).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconVariant { Auto, Light, Dark, Symbolic }

/// Optional shared cache for icons (if you want a global cache in addition to ThemeManager’s).
pub static ICON_CACHE: Lazy<Mutex<HashMap<(String, IconVariant, Option<(u32,u32)>), Image>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

    // Contain-fit into target size:
    if let Some((tw, th)) = target {
        if w == 0 || h == 0 { w = tw.max(1); h = th.max(1); }
        else {
            let s = (tw as f32 / w as f32).min(th as f32 / h as f32).max(0.0);
            w = (w as f32 * s).round().max(1.0) as u32;
            h = (h as f32 * s).round().max(1.0) as u32;
        }
    } else if w == 0 || h == 0 {
        w = 24; h = 24; // reasonable default if the SVG has no explicit size
    }

    // 1px pad to avoid clipping strokes at the edges
    let pad = 1;
    let bw = w + pad*2;
    let bh = h + pad*2;
    let mut pm = Pixmap::new(bw, bh)?;
    let mut pmut = pm.as_mut();
    resvg::render(&tree, Transform::from_translate(pad as f32, pad as f32), &mut pmut);

    // Crop back to requested size and convert to orbimage::Image
    let src = pm.data();
    let mut out = Vec::with_capacity((w*h) as usize);
    for row in 0..h {
        let start = ((row + pad) * bw * 4 + pad * 4) as usize;
        let end   = start + (w * 4) as usize;
        for px in src[start..end].chunks_exact(4) {
            out.push(Color::rgba(px[0], px[1], px[2], px[3]));
        }
    }
    Image::from_data(w, h, out.into()).ok()
}
