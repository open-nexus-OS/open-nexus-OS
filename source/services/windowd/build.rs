// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use nexus_theme::{ColorValue, Qualifier, ThemeRuntime};

mod proof_panel_spec {
    #![allow(dead_code)]
    include!("build_support/proof_panel_spec.rs");
}

// The pointer's sprites in both Mocu variants (TASK-0353 P8).
#[path = "build_support/cursors.rs"]
mod cursors;

use proof_panel_spec::ALL_TEXT_SPECS;

const INTER_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";
const THEMES_DIR: &str = "../../../resources/themes";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={INTER_FONT}");
    println!("cargo:rerun-if-changed={THEMES_DIR}");
    println!("cargo:rerun-if-changed=src/proof_panel_spec.rs");

    let out_dir = env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let out_dir = Path::new(&out_dir);
    let font_bytes = fs::read(INTER_FONT)?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|err| std::io::Error::other(format!("parse Inter font: {err:?}")))?;
    // The proof texts are baked in the boot theme (dark). The compositor's runtime colours are
    // not baked here: it reads the typed tokens (`nexus-theme-tokens`, generated from the same
    // theme files) for the active mode — see `src/theme.rs`.
    let mut theme_runtime = ThemeRuntime::load(Path::new(THEMES_DIR))?;
    theme_runtime.set_qualifier(Qualifier::Dark);

    let generated_path = out_dir.join("windowd_generated_assets.rs");
    let mut generated = File::create(generated_path)?;

    for spec in ALL_TEXT_SPECS {
        let rendered = render_text_asset(
            &font,
            spec.content,
            spec.font_size as f32,
            color_bgra(theme_runtime.resolve(spec.color_token)?),
        );
        let const_prefix = const_prefix(spec.id);
        let text_path = out_dir.join(format!("{}.bgra", spec.id));
        fs::write(&text_path, &rendered.data)?;
        writeln!(generated, "pub const {const_prefix}_WIDTH: u32 = {};", rendered.width)?;
        writeln!(generated, "pub const {const_prefix}_HEIGHT: u32 = {};", rendered.height)?;
        writeln!(
            generated,
            "pub const {const_prefix}_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
            text_path.display()
        )?;
    }
    // Runtime glyph atlases (TASK-0070 Phase 6): A8 coverage bitmaps of the
    // vendored variable UI face at the two shell text sizes, plus metrics and
    // sparse kerning, all as consts — `src/text.rs` renders DYNAMIC text from
    // these at runtime (replacing the 5×7 bitmap font). The font family is the
    // manifest-driven default behind the prepared `ui.font.family` settings key;
    // live switching is a follow-up.
    writeln!(generated, "pub const FONT_FAMILY: &str = \"inter\";")?;
    // Glyph atlases: PROMOTED to `nexus-text-baked` (RFC-0067 P5) — windowd
    // consumes the shared SSOT; `font` below only prerenders panel texts.

    cursors::emit(out_dir)?;

    // Real icon (TASK #61 "real icon layer"): render a Lucide icon through the
    // nexus-svg render-at-scale pipeline (currentColor → tint) into a BGRA sprite.
    // gpud composites it as a GPU sprite layer on the virgl scanout — the
    // production "real SVG icon on the GPU compositor" path.
    //
    // The sprite is uploaded INLINE in one IPC frame, so the FINAL size must fit
    // the kernel's 8 KiB MAX_FRAME_BYTES: 44×44×4 + a 25-byte header = 7769 B (~45²
    // is the cap). Bigger uploads need the shared-VMO/atlas path (Shell-P3).
    //
    // Render at 4× (176²) and box-average down to 44² — supersampling for extra
    // AA smoothness at this small size (the wallpaper downscale pattern).
    // Abutting-shape seams are gone at the source: nexus-svg composites all
    // shapes conflation-free in one partitioned sweep. nexus-svg output is
    // premultiplied, so averaging the (already alpha-weighted) channels is the
    // correct, fringe-free downscale. The 176² render is build-time only; just
    // the 44² result is uploaded.
    const SHELL_ICON_LOGICAL: u32 = 44;
    const SHELL_ICON_SS: u32 = 4;
    let icon_svg = include_str!("../../../resources/icons/lucide/icons/house.svg");
    let hi = nexus_svg::render_svg_tinted_at(
        icon_svg,
        (240, 244, 255),
        SHELL_ICON_LOGICAL * SHELL_ICON_SS,
        SHELL_ICON_LOGICAL * SHELL_ICON_SS,
    )
    .map_err(|err| std::io::Error::other(format!("render Lucide icon SVG: {err:?}")))?;
    // Downscale in PREMULTIPLIED space (nexus-svg output) — correct, fringe-free.
    let icon_premul = box_average_downscale(&hi.buffer, hi.width, hi.height, SHELL_ICON_SS);
    // …then UN-premultiply to straight alpha: gpud composites the icon with a
    // straight-alpha src-over blend (rgb·a + dst·(1−a)). A premultiplied sprite
    // would be multiplied by alpha twice → a dark fringe on every AA edge (the
    // "section borders in another colour"). Straight alpha matches the blend.
    let icon_bgra = unpremultiply_bgra(&icon_premul);
    assert!(
        icon_bgra.len() + 25 <= 8 * 1024,
        "shell icon ({}B) + header must fit the 8 KiB IPC frame",
        icon_bgra.len()
    );
    let icon_path = out_dir.join("shell_icon.bgra");
    fs::write(&icon_path, &icon_bgra)?;
    writeln!(generated, "pub const SHELL_ICON_WIDTH: u32 = {};", SHELL_ICON_LOGICAL)?;
    writeln!(generated, "pub const SHELL_ICON_HEIGHT: u32 = {};", SHELL_ICON_LOGICAL)?;
    writeln!(generated, "pub const SHELL_ICON_LOGICAL: u32 = {SHELL_ICON_LOGICAL};")?;
    writeln!(
        generated,
        "pub const SHELL_ICON_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
        icon_path.display()
    )?;

    // Topbar chrome icons: the REAL Lucide `menu` + `x` icons (currentColor → white
    // tint), rendered at 4× supersample then box-downscaled fringe-free — the same
    // proven path as the shell icon above. windowd blends these straight-alpha
    // sprites into the topbar / title-bar surfaces, replacing the hand-drawn
    // approximations. `_DIM` is the on-surface size (must match the placement
    // constants: MENU_ICON_SIZE=26, the title-bar close glyph ≈20).
    for (name, dim, svg) in [
        ("MENU_ICON", 26u32, include_str!("../../../resources/icons/lucide/icons/menu.svg")),
        ("CLOSE_ICON", 20u32, include_str!("../../../resources/icons/lucide/icons/x.svg")),
        // Title-bar window controls (TASK-0070 Phase 2): minimize "–" +
        // maximize "□", sized to read alongside the 20px close "x".
        ("MINIMIZE_ICON", 20u32, include_str!("../../../resources/icons/lucide/icons/minus.svg")),
        ("MAXIMIZE_ICON", 16u32, include_str!("../../../resources/icons/lucide/icons/square.svg")),
        // Greeter avatar glyph (TASK-0065B): a user inside a circle, blended
        // into the login window's avatar disc.
        (
            "GREETER_AVATAR_ICON",
            64u32,
            include_str!("../../../resources/icons/lucide/icons/circle-user.svg"),
        ),
    ] {
        const SS: u32 = 4;
        let hi = nexus_svg::render_svg_tinted_at(svg, (255, 255, 255), dim * SS, dim * SS)
            .map_err(|err| std::io::Error::other(format!("render Lucide {name}: {err:?}")))?;
        let premul = box_average_downscale(&hi.buffer, hi.width, hi.height, SS);
        let bgra = unpremultiply_bgra(&premul);
        let path = out_dir.join(format!("{}.bgra", name.to_lowercase()));
        fs::write(&path, &bgra)?;
        writeln!(generated, "pub const {name}_DIM: u32 = {dim};")?;
        writeln!(
            generated,
            "pub const {name}_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
            path.display()
        )?;
    }
    Ok(())
}

