// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

/// Cursor image with bitmap data and hotspot offset.
#[derive(Debug, Clone)]
pub struct CursorAsset {
    /// Cursor name (e.g. "left_ptr", "right_ptr").
    pub name: String,
    /// BGRA8888 pixel data, row-major.
    pub bitmap: Vec<u8>,
    /// Cursor width in pixels.
    pub width: u32,
    /// Cursor height in pixels.
    pub height: u32,
    /// Hotspot X offset from left edge.
    pub hotspot_x: u32,
    /// Hotspot Y offset from top edge.
    pub hotspot_y: u32,
}

/// A loaded BreezeX cursor theme.
#[derive(Debug, Clone)]
pub struct CursorSet {
    pub name: String,
    pub cursors: HashMap<String, CursorAsset>,
}

/// Load a BreezeX cursor theme from a directory.
///
/// The directory should contain SVG cursor files named like `left_ptr.svg`,
/// `right_ptr.svg`, etc. Each SVG is rasterized via `nexus-svg`, and the
/// hotspot is determined by the cursor name using `DEFAULT_HOTSPOTS`.
pub fn load_cursor_set(dir: &std::path::Path) -> Result<CursorSet, String> {
    let mut cursors = HashMap::new();

    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "svg" {
            continue;
        }

        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();

        let svg_data = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;

        let output = nexus_svg::render_svg(&svg_data).map_err(|e| format!("svg render: {e}"))?;

        let (hx, hy) = super::hotspot::hotspot_for(&name, output.width, output.height);

        cursors.insert(
            name.clone(),
            CursorAsset {
                name,
                bitmap: output.buffer,
                width: output.width,
                height: output.height,
                hotspot_x: hx,
                hotspot_y: hy,
            },
        );
    }

    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();

    Ok(CursorSet { name: dir_name, cursors })
}
