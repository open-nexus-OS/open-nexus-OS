// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const WALLPAPER: &str = "../../../resources/wallpapers/base/default.jpeg";
/// Dark-mode wallpaper (falls back to the light one when absent).
const WALLPAPER_DARK: &str = "../../../resources/wallpapers/base/default.dark.jpg";
const SHELL_MANIFEST: &str = "manifests/shells/desktop/shell.toml";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={WALLPAPER}");
    println!("cargo:rerun-if-changed={WALLPAPER_DARK}");
    println!("cargo:rerun-if-changed={SHELL_MANIFEST}");

    let (target_width, target_height) =
        first_frame_size(Path::new(SHELL_MANIFEST)).unwrap_or((160, 100));
    let wallpaper = decode_wallpaper(Path::new(WALLPAPER), target_width, target_height)
        .map_err(std::io::Error::other)?;
    // Theme-matched wallpaper: bake the dark variant too (same target size);
    // a missing dark asset falls back to the light bytes — never a build break.
    let dark_path = Path::new(WALLPAPER_DARK);
    let wallpaper_dark = if dark_path.exists() {
        decode_wallpaper(dark_path, target_width, target_height).map_err(std::io::Error::other)?
    } else {
        wallpaper.clone()
    };

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    // ROW-RLE both variants: raw 2×4MB BGRA overflowed the image RAM region;
    // runs of `[len:u16 LE][b g r a]` per row + a row-offset table decode
    // stack-side in the compositor — full resolution, ~10× smaller.
    let (light_rle, light_rows) = rle_encode(&wallpaper, target_width, target_height);
    let (dark_rle, dark_rows) = rle_encode(&wallpaper_dark, target_width, target_height);
    let rle_path = out_dir.join("default_wallpaper.rle");
    fs::write(&rle_path, &light_rle)?;
    let dark_rle_path = out_dir.join("default_wallpaper_dark.rle");
    fs::write(&dark_rle_path, &dark_rle)?;

    let generated_path = out_dir.join("wallpaper_generated.rs");
    let mut generated = File::create(&generated_path)?;
    writeln!(generated, "pub const WALLPAPER_WIDTH: u32 = {target_width};")?;
    writeln!(generated, "pub const WALLPAPER_HEIGHT: u32 = {target_height};")?;
    writeln!(
        generated,
        "pub const WALLPAPER_RLE: &[u8] = include_bytes!(r#\"{}\"#);",
        rle_path.display()
    )?;
    writeln!(
        generated,
        "pub const WALLPAPER_DARK_RLE: &[u8] = include_bytes!(r#\"{}\"#);",
        dark_rle_path.display()
    )?;
    write_rows(&mut generated, "WALLPAPER_ROWS", &light_rows)?;
    write_rows(&mut generated, "WALLPAPER_DARK_ROWS", &dark_rows)?;
    Ok(())
}

/// Per-row QOI-style encode of opaque BGRA (QOI ops, state reset per row so
/// rows stay randomly accessible): RUN (up to 62), INDEX (64-slot hash),
/// DIFF (2-bit channel deltas), LUMA (green-relative), RGB literal. Photos
/// compress ~2-4×; identical-run RLE did NOT (JPEG noise kills runs).
/// Returns (data, row offsets len h+1). Decoder = `frame::decode_qoi_row`.
fn rle_encode(bgra: &[u8], width: u32, height: u32) -> (Vec<u8>, Vec<u32>) {
    let w = width as usize;
    let mut data = Vec::new();
    let mut rows = Vec::with_capacity(height as usize + 1);
    for y in 0..height as usize {
        rows.push(data.len() as u32);
        let row = &bgra[y * w * 4..(y + 1) * w * 4];
        let mut index = [[0u8; 3]; 64];
        let mut prev = [0u8, 0u8, 0u8]; // b, g, r
        let mut run = 0u8;
        for x in 0..w {
            let px = [row[x * 4], row[x * 4 + 1], row[x * 4 + 2]];
            if px == prev {
                run += 1;
                if run == 62 {
                    data.push(0b1100_0000 | (run - 1));
                    run = 0;
                }
                continue;
            }
            if run > 0 {
                data.push(0b1100_0000 | (run - 1));
                run = 0;
            }
            let hash =
                (px[2] as usize * 3 + px[1] as usize * 5 + px[0] as usize * 7 + 255 * 11) % 64;
            if index[hash] == px {
                data.push(hash as u8); // QOI_OP_INDEX (top bits 00)
                prev = px;
                continue;
            }
            index[hash] = px;
            let db = px[0].wrapping_sub(prev[0]) as i8 as i16;
            let dg = px[1].wrapping_sub(prev[1]) as i8 as i16;
            let dr = px[2].wrapping_sub(prev[2]) as i8 as i16;
            if (-2..=1).contains(&dr) && (-2..=1).contains(&dg) && (-2..=1).contains(&db) {
                data.push(
                    0b0100_0000
                        | (((dr + 2) as u8) << 4)
                        | (((dg + 2) as u8) << 2)
                        | ((db + 2) as u8),
                );
            } else {
                let dr_dg = dr - dg;
                let db_dg = db - dg;
                if (-32..=31).contains(&dg)
                    && (-8..=7).contains(&dr_dg)
                    && (-8..=7).contains(&db_dg)
                {
                    data.push(0b1000_0000 | ((dg + 32) as u8));
                    data.push((((dr_dg + 8) as u8) << 4) | ((db_dg + 8) as u8));
                } else {
                    data.push(0b1111_1110); // QOI_OP_RGB
                    data.push(px[2]); // r
                    data.push(px[1]); // g
                    data.push(px[0]); // b
                }
            }
            prev = px;
        }
        if run > 0 {
            data.push(0b1100_0000 | (run - 1));
        }
    }
    rows.push(data.len() as u32);
    (data, rows)
}

