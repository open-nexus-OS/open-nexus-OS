// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::OnceLock;

/// Default hotspot map for common X11/BreezeX cursor names.
/// Values are fractions (0.0–1.0) of cursor dimensions.
static HOTSPOT_MAP: &[(&str, f32, f32)] = &[
    ("left_ptr", 0.15, 0.15),
    ("right_ptr", 0.85, 0.15),
    ("cross", 0.50, 0.50),
    ("hand2", 0.50, 0.10),
    ("watch", 0.50, 0.50),
    ("sb_h_double_arrow", 0.50, 0.50),
    ("sb_v_double_arrow", 0.50, 0.50),
    ("top_side", 0.50, 0.20),
    ("bottom_side", 0.50, 0.80),
    ("left_side", 0.20, 0.50),
    ("right_side", 0.80, 0.50),
    ("top_left_corner", 0.20, 0.20),
    ("top_right_corner", 0.80, 0.20),
    ("bottom_left_corner", 0.20, 0.80),
    ("bottom_right_corner", 0.80, 0.80),
    ("move", 0.50, 0.50),
    ("copy", 0.30, 0.30),
    ("link", 0.20, 0.10),
    ("circle", 0.50, 0.50),
    ("dot", 0.50, 0.50),
    ("arrow", 0.15, 0.15),
    ("dnd-none", 0.50, 0.50),
    ("dnd-copy", 0.30, 0.30),
    ("dnd-move", 0.50, 0.50),
    ("dnd-link", 0.20, 0.10),
    ("text", 0.50, 0.85),
    ("help", 0.15, 0.15),
    ("progress", 0.30, 0.30),
    ("wait", 0.50, 0.50),
    ("default", 0.15, 0.15),
];

fn hotspots() -> &'static HashMap<&'static str, (f32, f32)> {
    static HOTSPOTS: OnceLock<HashMap<&'static str, (f32, f32)>> = OnceLock::new();
    HOTSPOTS.get_or_init(|| HOTSPOT_MAP.iter().map(|&(k, x, y)| (k, (x, y))).collect())
}

/// Compute the hotspot in pixel coordinates for a cursor of given dimensions.
pub fn hotspot_for(name: &str, width: u32, height: u32) -> (u32, u32) {
    let (fx, fy) = hotspots().get(name).copied().unwrap_or((0.5, 0.5));
    ((width as f32 * fx).round() as u32, (height as f32 * fy).round() as u32)
}
