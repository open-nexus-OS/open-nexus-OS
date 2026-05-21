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
    TOKEN_CLICK, TOKEN_HOVER, TOKEN_ICON_BG, TOKEN_ICON_FG, TOKEN_KEYBOARD, TOKEN_PANEL_BG,
    TOKEN_PANEL_BORDER, TOKEN_PANEL_MUTED, TOKEN_PANEL_SUBTITLE, TOKEN_PANEL_TITLE, TOKEN_SCROLL,
};

const INTER_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";
const MOCU_DEFAULT: &str = "../../../resources/cursors/mocu/src/svg/default.svg";
const THEMES_DIR: &str = "../../../resources/themes";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={INTER_FONT}");
    println!("cargo:rerun-if-changed={MOCU_DEFAULT}");
    println!("cargo:rerun-if-changed={THEMES_DIR}");
    println!("cargo:rerun-if-changed=src/proof_panel_spec.rs");

    let out_dir = env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let out_dir = Path::new(&out_dir);
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
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_BG_RGBA",
        &theme_runtime,
        TOKEN_PANEL_BG,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_BORDER_RGBA",
        &theme_runtime,
        TOKEN_PANEL_BORDER,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_TITLE_RGBA",
        &theme_runtime,
        TOKEN_PANEL_TITLE,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_SUBTITLE_RGBA",
        &theme_runtime,
        TOKEN_PANEL_SUBTITLE,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_PANEL_MUTED_RGBA",
        &theme_runtime,
        TOKEN_PANEL_MUTED,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_CARD_BG_RGBA",
        &theme_runtime,
        TOKEN_CARD_BG,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_CARD_ACTIVE_BG_RGBA",
        &theme_runtime,
        TOKEN_CARD_ACTIVE_BG,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_CARD_BORDER_RGBA",
        &theme_runtime,
        TOKEN_CARD_BORDER,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_CARD_LABEL_RGBA",
        &theme_runtime,
        TOKEN_CARD_LABEL,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_ICON_BG_RGBA",
        &theme_runtime,
        TOKEN_ICON_BG,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_ICON_FG_RGBA",
        &theme_runtime,
        TOKEN_ICON_FG,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_HOVER_RGBA",
        &theme_runtime,
        TOKEN_HOVER,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_CLICK_RGBA",
        &theme_runtime,
        TOKEN_CLICK,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_SCROLL_RGBA",
        &theme_runtime,
        TOKEN_SCROLL,
    )?;
    emit_theme_color(
        &mut generated,
        "PROOF_KEYBOARD_RGBA",
        &theme_runtime,
        TOKEN_KEYBOARD,
    )?;

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
        writeln!(
            generated,
            "pub const {const_prefix}_WIDTH: u32 = {};",
            rendered.width
        )?;
        writeln!(
            generated,
            "pub const {const_prefix}_HEIGHT: u32 = {};",
            rendered.height
        )?;
        writeln!(
            generated,
            "pub const {const_prefix}_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
            text_path.display()
        )?;
    }
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_X: i32 = 2;")?;
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_Y: i32 = 2;")?;
    writeln!(
        generated,
        "pub const MOCU_CURSOR_LEFT_PTR_SVG: &str = r##\"{}\"##;",
        normalized_mocu_cursor_svg()
    )?;
    Ok(())
}

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
        return RenderedText {
            width: 1,
            height: 1,
            data: vec![0u8; 4],
        };
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
    RenderedText {
        width,
        height,
        data: out,
    }
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

fn color_bgra(color: ColorValue) -> [u8; 4] {
    [color.b, color.g, color.r, color.a]
}

fn const_prefix(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn normalized_mocu_cursor_svg() -> &'static str {
    "<svg width=\"32\" height=\"32\" xmlns=\"http://www.w3.org/2000/svg\">\
<path d=\"M 1 1 L 1 30 L 9.3 26.6 L 12.9 35.3 L 18 33 L 14.4 24.3 L 22.7 20.9 Z\" fill=\"#0a0b0c\"/>\
<path d=\"M 2 2 L 2 29.5 L 9.9 26.2 L 13.3 34.6 L 17 33 L 13.6 24.7 L 21.5 21.4 Z\" fill=\"#1a1b1c\"/>\
<path d=\"M 4 4 L 4 25 L 10.4 22.4 L 13.8 30.6 L 15.2 30 L 11.9 21.9 L 18.4 19.2 Z\" fill=\"#fafbfc\"/>\
</svg>"
}
