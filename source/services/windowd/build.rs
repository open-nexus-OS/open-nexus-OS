// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use nexus_theme::{ColorValue, Qualifier, ThemeRuntime};

mod proof_panel_spec {
    #![allow(dead_code)]
    include!("src/proof_panel_spec.rs");
}

use proof_panel_spec::{
    ALL_TEXT_SPECS, TOKEN_CARD_ACTIVE_BG, TOKEN_CARD_BG, TOKEN_CARD_BORDER, TOKEN_CARD_LABEL,
    TOKEN_CLICK, TOKEN_GLASS_EDGE, TOKEN_GLASS_TINT, TOKEN_HOVER, TOKEN_ICON_BG, TOKEN_ICON_FG,
    TOKEN_KEYBOARD, TOKEN_PANEL_BG, TOKEN_PANEL_BORDER, TOKEN_PANEL_MUTED, TOKEN_PANEL_SUBTITLE,
    TOKEN_PANEL_TITLE, TOKEN_SCROLL,
};

const INTER_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";
const MOCU_DEFAULT: &str = "../../../resources/cursors/mocu/src/svg/default.svg";
const THEMES_DIR: &str = "../../../resources/themes";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={INTER_FONT}");
    println!("cargo:rerun-if-changed={MOCU_DEFAULT}");
    println!("cargo:rerun-if-changed={THEMES_DIR}");
    println!("cargo:rerun-if-changed=src/proof_panel_spec.rs");
    println!("cargo:rerun-if-changed={DSL_DEMO_NX}");

    let out_dir = env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let out_dir = Path::new(&out_dir);
    compile_dsl_demo(out_dir)?;
    let font_bytes = fs::read(INTER_FONT)?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|err| std::io::Error::other(format!("parse Inter font: {err:?}")))?;
    let mut theme_runtime = ThemeRuntime::load(Path::new(THEMES_DIR))?;
    theme_runtime.set_qualifier(Qualifier::Dark);

    let mocu = fs::read_to_string(MOCU_DEFAULT)?;
    if !(mocu.contains("#fafbfc") && mocu.contains("#1a1b1c") && mocu.contains("id=\"hot\"")) {
        return Err("Mocu default cursor source shape changed".into());
    }

    let generated_path = out_dir.join("windowd_generated_assets.rs");
    let mut generated = File::create(generated_path)?;
    emit_theme_color(&mut generated, "PROOF_PANEL_BG_RGBA", &theme_runtime, TOKEN_PANEL_BG)?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_BORDER_RGBA",
        &theme_runtime,
        TOKEN_PANEL_BORDER,
    )?;
    emit_theme_color(&mut generated, "PROOF_PANEL_TITLE_RGBA", &theme_runtime, TOKEN_PANEL_TITLE)?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_SUBTITLE_RGBA",
        &theme_runtime,
        TOKEN_PANEL_SUBTITLE,
    )?;
    emit_theme_color(&mut generated, "PROOF_PANEL_MUTED_RGBA", &theme_runtime, TOKEN_PANEL_MUTED)?;
    emit_theme_color(&mut generated, "PROOF_CARD_BG_RGBA", &theme_runtime, TOKEN_CARD_BG)?;
    emit_theme_color(
        &mut generated,
        "PROOF_CARD_ACTIVE_BG_RGBA",
        &theme_runtime,
        TOKEN_CARD_ACTIVE_BG,
    )?;
    emit_theme_color(&mut generated, "PROOF_CARD_BORDER_RGBA", &theme_runtime, TOKEN_CARD_BORDER)?;
    emit_theme_color(&mut generated, "PROOF_CARD_LABEL_RGBA", &theme_runtime, TOKEN_CARD_LABEL)?;
    emit_theme_color(&mut generated, "PROOF_ICON_BG_RGBA", &theme_runtime, TOKEN_ICON_BG)?;
    emit_theme_color(&mut generated, "PROOF_ICON_FG_RGBA", &theme_runtime, TOKEN_ICON_FG)?;
    emit_theme_color(&mut generated, "PROOF_HOVER_RGBA", &theme_runtime, TOKEN_HOVER)?;
    emit_theme_color(&mut generated, "PROOF_CLICK_RGBA", &theme_runtime, TOKEN_CLICK)?;
    emit_theme_color(&mut generated, "PROOF_SCROLL_RGBA", &theme_runtime, TOKEN_SCROLL)?;
    emit_theme_color(&mut generated, "PROOF_KEYBOARD_RGBA", &theme_runtime, TOKEN_KEYBOARD)?;
    emit_theme_color(&mut generated, "GLASS_TINT_RGBA", &theme_runtime, TOKEN_GLASS_TINT)?;
    emit_theme_color(&mut generated, "GLASS_EDGE_RGBA", &theme_runtime, TOKEN_GLASS_EDGE)?;

    // Dual theme snapshots (TASK-0072 Phase 9): the SAME token vocabulary baked
    // for BOTH qualifiers as `ThemeTokens` consts (BGRA), so the runtime light/
    // dark switch is a const swap + full redraw. Dark is the boot default.
    emit_theme_tokens(&mut generated, &theme_runtime, "THEME_DARK")?;
    theme_runtime.set_qualifier(Qualifier::Light);
    emit_theme_tokens(&mut generated, &theme_runtime, "THEME_LIGHT")?;
    theme_runtime.set_qualifier(Qualifier::Dark);

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

    let cursor_svg = normalized_mocu_cursor_svg();
    // The Mocu cursor's path geometry runs to y≈35.3 / x≈22.7, but its canvas was
    // declared 32×32 — rendering at the intrinsic size clipped the bottom tail.
    // The SVG now declares a 36×36 canvas that encloses the full artwork; render
    // it *scaled into* a 32×32 square so the complete cursor fits, nothing clipped
    // (nexus-svg maps the doc box onto the target — render-at-scale). Hotspot
    // (the tip, ~user (2,2)) stays ≈(2,2) after the 32/36 scale.
    const CURSOR_DIM: u32 = 32;
    const CURSOR_SS: u32 = 4;
    let cursor_hi = nexus_svg::render_svg_at(cursor_svg, CURSOR_DIM * CURSOR_SS, CURSOR_DIM * CURSOR_SS)
        .map_err(|err| std::io::Error::other(format!("render Mocu cursor SVG: {err:?}")))?;
    let cursor_bgra = box_average_downscale(&cursor_hi.buffer, cursor_hi.width, cursor_hi.height, CURSOR_SS);
    let cursor_path = out_dir.join("mocu_cursor.bgra");
    fs::write(&cursor_path, &cursor_bgra)?;
    writeln!(generated, "pub const MOCU_CURSOR_WIDTH: u32 = {CURSOR_DIM};")?;
    writeln!(generated, "pub const MOCU_CURSOR_HEIGHT: u32 = {CURSOR_DIM};")?;
    writeln!(
        generated,
        "pub const MOCU_CURSOR_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
        cursor_path.display()
    )?;
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_X: i32 = 2;")?;
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_Y: i32 = 2;")?;
    writeln!(generated, "pub const MOCU_CURSOR_LEFT_PTR_SVG: &str = r##\"{}\"##;", cursor_svg)?;

    // Resize pointer shapes (TASK-0070 Phase 3): the vendored cursor theme's
    // `ew`/`ns`/`nesw`/`nwse` variants, rendered through the same 4×-SSAA
    // pipeline at the same 32×32 as the default pointer. Hotspot = center
    // (16,16) — these are symmetric double-arrow shapes.
    for (name, svg) in [
        ("CURSOR_RESIZE_EW", include_str!("../../../resources/cursors/mocu/src/svg/ew-resize.svg")),
        ("CURSOR_RESIZE_NS", include_str!("../../../resources/cursors/mocu/src/svg/ns-resize.svg")),
        (
            "CURSOR_RESIZE_NESW",
            include_str!("../../../resources/cursors/mocu/src/svg/nesw-resize.svg"),
        ),
        (
            "CURSOR_RESIZE_NWSE",
            include_str!("../../../resources/cursors/mocu/src/svg/nwse-resize.svg"),
        ),
    ] {
        // The theme's shape sources use `<defs>` + `<use>` (unsupported by
        // nexus-svg) — normalize like the default pointer: inline the shared
        // path as dark outline + light fill, drop the faint offset shadow and
        // the red hotspot marker circle.
        let normalized = normalized_mocu_shape_svg(svg)
            .ok_or_else(|| std::io::Error::other(format!("normalize cursor {name}")))?;
        let hi =
            nexus_svg::render_svg_at(&normalized, CURSOR_DIM * CURSOR_SS, CURSOR_DIM * CURSOR_SS)
                .map_err(|err| std::io::Error::other(format!("render cursor {name}: {err:?}")))?;
        let bgra = box_average_downscale(&hi.buffer, hi.width, hi.height, CURSOR_SS);
        let path = out_dir.join(format!("{}.bgra", name.to_lowercase()));
        fs::write(&path, &bgra)?;
        writeln!(
            generated,
            "pub const {name}_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
            path.display()
        )?;
    }

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
        (
            "MINIMIZE_ICON",
            20u32,
            include_str!("../../../resources/icons/lucide/icons/minus.svg"),
        ),
        (
            "MAXIMIZE_ICON",
            16u32,
            include_str!("../../../resources/icons/lucide/icons/square.svg"),
        ),
        // Dock icons for minimized windows (one per shell window today).
        (
            "DOCK_CHAT_ICON",
            28u32,
            include_str!("../../../resources/icons/lucide/icons/message-circle.svg"),
        ),
        (
            "DOCK_SEARCH_ICON",
            28u32,
            include_str!("../../../resources/icons/lucide/icons/search.svg"),
        ),
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

