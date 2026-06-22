// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Search app surface rendering — the app composes its **own** surface buffer.
//!
//! The app renders its content (filter field + result rows) into a BGRA buffer it
//! owns; windowd hosts that buffer as a per-app layer (ADR-0037) and wraps it in
//! the window chrome. This is CPU pixel composition with no windowd dependency, so
//! it is fully host-tested. Text glyphs are a follow-up; rows are laid out as
//! distinct bands so layout + state (results vs. empty) are verifiable now.

use crate::model;

/// BGRA pixel (windowd surface byte order).
type Bgra = [u8; 4];

const BG: Bgra = [0x2a, 0x28, 0x24, 0xeb]; // dark glass body
const FIELD: Bgra = [0x42, 0x40, 0x3a, 0xff]; // filter input field
const ROW_EVEN: Bgra = [0x34, 0x32, 0x2e, 0xff];
const ROW_ODD: Bgra = [0x2e, 0x2c, 0x28, 0xff];
const ROW_TEXT: Bgra = [0xd0, 0xd0, 0xd8, 0xff]; // result "ink" marker bar

/// A surface buffer the app owns (its own VMO content on the OS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSurface {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// BGRA pixels, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl OwnedSurface {
    /// The BGRA pixel at `(x, y)`, or `None` if out of bounds.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Bgra> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some([self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]])
    }
}

fn fill_rect(buf: &mut [u8], stride: u32, x: u32, y: u32, w: u32, h: u32, c: Bgra) {
    for yy in y..y + h {
        let row = (yy * stride) as usize;
        for xx in x..x + w {
            let i = row + (xx * 4) as usize;
            if i + 4 <= buf.len() {
                buf[i..i + 4].copy_from_slice(&c);
            }
        }
    }
}

/// Renders the search content surface for `filter_text` at row `scroll` offset.
/// The app owns this composition end to end.
pub fn render(filter_text: &str) -> OwnedSurface {
    render_scrolled(filter_text, 0)
}

/// As [`render`], with a vertical scroll offset (in rows) into the filtered list.
pub fn render_scrolled(filter_text: &str, scroll: u32) -> OwnedSurface {
    let width = model::SEARCH_W;
    let height = model::content_height();
    let stride = width * 4;
    let mut pixels = vec![0u8; (stride * height) as usize];

    // Body.
    fill_rect(&mut pixels, stride, 0, 0, width, height, BG);

    // Filter field.
    let field_x = model::SEARCH_PAD;
    let field_y = model::SEARCH_PAD;
    let field_w = width - 2 * model::SEARCH_PAD;
    fill_rect(&mut pixels, stride, field_x, field_y, field_w, model::FILTER_H, FIELD);

    // Result rows (the app's filtered data, scrolled).
    let results = model::filter(filter_text);
    let scroll = scroll.min(model::max_scroll(results.len()));
    let list_top = field_y + model::FILTER_H + model::SEARCH_PAD;
    for visible in 0..model::VISIBLE_ROWS {
        let idx = scroll + visible;
        let ry = list_top + visible * model::ROW_H;
        let band = if visible % 2 == 0 { ROW_EVEN } else { ROW_ODD };
        fill_rect(&mut pixels, stride, model::SEARCH_PAD, ry, field_w, model::ROW_H, band);
        // A result present at this row gets an "ink" marker bar (stand-in for text).
        if (idx as usize) < results.len() {
            let word_len = results[idx as usize].len() as u32;
            let bar_w = (word_len * 6).min(field_w - 16);
            fill_rect(&mut pixels, stride, model::SEARCH_PAD + 8, ry + 10, bar_w, 6, ROW_TEXT);
        }
    }

    OwnedSurface { width, height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_has_expected_dimensions() {
        let s = render("");
        assert_eq!(s.width, model::SEARCH_W);
        assert_eq!(s.height, model::content_height());
        assert_eq!(s.pixels.len(), (s.width * s.height * 4) as usize);
    }

    #[test]
    fn filter_field_is_drawn() {
        let s = render("ap");
        // A pixel inside the filter field band differs from the body.
        let inside = s.pixel(model::SEARCH_PAD + 4, model::SEARCH_PAD + 4).unwrap();
        assert_eq!(inside, FIELD);
    }

    #[test]
    fn results_draw_ink_bars() {
        let s = render("ap"); // several matches
        let list_top = model::SEARCH_PAD + model::FILTER_H + model::SEARCH_PAD;
        // First row's ink bar pixel is the text color.
        let ink = s.pixel(model::SEARCH_PAD + 10, list_top + 10).unwrap();
        assert_eq!(ink, ROW_TEXT);
    }

    #[test]
    fn empty_state_has_no_ink_bars() {
        let s = render("zzzz"); // no matches
        let list_top = model::SEARCH_PAD + model::FILTER_H + model::SEARCH_PAD;
        // The row band exists but carries no ink bar (it is the band color).
        let px = s.pixel(model::SEARCH_PAD + 10, list_top + 10).unwrap();
        assert_ne!(px, ROW_TEXT);
        assert_eq!(px, ROW_EVEN);
    }

    #[test]
    fn scroll_is_clamped() {
        // Scrolling far past the end still renders a full, valid surface.
        let s = render_scrolled("a", 9999);
        assert_eq!(s.pixels.len(), (s.width * s.height * 4) as usize);
    }
}
