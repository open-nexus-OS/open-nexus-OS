// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `nexus-app-icons` — the baked REAL app-icon artwork. Each app
//! bundle ships its icon as `assets/icon.svg` (declared in `manifest.toml`
//! as `icon_svg`); the build script rasterizes it through `nexus-svg`
//! (gradients, groups, the full artwork) into straight-alpha RGBA sprites at
//! the shell's tile sizes. Runtime is a pure table lookup — no parsing, no
//! allocation, `no_std`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 tests (lookup + pixel sanity)

mod baked {
    include!(concat!(env!("OUT_DIR"), "/app_icons.rs"));
}

/// One baked sprite: `size × size` straight-alpha RGBA rows.
#[derive(Debug, Clone, Copy)]
pub struct AppIconSprite {
    pub size: u32,
    /// `size * size * 4` bytes, `[r, g, b, a]` per pixel, row-major.
    pub rgba: &'static [u8],
}

/// The baked sprite for an app id at an exact tile size (64/44/32), or the
/// LARGEST baked size as a fallback (the painter samples nearest, so any box
/// size renders). `None` = the app ships no `icon_svg` artwork.
#[must_use]
pub fn sprite(id: &str, size: u32) -> Option<AppIconSprite> {
    if let Some(rgba) = baked::sprite_bytes(id, size) {
        return Some(AppIconSprite { size, rgba });
    }
    for &s in &[64u32, 44, 32] {
        if let Some(rgba) = baked::sprite_bytes(id, s) {
            return Some(AppIconSprite { size: s, rgba });
        }
    }
    None
}

/// Whether an app ships real icon artwork (drives the shell's tile branch).
#[must_use]
pub fn has_artwork(id: &str) -> bool {
    sprite(id, 64).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_sprite_exists_at_all_tile_sizes() {
        for size in [64, 44, 32] {
            let s = sprite("calculator", size).expect("baked");
            assert_eq!(s.size, size);
            assert_eq!(s.rgba.len() as u32, size * size * 4);
        }
        assert!(sprite("no-such-app", 64).is_none());
    }

    #[test]
    fn sprite_has_opaque_center_and_transparent_corner() {
        let s = sprite("chat", 64).expect("baked");
        let px = |x: u32, y: u32| {
            let o = ((y * s.size + x) * 4) as usize;
            (s.rgba[o], s.rgba[o + 1], s.rgba[o + 2], s.rgba[o + 3])
        };
        assert_eq!(px(0, 0).3, 0, "squircle corner transparent");
        assert!(px(32, 32).3 > 200, "center opaque");
    }
}
