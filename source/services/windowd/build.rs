// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

const INTER_FONT: &str = "../../../resources/fonts/inter/docs/font-files/InterVariable.ttf";
const MOCU_DEFAULT: &str = "../../../resources/cursors/mocu/src/svg/default.svg";
const TEXT_WIDTH: usize = 610;
const TEXT_HEIGHT: usize = 260;

fn main() {
    println!("cargo:rerun-if-changed={INTER_FONT}");
    println!("cargo:rerun-if-changed={MOCU_DEFAULT}");

    let text = render_text_overlay(Path::new(INTER_FONT)).expect("render Inter proof text");
    let out_dir = env::var_os("OUT_DIR").expect("OUT_DIR");
    let out_dir = Path::new(&out_dir);
    let text_path = out_dir.join("proof_text_inter.bgra");
    fs::write(&text_path, text).expect("write proof text overlay");

    let mocu = fs::read_to_string(MOCU_DEFAULT).expect("read Mocu default cursor");
    assert!(
        mocu.contains("#fafbfc") && mocu.contains("#1a1b1c") && mocu.contains("id=\"hot\""),
        "Mocu default cursor source shape changed"
    );

    let generated_path = out_dir.join("windowd_generated_assets.rs");
    let mut generated = File::create(generated_path).expect("create generated assets");
    writeln!(generated, "pub const PROOF_TEXT_WIDTH: u32 = {TEXT_WIDTH};").unwrap();
    writeln!(
        generated,
        "pub const PROOF_TEXT_HEIGHT: u32 = {TEXT_HEIGHT};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const PROOF_TEXT_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
        text_path.display()
    )
    .unwrap();
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_X: i32 = 2;").unwrap();
    writeln!(generated, "pub const MOCU_CURSOR_HOTSPOT_Y: i32 = 2;").unwrap();
    writeln!(
        generated,
        "pub const MOCU_CURSOR_LEFT_PTR_SVG: &str = r##\"{}\"##;",
        normalized_mocu_cursor_svg()
    )
    .unwrap();
}

fn render_text_overlay(path: &Path) -> Result<Vec<u8>, String> {
    let font_bytes = fs::read(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
        .map_err(|err| format!("parse Inter font: {err:?}"))?;
    let mut out = vec![0u8; TEXT_WIDTH * TEXT_HEIGHT * 4];

    draw_text(
        &font,
        &mut out,
        24,
        45,
        30.0,
        "Open Nexus OS",
        [0xff, 0xff, 0xff, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        25,
        82,
        18.0,
        "DisplayServer v0 - Inter variable font",
        [0xc8, 0xd8, 0xff, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        25,
        112,
        16.0,
        "Hover, click, scroll up/down, keyboard press",
        [0x9c, 0xac, 0xc8, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        38,
        231,
        16.0,
        "Hover",
        [0xf4, 0xf6, 0xff, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        180,
        231,
        16.0,
        "Click",
        [0xf4, 0xf6, 0xff, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        322,
        231,
        16.0,
        "Scroll",
        [0xf4, 0xf6, 0xff, 0xff],
    );
    draw_text(
        &font,
        &mut out,
        464,
        231,
        16.0,
        "Key",
        [0xf4, 0xf6, 0xff, 0xff],
    );
    Ok(out)
}

fn draw_text(
    font: &fontdue::Font,
    out: &mut [u8],
    mut pen_x: i32,
    baseline_y: i32,
    px: f32,
    text: &str,
    bgra: [u8; 4],
) {
    for ch in text.chars() {
        let (metrics, bitmap) = font.rasterize(ch, px);
        let x0 = pen_x + metrics.xmin;
        let y0 = baseline_y - metrics.height as i32 - metrics.ymin;
        for y in 0..metrics.height {
            let dst_y = y0 + y as i32;
            if dst_y < 0 || dst_y >= TEXT_HEIGHT as i32 {
                continue;
            }
            for x in 0..metrics.width {
                let dst_x = x0 + x as i32;
                if dst_x < 0 || dst_x >= TEXT_WIDTH as i32 {
                    continue;
                }
                let alpha = bitmap[y * metrics.width + x];
                if alpha == 0 {
                    continue;
                }
                let dst = (dst_y as usize * TEXT_WIDTH + dst_x as usize) * 4;
                blend(&mut out[dst..dst + 4], [bgra[0], bgra[1], bgra[2], alpha]);
            }
        }
        pen_x += metrics.advance_width.ceil() as i32;
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

fn normalized_mocu_cursor_svg() -> &'static str {
    "<svg width=\"32\" height=\"32\" xmlns=\"http://www.w3.org/2000/svg\">\
<path d=\"M 1 1 L 1 30 L 9.3 26.6 L 12.9 35.3 L 18 33 L 14.4 24.3 L 22.7 20.9 Z\" fill=\"#0a0b0c\"/>\
<path d=\"M 2 2 L 2 29.5 L 9.9 26.2 L 13.3 34.6 L 17 33 L 13.6 24.7 L 21.5 21.4 Z\" fill=\"#1a1b1c\"/>\
<path d=\"M 4 4 L 4 25 L 10.4 22.4 L 13.8 30.6 L 15.2 30 L 11.9 21.9 L 18.4 19.2 Z\" fill=\"#fafbfc\"/>\
</svg>"
}