/// Bake one A8 coverage glyph atlas: ASCII 32..=126 rasterized at `px`, bitmaps
/// concatenated into one blob (`{NAME}_COV`), per-glyph placement/advance in
/// `{NAME}_GLYPHS`, line metrics as consts, and the SPARSE kerning pairs whose
/// rounded value is non-zero at this size (`{NAME}_KERN`). Glyph tuple layout
/// (documented for `src/text.rs`, the sole consumer):
/// `(cov_offset, w, h, left_bearing, top_from_band_top, advance_px)` where
/// `top_from_band_top = ascent − (ymin + height)` places the bitmap in a text
/// band whose baseline sits `ascent` pixels below the band top.
struct RenderedText {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

fn render_text_asset(font: &fontdue::Font, text: &str, px: f32, bgra: [u8; 4]) -> RenderedText {
    let mut pen_x = 0i32;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for ch in text.chars() {
        let (metrics, _) = font.rasterize(ch, px);
        let x0 = pen_x + metrics.xmin;
        let y0 = -(metrics.height as i32) - metrics.ymin;
        if metrics.width != 0 && metrics.height != 0 {
            min_x = min_x.min(x0);
            min_y = min_y.min(y0);
            max_x = max_x.max(x0 + metrics.width as i32);
            max_y = max_y.max(y0 + metrics.height as i32);
        }
        pen_x += metrics.advance_width.ceil() as i32;
    }
    if min_x == i32::MAX || min_y == i32::MAX {
        return RenderedText { width: 1, height: 1, data: vec![0u8; 4] };
    }
    let width = (max_x - min_x).max(1) as usize;
    let height = (max_y - min_y).max(1) as usize;
    let mut out = vec![0u8; width * height * 4];
    pen_x = 0;
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, px);
        let x0 = pen_x + metrics.xmin - min_x;
        let y0 = -(metrics.height as i32) - metrics.ymin - min_y;
        for y in 0..metrics.height {
            let dst_y = y0 + y as i32;
            if dst_y < 0 || dst_y >= height as i32 {
                continue;
            }
            for x in 0..metrics.width {
                let dst_x = x0 + x as i32;
                if dst_x < 0 || dst_x >= width as i32 {
                    continue;
                }
                let alpha = bitmap[y * metrics.width + x];
                if alpha == 0 {
                    continue;
                }
                let dst = (dst_y as usize * width + dst_x as usize) * 4;
                blend(&mut out[dst..dst + 4], [bgra[0], bgra[1], bgra[2], alpha]);
            }
        }
        pen_x += metrics.advance_width.ceil() as i32;
    }
    RenderedText { width, height, data: out }
}

