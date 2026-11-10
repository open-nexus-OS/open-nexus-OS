// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pointer's sprites in both upstream Mocu variants (TASK-0353 P8). The colours come from the
//! cursor theme's own `src/themes.txt` (fill, stroke, shadow per variant) — nothing here names a
//! colour of its own — and every shape plus the wait ring is rendered once per variant into
//! `windowd_generated_cursor.rs`: `WHITE_*` (light pointer, dark outline) and `BLACK_*` (dark
//! pointer, light outline). Light mode shows the black set, dark mode the white one
//! (`src/cursor_assets.rs`). Every shape — the arrow included — is drawn from its upstream path
//! the way the theme draws it: the shadow (the outline moved one unit down-right at a tenth of
//! its opacity), the outline (stroke 2, round joins), the body. The theme's 24-unit canvas maps
//! onto the 32 px "normal" size (docs/dev/ui/foundations/visual/cursor-themes.md);
//! 4×-supersampled through nexus-svg, premultiplied BGRA, 32×32.

use std::fs;
use std::io::Write;
use std::path::Path;

const MOCU_SRC: &str = "../../../resources/cursors/mocu/src";
/// The sprite square (px) and the supersampling factor.
const DIM: u32 = 32;
const SS: u32 = 4;
/// Wait-ring frames per turn (windowd steps one every 90 ms).
const RING_FRAMES: u32 = 8;
/// The shapes besides the arrow: generated name and upstream source.
const SHAPES: [(&str, &str); 5] = [
    ("RESIZE_EW", "ew-resize.svg"),
    ("RESIZE_NS", "ns-resize.svg"),
    ("RESIZE_NESW", "nesw-resize.svg"),
    ("RESIZE_NWSE", "nwse-resize.svg"),
    ("TEXT", "text.svg"),
];
/// The two sets: generated prefix and the upstream theme line they take their colours from.
const SETS: [(&str, &str); 2] = [("WHITE", "Mocu-White-Right"), ("BLACK", "Mocu-Black-Right")];

/// One upstream variant's colours (`#rrggbb`).
struct Colours {
    fill: String,
    stroke: String,
    shadow: String,
}

/// The colours of `theme` in `themes.txt` (`Name Fill Stroke Shadow Handedness`).
fn colours(themes: &str, theme: &str) -> Result<Colours, String> {
    let line = themes
        .lines()
        .find(|l| l.split_whitespace().next() == Some(theme))
        .ok_or_else(|| format!("cursor theme {theme} missing from themes.txt"))?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    let hex = |s: &str| (s.len() == 7 && s.starts_with('#')).then(|| s.to_string());
    match (
        cols.get(1).and_then(|s| hex(s)),
        cols.get(2).and_then(|s| hex(s)),
        cols.get(3).and_then(|s| hex(s)),
    ) {
        (Some(fill), Some(stroke), Some(shadow)) => Ok(Colours { fill, stroke, shadow }),
        _ => Err(format!("cursor theme {theme}: expected three #rrggbb colours")),
    }
}

/// One upstream shape (its `<path id="c" … d="…">`, attributes in any order) in a variant's
/// colours, as the theme's own SVGs draw it: the shadow, the outline, the body.
fn mocu_svg(raw: &str, c: &Colours) -> Option<String> {
    let element = raw.split("<path id=\"c\"").nth(1)?.split("/>").next()?;
    let d = element.split(" d=\"").nth(1)?.split('"').next()?;
    Some(format!(
        "<svg width=\"24\" height=\"24\" xmlns=\"http://www.w3.org/2000/svg\">\
         <path d=\"{d}\" transform=\"translate(1,1)\" fill=\"{sh}\" stroke=\"{sh}\" \
         stroke-width=\"2\" stroke-linejoin=\"round\" opacity=\"0.1\"/>\
         <path d=\"{d}\" fill=\"{s}\" stroke=\"{s}\" stroke-width=\"2\" stroke-linejoin=\"round\"/>\
         <path d=\"{d}\" fill=\"{f}\"/></svg>",
        sh = c.shadow,
        s = c.stroke,
        f = c.fill
    ))
}

fn bgr(hex: &str) -> (f32, f32, f32) {
    let ch = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f32;
    (ch(5), ch(3), ch(1))
}

fn render(svg: &str, what: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let hi = nexus_svg::render_svg_at(svg, DIM * SS, DIM * SS)
        .map_err(|err| std::io::Error::other(format!("render cursor {what}: {err:?}")))?;
    Ok(super::box_average_downscale(&hi.buffer, hi.width, hi.height, SS))
}