fn write_rows(out: &mut File, name: &str, rows: &[u32]) -> std::io::Result<()> {
    write!(out, "pub const {name}: &[u32] = &[")?;
    for (i, r) in rows.iter().enumerate() {
        if i % 16 == 0 {
            writeln!(out)?;
        }
        write!(out, "{r}, ")?;
    }
    writeln!(out, "\n];")?;
    Ok(())
}

fn first_frame_size(path: &Path) -> Option<(u32, u32)> {
    let manifest = fs::read_to_string(path).ok()?;
    let mut in_first_frame = false;
    let mut width = None;
    let mut height = None;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_first_frame = line == "[first_frame]";
            continue;
        }
        if !in_first_frame {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse::<u32>().ok(),
            "height" => height = value.trim().parse::<u32>().ok(),
            _ => {}
        }
    }
    Some((width?, height?))
}

fn decode_wallpaper(path: &Path, target_width: u32, target_height: u32) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|err| format!("open {}: {err}", path.display()))?;
    let mut decoder = jpeg_decoder::Decoder::new(file);
    let pixels = decoder.decode().map_err(|err| format!("decode JPEG: {err}"))?;
    let info = decoder.info().ok_or("missing JPEG info")?;
    let src_width = usize::from(info.width);
    let src_height = usize::from(info.height);
    let target_width = target_width as usize;
    let target_height = target_height as usize;
    let mut out = vec![0u8; target_width * target_height * 4];

    // COVER, not stretch (the design contract: `object-fit: cover`). The source
    // is 3:2 and the panel 8:5, so mapping the full source onto the full target
    // squashed the image ~7% — mountains got wider, faces would get fatter.
    // Take the largest CENTRED window of the source that has the target's
    // aspect and scale that: one axis is used whole, the other is cropped
    // evenly on both sides. Never a letterbox — the window is inscribed in the
    // source, so every destination pixel has source behind it.
    // (windowd's `build_cover_luts` applies the same rule again at runtime for
    // a non-native display mode; at the native mode it is the identity, so the
    // image is cropped once, here.)
    let (win_w, win_h) = if src_width * target_height >= src_height * target_width {
        // Source is WIDER than the target: full height, crop the width.
        ((src_height * target_width / target_height).max(1).min(src_width), src_height)
    } else {
        // Source is TALLER than the target: full width, crop the height.
        (src_width, (src_width * target_height / target_width).max(1).min(src_height))
    };
    let win_x = (src_width - win_w) / 2;
    let win_y = (src_height - win_h) / 2;

    // Resample the window with a separable Lanczos-3 filter (`resample_axis`): upscaling
    // reconstructs a smooth, sharp image (the former box filter degenerated to nearest-neighbour
    // above 1:1 — blocky at the 1.25× a 1536-wide source needs for 1920), downscaling widens
    // the kernel by the scale (an area-correct low-pass: no aliasing). TASK-0251 P2a step 3.
    let mut src = vec![0f32; win_w * win_h * 3];
    for y in 0..win_h {
        for x in 0..win_w {
            let (r, g, b) = rgb_at(&pixels, info.pixel_format, src_width, win_x + x, win_y + y)?;
            let o = (y * win_w + x) * 3;
            src[o..o + 3].copy_from_slice(&[f32::from(r), f32::from(g), f32::from(b)]);
        }
    }
    // Horizontal pass: win_w → target_width, every window row.
    let cols = resample_axis(win_w, target_width);
    let mut wide = vec![0f32; target_width * win_h * 3];
    for y in 0..win_h {
        for (x, taps) in cols.iter().enumerate() {
            let mut acc = [0f32; 3];
            for &(sx, w) in taps {
                let o = (y * win_w + sx) * 3;
                for c in 0..3 {
                    acc[c] += src[o + c] * w;
                }
            }
            let o = (y * target_width + x) * 3;
            wide[o..o + 3].copy_from_slice(&acc);
        }
    }
    // Vertical pass: win_h → target_height.
    let rows = resample_axis(win_h, target_height);
    for (y, taps) in rows.iter().enumerate() {
        for x in 0..target_width {
            let mut acc = [0f32; 3];
            for &(sy, w) in taps {
                let o = (sy * target_width + x) * 3;
                for c in 0..3 {
                    acc[c] += wide[o + c] * w;
                }
            }
            let channel = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            let dst = (y * target_width + x) * 4;
            out[dst..dst + 4].copy_from_slice(&[
                channel(acc[2]),
                channel(acc[1]),
                channel(acc[0]),
                0xff,
            ]);
        }
    }

    Ok(out)
}

