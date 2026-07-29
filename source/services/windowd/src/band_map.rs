// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: scene-rect → packed-band mapping for material glass regions on a
//! SCROLLABLE (banded) window. Pure geometry, host-tested — the scene encoder
//! only forwards its result.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below
//!
//! The WebRender packed band stores a scrollable window's pixels as
//! `[WM title + app header][app footer][tall content]` (see
//! `composite_scrollable_glass`). An app's material region (`LayerDesc`)
//! however names a rect in SCENE coordinates — the app's own layout space,
//! where the header is at the top, the content follows, and the footer sits at
//! the very bottom. Before RFC-0084 the scrollable composite simply skipped
//! every region ("scroll takes priority"), which is how a freeform file
//! manager lost all of its pane glass the moment its listing made the window
//! banded. This module is the missing translation.

/// The banded window's geometry, all in surface rows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandGeometry {
    /// WM title-bar rows painted into the band top by windowd (0 = plain).
    pub title_h: u32,
    /// The APP's fixed header rows (scene rows `[0, header_h)`).
    pub header_h: u32,
    /// The app's fixed footer rows (the scene's bottom `footer_h` rows).
    pub footer_h: u32,
    /// Total content rows in the band (the tall scrollable middle).
    pub content_h: u32,
    /// Current scroll offset in rows.
    pub scroll_rows: u32,
    /// The window frame height (title + visible rows).
    pub frame_h: u32,
}

/// One region translated to composite inputs: band source row, window-local
/// destination y, and the (possibly viewport-clipped) height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BandSlice {
    /// Source row INSIDE the band (add the band's `atlas_row`).
    pub src_row: u32,
    /// Destination y INSIDE the window frame (add the window's `y`).
    pub dst_y: u32,
    pub h: u32,
}

/// Maps a scene-space region `(y, h)` into the packed band — one slice, for
/// a region that does not cross a slice boundary. A straddling region is
/// CLAMPED to the slice its top row lives in; callers that need the whole
/// region use [`band_region_slices`], which splits at the boundaries first.
pub(crate) fn band_region_slice(y: u32, h: u32, g: &BandGeometry) -> Option<BandSlice> {
    if h == 0 || g.frame_h == 0 {
        return None;
    }
    let scene_h = g.header_h + g.content_h + g.footer_h;
    let footer_scene_y = scene_h.saturating_sub(g.footer_h);
    if y < g.header_h {
        // Fixed header: band rows follow the WM title rows 1:1.
        let h = h.min(g.header_h - y);
        return Some(BandSlice { src_row: g.title_h + y, dst_y: g.title_h + y, h });
    }
    if y >= footer_scene_y {
        // Fixed footer: packed right after the header block; shown at the
        // window bottom.
        let off = y - footer_scene_y;
        if off >= g.footer_h {
            return None;
        }
        let h = h.min(g.footer_h - off);
        let dst_y = g.frame_h.saturating_sub(g.footer_h) + off;
        return Some(BandSlice { src_row: g.title_h + g.header_h + off, dst_y, h });
    }
    // Scrollable content: band block starts after header+footer; on screen it
    // starts below the fixed top and is clipped to the viewport.
    let viewport_top = g.title_h + g.header_h;
    let viewport_h = g.frame_h.saturating_sub(viewport_top).saturating_sub(g.footer_h);
    if viewport_h == 0 {
        return None;
    }
    let content_y = y - g.header_h; // row within the tall content block
    let content_base = g.title_h + g.header_h + g.footer_h;
    // Visible content rows are [scroll_rows, scroll_rows + viewport_h).
    let vis_start = content_y.max(g.scroll_rows);
    let vis_end = (content_y + h).min(g.scroll_rows + viewport_h).min(g.content_h);
    if vis_end <= vis_start {
        return None;
    }
    Some(BandSlice {
        src_row: content_base + vis_start,
        dst_y: viewport_top + (vis_start - g.scroll_rows),
        h: vis_end - vis_start,
    })
}

