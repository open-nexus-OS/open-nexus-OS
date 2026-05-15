// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const WALLPAPER: &str = "../../../resources/wallpapers/base/default.jpeg";
const SHELL_MANIFEST: &str = "manifests/shells/desktop/shell.toml";

fn main() {
    println!("cargo:rerun-if-changed={WALLPAPER}");
    println!("cargo:rerun-if-changed={SHELL_MANIFEST}");

    let (target_width, target_height) =
        first_frame_size(Path::new(SHELL_MANIFEST)).unwrap_or((160, 100));
    let wallpaper = decode_wallpaper(Path::new(WALLPAPER), target_width, target_height)
        .expect("decode default JPEG wallpaper");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let bgra_path = out_dir.join("default_wallpaper.bgra");
    fs::write(&bgra_path, wallpaper).expect("write generated wallpaper BGRA");

    let generated_path = out_dir.join("wallpaper_generated.rs");
    let mut generated = File::create(&generated_path).expect("create wallpaper generated rs");
    writeln!(
        generated,
        "pub const WALLPAPER_WIDTH: u32 = {target_width};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const WALLPAPER_HEIGHT: u32 = {target_height};"
    )
    .unwrap();
    writeln!(
        generated,
        "pub const WALLPAPER_BGRA: &[u8] = include_bytes!(r#\"{}\"#);",
        bgra_path.display()
    )
    .unwrap();
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
    let pixels = decoder
        .decode()
        .map_err(|err| format!("decode JPEG: {err}"))?;
    let info = decoder.info().ok_or("missing JPEG info")?;
    let src_width = usize::from(info.width);
    let src_height = usize::from(info.height);
    let target_width = target_width as usize;
    let target_height = target_height as usize;
    let mut out = vec![0u8; target_width * target_height * 4];

    for y in 0..target_height {
        let src_y = y * src_height / target_height;
        for x in 0..target_width {
            let src_x = x * src_width / target_width;
            let (r, g, b) = rgb_at(&pixels, info.pixel_format, src_width, src_x, src_y)?;
            let dst = (y * target_width + x) * 4;
            out[dst..dst + 4].copy_from_slice(&[b, g, r, 0xff]);
        }
    }

    Ok(out)
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
            let cmyk = pixels
                .get(offset..offset + 4)
                .ok_or("truncated CMYK JPEG")?;
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