fn emit_sprite(
    out: &mut fs::File,
    out_dir: &Path,
    name: &str,
    bgra: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let path = out_dir.join(format!("cursor_{}.bgra", name.to_lowercase()));
    fs::write(&path, bgra)?;
    writeln!(out, "pub const {name}: &[u8] = include_bytes!(r#\"{}\"#);", path.display())?;
    Ok(())
}

/// Writes `windowd_generated_cursor.rs`: per set the arrow, the five shapes and the ring frames.
pub(crate) fn emit(out_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={MOCU_SRC}");
    let themes = fs::read_to_string(format!("{MOCU_SRC}/themes.txt"))?;
    let default = fs::read_to_string(format!("{MOCU_SRC}/svg/default.svg"))?;
    if !(default.contains("#fafbfc")
        && default.contains("#1a1b1c")
        && default.contains("id=\"hot\""))
    {
        return Err("Mocu default cursor source shape changed".into());
    }
    let mut out = fs::File::create(out_dir.join("windowd_generated_cursor.rs"))?;
    writeln!(
        out,
        "// @generated by build_support/cursors.rs from resources/cursors/mocu — do not edit."
    )?;
    writeln!(out, "pub const DIM: u32 = {DIM};")?;
    for (set, theme) in SETS {
        let c = colours(&themes, theme)?;
        let arrow = mocu_svg(&default, &c).ok_or("normalize cursor default.svg")?;
        emit_sprite(&mut out, out_dir, &format!("{set}_ARROW"), &render(&arrow, "default.svg")?)?;
        for (name, file) in SHAPES {
            let raw = fs::read_to_string(format!("{MOCU_SRC}/svg/{file}"))?;
            let svg = mocu_svg(&raw, &c).ok_or_else(|| format!("normalize cursor {file}"))?;
            emit_sprite(&mut out, out_dir, &format!("{set}_{name}"), &render(&svg, file)?)?;
        }
        // The ring is the set's own: its body colour over a halo in its outline colour.
        let (core, halo) = (bgr(&c.fill), bgr(&c.stroke));
        for f in 0..RING_FRAMES {
            let bgra = ring_frame(f, core, halo);
            emit_sprite(&mut out, out_dir, &format!("{set}_RING_{f}"), &bgra)?;
        }
        let frames: Vec<String> = (0..RING_FRAMES).map(|f| format!("{set}_RING_{f}")).collect();
        writeln!(out, "pub const {set}_RING: [&[u8]; {RING_FRAMES}] = [{}];", frames.join(", "))?;
    }
    Ok(())
}

/// Frame `frame` of the activity ring: a core ring with a rotating fading tail over a soft halo,
/// distance-based AA, premultiplied BGRA. Frame `i` leads at `i/N × 2π`.
fn ring_frame(frame: u32, core: (f32, f32, f32), halo: (f32, f32, f32)) -> Vec<u8> {
    let dim = DIM;
    let mut out = vec![0u8; (dim * dim * 4) as usize];
    let c = dim as f32 / 2.0;
    let r_mid = dim as f32 * 0.34;
    let (core_half, halo_half) = (1.6f32, 2.8f32);
    let lead = frame as f32 / RING_FRAMES as f32 * core::f32::consts::TAU;
    for y in 0..dim {
        for x in 0..dim {
            let dx = x as f32 + 0.5 - c;
            let dy = y as f32 + 0.5 - c;
            let d = (dx * dx + dy * dy).sqrt();
            // Angular tail: 1.0 at the leading angle, fading to 0.12 behind it.
            let mut behind = (lead - dy.atan2(dx)) / core::f32::consts::TAU;
            behind -= behind.floor();
            let tail = 1.0 - behind * 0.88;
            let core_cov = (core_half - (d - r_mid).abs() + 0.5).clamp(0.0, 1.0);
            let halo_cov = (halo_half - (d - r_mid).abs() + 0.5).clamp(0.0, 1.0);
            let a_halo = halo_cov * 0.85 * tail;
            let a_core = core_cov * tail;
            let a = a_core + a_halo * (1.0 - a_core);
            if a <= 0.0 {
                continue;
            }
            let blend = |cc: f32, hc: f32| cc * a_core + hc * a_halo * (1.0 - a_core);
            let i = ((y * dim + x) * 4) as usize;
            out[i] = blend(core.0, halo.0) as u8;
            out[i + 1] = blend(core.1, halo.1) as u8;
            out[i + 2] = blend(core.2, halo.2) as u8;
            out[i + 3] = (a * 255.0) as u8;
        }
    }
    out
}