fn blend(dst: &mut [u8], src: [u8; 4]) {
    let alpha = u32::from(src[3]);
    let inv = 255u32.saturating_sub(alpha);
    for channel in 0..3 {
        dst[channel] =
            ((u32::from(src[channel]) * alpha + u32::from(dst[channel]) * inv) / 255) as u8;
    }
    dst[3] = dst[3].max(src[3]);
}

fn color_bgra(color: ColorValue) -> [u8; 4] {
    [color.b, color.g, color.r, color.a]
}

fn const_prefix(id: &str) -> String {
    id.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_uppercase() } else { '_' })
        .collect()
}

/// Box-average downscale a premultiplied BGRA image by an integer `factor`
/// (`sw`/`sh` must be divisible by it). Averaging the already alpha-weighted
/// channels is the correct, fringe-free supersample downscale — extra AA
/// smoothness at small icon sizes (shape-abutment seams are already fixed at
/// the source: nexus-svg composites conflation-free).
fn box_average_downscale(src: &[u8], sw: u32, sh: u32, factor: u32) -> Vec<u8> {
    let dw = sw / factor;
    let dh = sh / factor;
    let n = factor * factor;
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let mut acc = [0u32; 4];
            for sy in 0..factor {
                for sx in 0..factor {
                    let i = (((y * factor + sy) * sw + (x * factor + sx)) * 4) as usize;
                    acc[0] += src[i] as u32;
                    acc[1] += src[i + 1] as u32;
                    acc[2] += src[i + 2] as u32;
                    acc[3] += src[i + 3] as u32;
                }
            }
            let o = ((y * dw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n) as u8;
            }
        }
    }
    out
}

/// Convert a premultiplied BGRA buffer to straight (un-associated) alpha:
/// `straight_rgb = premul_rgb · 255 / a` (rounded, clamped), alpha unchanged.
/// Transparent pixels (a=0) stay zero. Matches gpud's straight-alpha layer blend.
fn unpremultiply_bgra(src: &[u8]) -> Vec<u8> {
    let mut out = src.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            for slot in px.iter_mut().take(3) {
                *slot = (((*slot as u32) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

// The doubled DSL-shell compile path (`dsl_root` → in-process mount) is
// DELETED (Umbau #17 2d): bundlemgrd's payload table is the ONE compile path;
// the shell runs as an app-host and connects a desktop surface.