/// The Lanczos-3 kernel.
fn lanczos3(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= 3.0 {
        return 0.0;
    }
    let px = core::f32::consts::PI * x;
    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
}

/// The taps of a 1-D resample from `src_len` to `dst_len` samples: per destination sample the
/// `(source index, weight)` pairs, weights normalized to 1, sources clamped at the edges. The
/// kernel is Lanczos-3 in destination units, widened by the scale when it shrinks.
fn resample_axis(src_len: usize, dst_len: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src_len as f32 / dst_len as f32;
    let support = 3.0 * scale.max(1.0);
    (0..dst_len)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale - 0.5;
            let lo = (centre - support).ceil() as i64;
            let hi = (centre + support).floor() as i64;
            let mut taps: Vec<(usize, f32)> = (lo..=hi)
                .map(|j| {
                    let w = lanczos3((j as f32 - centre) / scale.max(1.0));
                    (j.clamp(0, src_len as i64 - 1) as usize, w)
                })
                .filter(|&(_, w)| w != 0.0)
                .collect();
            let sum: f32 = taps.iter().map(|&(_, w)| w).sum();
            for tap in &mut taps {
                tap.1 /= sum;
            }
            taps
        })
        .collect()
}

fn rgb_at(
    pixels: &[u8],
    format: jpeg_decoder::PixelFormat,
    width: usize,
    x: usize,
    y: usize,
) -> Result<(u8, u8, u8), String> {
    let idx = y
        .checked_mul(width)
        .and_then(|base| base.checked_add(x))
        .ok_or("wallpaper index overflow")?;
    match format {
        jpeg_decoder::PixelFormat::L8 => {
            let l = *pixels.get(idx).ok_or("truncated L8 JPEG")?;
            Ok((l, l, l))
        }
        jpeg_decoder::PixelFormat::RGB24 => {
            let offset = idx.checked_mul(3).ok_or("wallpaper RGB index overflow")?;
            let rgb = pixels.get(offset..offset + 3).ok_or("truncated RGB JPEG")?;
            Ok((rgb[0], rgb[1], rgb[2]))
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            let offset = idx.checked_mul(4).ok_or("wallpaper CMYK index overflow")?;
            let cmyk = pixels.get(offset..offset + 4).ok_or("truncated CMYK JPEG")?;
            let c = u16::from(cmyk[0]);
            let m = u16::from(cmyk[1]);
            let y = u16::from(cmyk[2]);
            let k = u16::from(cmyk[3]);
            let r = 255u16.saturating_sub((c + k).min(255)) as u8;
            let g = 255u16.saturating_sub((m + k).min(255)) as u8;
            let b = 255u16.saturating_sub((y + k).min(255)) as u8;
            Ok((r, g, b))
        }
        _ => Err("unsupported JPEG pixel format".to_string()),
    }
}