/// Maps a scene-space region `(y, h)` into the packed band, SPLITTING it at
/// the slice boundaries — a straddler cannot be expressed as one contiguous
/// band read (the band reorders the slices), but it CAN be expressed as up to
/// three: its header part, its content part, its footer part. Writes into
/// `out` and returns how many slices are valid. A region whose pane spans
/// header AND content (a full-height side pane on a banded window) used to be
/// truncated at the header boundary here — the pane's glass simply stopped.
pub(crate) fn band_region_slices(
    y: u32,
    h: u32,
    g: &BandGeometry,
    out: &mut [BandSlice; 3],
) -> usize {
    if h == 0 {
        return 0;
    }
    let scene_h = g.header_h + g.content_h + g.footer_h;
    let footer_scene_y = scene_h.saturating_sub(g.footer_h);
    let (y0, y1) = (y, y.saturating_add(h));
    let mut n = 0;
    // Per-slice segments in scene space; each maps without straddling.
    let segments = [
        (y0, y1.min(g.header_h)),                     // header part
        (y0.max(g.header_h), y1.min(footer_scene_y)), // content part
        (y0.max(footer_scene_y), y1),                 // footer part
    ];
    for (s0, s1) in segments {
        if s1 <= s0 {
            continue;
        }
        if let Some(slice) = band_region_slice(s0, s1 - s0, g) {
            out[n] = slice;
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain (title-less) window: 40 header rows, 30 footer rows, 800
    /// content rows, 526-row frame → viewport of 456 rows.
    const G: BandGeometry = BandGeometry {
        title_h: 0,
        header_h: 40,
        footer_h: 30,
        content_h: 800,
        scroll_rows: 0,
        frame_h: 526,
    };

    #[test]
    fn a_header_region_maps_onto_the_band_top() {
        assert_eq!(band_region_slice(4, 30, &G), Some(BandSlice { src_row: 4, dst_y: 4, h: 30 }));
    }

    #[test]
    fn a_footer_region_maps_behind_the_header_block_and_shows_at_the_bottom() {
        // Scene footer starts at 40 + 800 = 840.
        assert_eq!(
            band_region_slice(842, 20, &G),
            Some(BandSlice { src_row: 42, dst_y: 526 - 30 + 2, h: 20 })
        );
    }

    #[test]
    fn a_content_pane_maps_into_the_tall_block_and_clips_to_the_viewport() {
        // A pane spanning the whole content (scene y 40, 800 rows tall):
        // src starts at the content block (40 + 30), dst right below the
        // header, height clipped to the 456-row viewport.
        assert_eq!(
            band_region_slice(40, 800, &G),
            Some(BandSlice { src_row: 70, dst_y: 40, h: 456 })
        );
    }

    #[test]
    fn scrolling_shifts_a_content_region_up_and_out() {
        let g = BandGeometry { scroll_rows: 100, ..G };
        // The full-height pane: still pinned to the viewport, sampling 100
        // rows further down the band.
        assert_eq!(
            band_region_slice(40, 800, &g),
            Some(BandSlice { src_row: 170, dst_y: 40, h: 456 })
        );
        // A short card at the content top is scrolled fully out.
        assert_eq!(band_region_slice(40, 80, &g), None);
    }

    #[test]
    fn a_chromed_window_offsets_by_its_title_rows() {
        let g = BandGeometry { title_h: 28, ..G };
        assert_eq!(band_region_slice(4, 30, &g), Some(BandSlice { src_row: 32, dst_y: 32, h: 30 }));
    }

    #[test]
    fn degenerate_geometry_yields_nothing() {
        assert_eq!(band_region_slice(0, 0, &G), None);
        let flat = BandGeometry { frame_h: 0, ..G };
        assert_eq!(band_region_slice(4, 30, &flat), None);
        let no_viewport = BandGeometry { frame_h: 60, ..G };
        assert_eq!(band_region_slice(50, 30, &no_viewport), None);
    }

    /// A full-height pane spanning header AND content AND footer: the split
    /// mapper yields all three parts where the single mapper used to truncate
    /// at the header boundary (the pane's glass just stopped there).
    #[test]
    fn a_straddling_region_splits_into_all_its_slices() {
        let mut out = [BandSlice { src_row: 0, dst_y: 0, h: 0 }; 3];
        // Scene y 4..870: 36 header rows + all 800 content rows + 30 footer.
        let n = band_region_slices(4, 866, &G, &mut out);
        assert_eq!(n, 3, "header, content and footer parts: {out:?}");
        assert_eq!(out[0], BandSlice { src_row: 4, dst_y: 4, h: 36 });
        assert_eq!(out[1], BandSlice { src_row: 70, dst_y: 40, h: 456 });
        assert_eq!(out[2], BandSlice { src_row: 40, dst_y: 526 - 30, h: 30 });
    }

    /// A region inside ONE slice behaves exactly like the single mapper.
    #[test]
    fn a_non_straddling_region_is_one_slice() {
        let mut out = [BandSlice { src_row: 0, dst_y: 0, h: 0 }; 3];
        let n = band_region_slices(4, 30, &G, &mut out);
        assert_eq!(n, 1);
        assert_eq!(Some(out[0]), band_region_slice(4, 30, &G));
    }
}