fn emit_theme_color(
    generated: &mut File,
    const_name: &str,
    runtime: &ThemeRuntime,
    token: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let color = runtime.resolve(token)?;
    writeln!(
        generated,
        "pub const {const_name}: [u8; 4] = [{}, {}, {}, {}];",
        color.r, color.g, color.b, color.a
    )?;
    Ok(())
}

/// Emit a [`crate::theme::ThemeTokens`] const for the runtime's CURRENT
/// qualifier (TASK-0072 Phase 9). Each field is a BGRA array so surface
/// renderers can write it directly. The token names match the `.nxtheme.toml`
/// vocabulary.
fn emit_theme_tokens(
    generated: &mut File,
    runtime: &ThemeRuntime,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let f = |t: &str| -> Result<String, Box<dyn std::error::Error>> {
        let c = color_bgra(runtime.resolve(t)?);
        Ok(format!("[{}, {}, {}, {}]", c[0], c[1], c[2], c[3]))
    };
    writeln!(
        generated,
        "pub const {name}: crate::theme::ThemeTokens = crate::theme::ThemeTokens {{\n\
         \x20   surface: {}, surface_alt: {}, border: {}, fg: {}, muted_fg: {},\n\
         \x20   accent: {}, accent_fg: {}, glass_tint: {}, glass_edge: {},\n\
         }};",
        f("surface")?,
        f("surfaceAlt")?,
        f("border")?,
        f("fg")?,
        f("mutedFg")?,
        f("accent")?,
        f("accentFg")?,
        f("glassTint")?,
        f("glassEdge")?,
    )?;
    Ok(())
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
    let n = (factor * factor) as u32;
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
            for c in 0..3 {
                px[c] = (((px[c] as u32) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

/// Inline a mocu shape SVG (`<defs><path id="c" …/></defs>` + `<use>` layers)
/// into the plain-path form nexus-svg renders: dark stroked outline + light
/// fill from the shared geometry; the 10%-opacity offset shadow and the red
/// `id="hot"` marker circle are dropped (same policy as the default pointer).
fn normalized_mocu_shape_svg(raw: &str) -> Option<String> {
    let d = raw.split("<path id=\"c\" d=\"").nth(1)?.split('"').next()?;
    Some(format!(
        "<svg width=\"24\" height=\"24\" xmlns=\"http://www.w3.org/2000/svg\">\
         <path d=\"{d}\" fill=\"#1a1b1c\" stroke=\"#1a1b1c\" stroke-width=\"2\" stroke-linejoin=\"round\"/>\
         <path d=\"{d}\" fill=\"#fafbfc\"/></svg>"
    ))
}

fn normalized_mocu_cursor_svg() -> &'static str {
    // Canvas is 36×36 (not 32) so it encloses the path geometry, which extends to
    // y≈35.3 / x≈22.7 — a 32 canvas clipped the cursor's bottom tail. The build
    // renders this scaled into the target cursor square.
    "<svg width=\"36\" height=\"36\" xmlns=\"http://www.w3.org/2000/svg\">\
<path d=\"M 1 1 L 1 30 L 9.3 26.6 L 12.9 35.3 L 18 33 L 14.4 24.3 L 22.7 20.9 Z\" fill=\"#0a0b0c\"/>\
<path d=\"M 2 2 L 2 29.5 L 9.9 26.2 L 13.3 34.6 L 17 33 L 13.6 24.7 L 21.5 21.4 Z\" fill=\"#1a1b1c\"/>\
<path d=\"M 4 4 L 4 25 L 10.4 22.4 L 13.8 30.6 L 15.2 30 L 11.9 21.9 L 18.4 19.2 Z\" fill=\"#fafbfc\"/>\
</svg>"
}

/// TASK-0076B: compile the DSL demo page (`.nx`) to canonical `.nxir` bytes
/// embedded into windowd for the visible in-compositor mount. The compiler
/// runs host-side (build script); the service only reads the canonical IR.
const DSL_DEMO_NX: &str = "../../../examples/dsl/counter/counter.nx";

fn compile_dsl_demo(out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(DSL_DEMO_NX)?;
    let file = nexus_dsl_core::parse_file(&source)
        .map_err(|d| std::io::Error::other(format!("dsl demo parse: {} {}", d.code, d.message)))?;
    let (model, diags) = nexus_dsl_core::check_file(&file);
    if nexus_dsl_core::has_errors(&diags) {
        return Err(std::io::Error::other(format!("dsl demo check: {diags:?}")).into());
    }
    let canonical = nexus_dsl_core::format_file(&file);
    let lowered = nexus_dsl_core::lower_file(&file, &model, &canonical)
        .map_err(|d| std::io::Error::other(format!("dsl demo lower: {} {}", d.code, d.message)))?;
    fs::write(out_dir.join("dsl_demo.nxir"), &lowered.nxir)?;
    Ok(())
}
