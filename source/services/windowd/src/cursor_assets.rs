// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pointer's sprites in both Mocu variants (TASK-0353 P8): the white set — a light
//! pointer with a dark outline — for a dark theme, the black set — a dark pointer with a light
//! outline — for a light theme. Both are rendered at build time from the cursor theme's own
//! colours (`build_support/cursors.rs`); windowd uploads the active set to gpud's shape cache and
//! swaps it when the theme changes (`compositor/runtime/cursor.rs`).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host unit tests below (the theme picks the set; every sprite is a full square;
//!   the two sets differ where the pointer's body is)

mod generated {
    include!(concat!(env!("OUT_DIR"), "/windowd_generated_cursor.rs"));
}

/// The sprite square (px).
pub(crate) const DIM: u32 = generated::DIM;
/// The arrow's hotspot: its tip — upstream `default.svg`'s `hot` point (1, 1) on the 24-unit
/// canvas, at the 32 px size.
pub(crate) const ARROW_HOT: (i32, i32) = (1, 1);
/// The symmetric shapes' and the ring's hotspot: the centre.
pub(crate) const CENTER_HOT: i32 = 16;
/// Wait-ring frames per turn.
pub(crate) const RING_FRAMES: usize = 8;

/// One variant's sprites: premultiplied BGRA, `DIM`×`DIM` each.
pub(crate) struct CursorSet {
    /// Marker-stable name (`windowd: cursor theme (… set=)`).
    pub(crate) name: &'static str,
    pub(crate) arrow: &'static [u8],
    pub(crate) resize_ew: &'static [u8],
    pub(crate) resize_ns: &'static [u8],
    pub(crate) resize_nesw: &'static [u8],
    pub(crate) resize_nwse: &'static [u8],
    pub(crate) text: &'static [u8],
    pub(crate) ring: [&'static [u8]; RING_FRAMES],
}

/// The light pointer with a dark outline.
pub(crate) const WHITE: CursorSet = CursorSet {
    name: "white",
    arrow: generated::WHITE_ARROW,
    resize_ew: generated::WHITE_RESIZE_EW,
    resize_ns: generated::WHITE_RESIZE_NS,
    resize_nesw: generated::WHITE_RESIZE_NESW,
    resize_nwse: generated::WHITE_RESIZE_NWSE,
    text: generated::WHITE_TEXT,
    ring: generated::WHITE_RING,
};

/// The dark pointer with a light outline.
pub(crate) const BLACK: CursorSet = CursorSet {
    name: "black",
    arrow: generated::BLACK_ARROW,
    resize_ew: generated::BLACK_RESIZE_EW,
    resize_ns: generated::BLACK_RESIZE_NS,
    resize_nesw: generated::BLACK_RESIZE_NESW,
    resize_nwse: generated::BLACK_RESIZE_NWSE,
    text: generated::BLACK_TEXT,
    ring: generated::BLACK_RING,
};

/// The set a theme shows: the black pointer on a light theme, the white one on a dark theme.
pub(crate) const fn for_light(light: bool) -> &'static CursorSet {
    if light {
        &BLACK
    } else {
        &WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprites(set: &CursorSet) -> impl Iterator<Item = &'static [u8]> {
        [set.arrow, set.resize_ew, set.resize_ns, set.resize_nesw, set.resize_nwse, set.text]
            .into_iter()
            .chain(set.ring)
    }

    /// The pixel at (x, y) of a sprite: (b, g, r, a).
    fn px(s: &[u8], x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * DIM + x) * 4) as usize;
        (s[i], s[i + 1], s[i + 2], s[i + 3])
    }

    #[test]
    fn a_light_theme_shows_the_black_pointer_and_a_dark_theme_the_white_one() {
        assert_eq!(for_light(true).name, "black");
        assert_eq!(for_light(false).name, "white");
    }

    #[test]
    fn every_sprite_is_a_full_square_with_its_hotspot_inside() {
        for set in [&WHITE, &BLACK] {
            for s in sprites(set) {
                assert_eq!(s.len(), (DIM * DIM * 4) as usize, "{}", set.name);
                assert!(s.chunks(4).any(|p| p[3] > 0), "{}: an empty sprite", set.name);
            }
        }
        let dim = DIM as i32;
        assert!(ARROW_HOT.0 < dim && ARROW_HOT.1 < dim && CENTER_HOT < dim);
        assert_eq!(WHITE.ring.len(), RING_FRAMES);
    }

    #[test]
    fn the_two_sets_differ_where_the_pointer_body_is() {
        // Inside the arrow's body, well clear of its outline.
        let (wb, wg, wr, wa) = px(WHITE.arrow, 6, 12);
        let (bb, bg, br, ba) = px(BLACK.arrow, 6, 12);
        assert!(wa == 255 && ba == 255, "the body is opaque in both sets");
        assert!(wb > 200 && wg > 200 && wr > 200, "white: a light body");
        assert!(bb < 60 && bg < 60 && br < 60, "black: a dark body");
    }
}
