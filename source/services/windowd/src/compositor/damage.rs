// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Damage-rect helpers for the windowd compositor: cursor damage, effect inflation,
//! intersection tests, and flush error labels.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests (cursor_damage_rect)

use super::types::ProofBoxRect;
use super::{SOFT_PANEL_SHADOW_BLUR_RADIUS, SOFT_PANEL_SHADOW_OFFSET_Y};
use crate::assets;
use crate::error::WindowdError;
use crate::smoke::VisibleBootstrapMode;
use nexus_layout::{LayoutResult, ScrollDamage};

pub(crate) fn cursor_damage_rect(
    cursor_x: i32,
    cursor_y: i32,
    cursor_width: u32,
    cursor_height: u32,
    mode_width: u32,
    mode_height: u32,
) -> Option<DamageRect> {
    if cursor_width == 0 || cursor_height == 0 || mode_width == 0 || mode_height == 0 {
        return None;
    }
    // Conservative hotspot pad: shapes place the hotspot anywhere up to the
    // sprite center (TASK-0070 Phase 3 resize shapes are 16,16; the default
    // pointer 2,2). One worst-case rect avoids threading live shape state here.
    let x0 = cursor_x.saturating_sub(assets::CURSOR_RESIZE_HOTSPOT);
    let y0 = cursor_y.saturating_sub(assets::CURSOR_RESIZE_HOTSPOT);
    let x1 = x0.saturating_add(cursor_width as i32);
    let y1 = y0.saturating_add(cursor_height as i32);
    let start_x = x0.max(0).min(mode_width as i32) as u32;
    let start_y = y0.max(0).min(mode_height as i32) as u32;
    let end_x = x1.max(0).min(mode_width as i32) as u32;
    let end_y = y1.max(0).min(mode_height as i32) as u32;
    if end_x <= start_x || end_y <= start_y {
        return None;
    }
    Some(DamageRect { x: start_x, y: start_y, width: end_x - start_x, height: end_y - start_y })
}

pub(crate) fn inflate_effect_rect(rect: ProofBoxRect, mode: VisibleBootstrapMode) -> DamageRect {
    let pad = SOFT_PANEL_SHADOW_BLUR_RADIUS
        .saturating_add(SOFT_PANEL_SHADOW_OFFSET_Y.unsigned_abs())
        .saturating_add(2);
    let x = rect.x.saturating_sub(pad);
    let y = rect.y.saturating_sub(pad);
    let end_x = rect.x.saturating_add(rect.width).saturating_add(pad).min(mode.width);
    let end_y = rect.y.saturating_add(rect.height).saturating_add(pad).min(mode.height);
    DamageRect { x, y, width: end_x.saturating_sub(x), height: end_y.saturating_sub(y) }
}

pub(crate) fn damage_rects_intersect(a: DamageRect, b: DamageRect) -> bool {
    a.x < b.end_x() && b.x < a.end_x() && a.y < b.end_y() && b.y < a.end_y()
}

pub(crate) fn flush_error_label(err: WindowdError) -> &'static str {
    match err {
        WindowdError::BufferLengthMismatch => "windowd: flush rows fail buffer-len",
        WindowdError::ArithmeticOverflow => "windowd: flush rows fail arith",
        _ => "windowd: flush rows fail",
    }
}

// ── Re-homed from the deleted `live_runtime.rs` (right-place move): the
// damage-rect model + premerge + the layout hot-path row index — pure
// damage/row accounting, i.e. compositor-service concerns.
const PANEL_BAND_ROWS: usize = 260; // C1: was proof_panel_spec::PANEL_HEIGHT
const DAMAGE_MERGE_AREA_PERCENT: u64 = 125;
const DAMAGE_MERGE_NEAR_GAP: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlassQuality {
    High,
    Low,
    Opaque,
}

impl GlassQuality {
    pub(crate) const fn blur_radius(self) -> u32 {
        match self {
            Self::High => 20,
            Self::Low => 8,
            Self::Opaque => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetDamage {
    Hover,
    Click,
    Scroll,
    Key,
    FilterPanel,
    FilterList,
    FilterInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DamageRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl DamageRect {
    pub(crate) const fn end_x(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub(crate) const fn end_y(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let end_x = self.end_x().max(other.end_x());
        let end_y = self.end_y().max(other.end_y());
        Self { x, y, width: end_x.saturating_sub(x), height: end_y.saturating_sub(y) }
    }
}

fn damage_area(rect: DamageRect) -> u64 {
    u64::from(rect.width).saturating_mul(u64::from(rect.height))
}

#[allow(clippy::implicit_saturating_sub)]
fn damage_gap(a_start: u32, a_end: u32, b_start: u32, b_end: u32) -> u32 {
    if a_end < b_start {
        b_start - a_end
    } else if b_end < a_start {
        a_start - b_end
    } else {
        0
    }
}

fn should_premerge_damage(a: DamageRect, b: DamageRect) -> bool {
    let x_gap = damage_gap(a.x, a.end_x(), b.x, b.end_x());
    let y_gap = damage_gap(a.y, a.end_y(), b.y, b.end_y());
    let spatially_close = (x_gap == 0 && y_gap <= DAMAGE_MERGE_NEAR_GAP)
        || (y_gap == 0 && x_gap <= DAMAGE_MERGE_NEAR_GAP);
    if !spatially_close {
        return false;
    }
    let merged = a.merge(b);
    let merged_area = damage_area(merged);
    let source_area = damage_area(a).saturating_add(damage_area(b));
    merged_area.saturating_mul(100) <= source_area.saturating_mul(DAMAGE_MERGE_AREA_PERCENT).max(1)
}

pub(crate) fn premerge_damage_rects(rects: &mut [DamageRect], mut count: usize) -> usize {
    let mut idx = 0;
    while idx < count {
        let mut scan = idx + 1;
        while scan < count {
            if should_premerge_damage(rects[idx], rects[scan]) {
                rects[idx] = rects[idx].merge(rects[scan]);
                rects[scan] = rects[count - 1];
                count -= 1;
            } else {
                scan += 1;
            }
        }
        idx += 1;
    }
    count
}

#[derive(Debug, Clone)]
pub(crate) struct LayoutHotPathIndex {
    band_start_y: u32,
    band_end_y: u32,
    row_masks: [u64; PANEL_BAND_ROWS],
    row_has_shadow: [bool; PANEL_BAND_ROWS],
    overflow_boxes: bool,
    hover_rows: Option<(u32, u32)>,
    click_rows: Option<(u32, u32)>,
    scroll_rows: Option<(u32, u32)>,
    key_rows: Option<(u32, u32)>,
    filter_panel_rows: Option<(u32, u32)>,
    filter_list_rows: Option<(u32, u32)>,
    filter_input_rows: Option<(u32, u32)>,
    hover_rect: Option<DamageRect>,
    click_rect: Option<DamageRect>,
    scroll_rect: Option<DamageRect>,
    key_rect: Option<DamageRect>,
    filter_panel_rect: Option<DamageRect>,
    filter_list_rect: Option<DamageRect>,
    filter_input_rect: Option<DamageRect>,
}

impl Default for LayoutHotPathIndex {
    fn default() -> Self {
        Self {
            band_start_y: 0,
            band_end_y: 0,
            row_masks: [0; PANEL_BAND_ROWS],
            row_has_shadow: [false; PANEL_BAND_ROWS],
            overflow_boxes: false,
            hover_rows: None,
            click_rows: None,
            scroll_rows: None,
            key_rows: None,
            filter_panel_rows: None,
            filter_list_rows: None,
            filter_input_rows: None,
            hover_rect: None,
            click_rect: None,
            scroll_rect: None,
            key_rect: None,
            filter_panel_rect: None,
            filter_list_rect: None,
            filter_input_rect: None,
        }
    }
}

impl LayoutHotPathIndex {
    pub(crate) fn build(
        layout: &LayoutResult,
        base_x: u32,
        base_y: u32,
        mode_width: u32,
        mode_height: u32,
    ) -> Self {
        let band_start_y = base_y.min(mode_height);
        let band_end_y = base_y.saturating_add(260).min(mode_height);
        let mut row_masks = [0u64; PANEL_BAND_ROWS];
        let mut row_has_shadow = [false; PANEL_BAND_ROWS];
        let mut overflow_boxes = false;
        let mut hover_rows = None;
        let mut click_rows = None;
        let mut scroll_rows = None;
        let mut key_rows = None;
        let mut filter_panel_rows = None;
        let mut filter_list_rows = None;
        let mut filter_input_rows = None;
        let mut hover_rect = None;
        let mut click_rect = None;
        let mut scroll_rect = None;
        let mut key_rect = None;
        let mut filter_panel_rect = None;
        let mut filter_list_rect = None;
        let mut filter_input_rect = None;

        for (box_idx, layout_box) in layout.boxes.iter().enumerate() {
            let Some((start_y, end_y)) = layout_box_row_range(layout_box, base_y, mode_height)
            else {
                continue;
            };
            let rect = layout_box_damage_rect(layout_box, base_x, base_y, mode_width, mode_height);
            for y in start_y.max(band_start_y)..end_y.min(band_end_y) {
                let row_idx = (y - band_start_y) as usize;
                if box_idx < u64::BITS as usize {
                    row_masks[row_idx] |= 1u64 << box_idx;
                } else {
                    overflow_boxes = true;
                }
                if layout_box.visual.shadow.is_some() {
                    row_has_shadow[row_idx] = true;
                }
            }
            match layout_box.id {
                Some("card_hover") => {
                    hover_rows = merge_row_range(hover_rows, Some((start_y, end_y)));
                    hover_rect = merge_damage_rect(hover_rect, rect);
                }
                Some("card_click") => {
                    click_rows = merge_row_range(click_rows, Some((start_y, end_y)));
                    click_rect = merge_damage_rect(click_rect, rect);
                }
                Some("card_scroll") => {
                    scroll_rows = merge_row_range(scroll_rows, Some((start_y, end_y)));
                    scroll_rect = merge_damage_rect(scroll_rect, rect);
                }
                Some("card_key") => {
                    key_rows = merge_row_range(key_rows, Some((start_y, end_y)));
                    key_rect = merge_damage_rect(key_rect, rect);
                }
                Some("filter_panel") => {
                    filter_panel_rows = merge_row_range(filter_panel_rows, Some((start_y, end_y)));
                    filter_panel_rect = merge_damage_rect(filter_panel_rect, rect);
                }
                Some("filter_list") => {
                    filter_list_rows = merge_row_range(filter_list_rows, Some((start_y, end_y)));
                    filter_list_rect = merge_damage_rect(filter_list_rect, rect);
                }
                Some("filter_text_input") => {
                    filter_input_rows = merge_row_range(filter_input_rows, Some((start_y, end_y)));
                    filter_input_rect = merge_damage_rect(filter_input_rect, rect);
                }
                _ => {}
            }
        }

        Self {
            band_start_y,
            band_end_y,
            row_masks,
            row_has_shadow,
            overflow_boxes,
            hover_rows,
            click_rows,
            scroll_rows,
            key_rows,
            filter_panel_rows,
            filter_list_rows,
            filter_input_rows,
            hover_rect,
            click_rect,
            scroll_rect,
            key_rect,
            filter_panel_rect,
            filter_list_rect,
            filter_input_rect,
        }
    }

    pub(crate) fn row_mask(&self, y: u32) -> u64 {
        if y < self.band_start_y || y >= self.band_end_y {
            return 0;
        }
        self.row_masks[(y - self.band_start_y) as usize]
    }

    pub(crate) fn row_has_shadow(&self, y: u32) -> bool {
        if y < self.band_start_y || y >= self.band_end_y {
            return false;
        }
        self.row_has_shadow[(y - self.band_start_y) as usize]
    }

    pub(crate) const fn overflow_boxes(&self) -> bool {
        self.overflow_boxes
    }

    pub(crate) fn target_rows(&self, target: TargetDamage) -> Option<(u32, u32)> {
        match target {
            TargetDamage::Hover => self.hover_rows,
            TargetDamage::Click => self.click_rows,
            TargetDamage::Scroll => self.scroll_rows,
            TargetDamage::Key => self.key_rows,
            TargetDamage::FilterPanel => self.filter_panel_rows,
            TargetDamage::FilterList => self.filter_list_rows,
            TargetDamage::FilterInput => self.filter_input_rows,
        }
    }

    pub(crate) fn target_rect(&self, target: TargetDamage) -> Option<DamageRect> {
        match target {
            TargetDamage::Hover => self.hover_rect,
            TargetDamage::Click => self.click_rect,
            TargetDamage::Scroll => self.scroll_rect,
            TargetDamage::Key => self.key_rect,
            TargetDamage::FilterPanel => self.filter_panel_rect,
            TargetDamage::FilterList => self.filter_list_rect,
            TargetDamage::FilterInput => self.filter_input_rect,
        }
    }
}

pub(crate) fn select_glass_quality(dirty_span_rows: u32) -> GlassQuality {
    if dirty_span_rows >= 320 {
        GlassQuality::Opaque
    } else if dirty_span_rows >= 160 {
        GlassQuality::Low
    } else {
        GlassQuality::High
    }
}

pub(crate) fn merge_row_range(
    current: Option<(u32, u32)>,
    incoming: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    match (current, incoming) {
        (Some((current_start, current_end)), Some((incoming_start, incoming_end))) => {
            Some((current_start.min(incoming_start), current_end.max(incoming_end)))
        }
        (Some(range), None) | (None, Some(range)) => Some(range),
        (None, None) => None,
    }
}

pub(crate) fn merge_damage_rect(
    current: Option<DamageRect>,
    incoming: Option<DamageRect>,
) -> Option<DamageRect> {
    match (current, incoming) {
        (Some(current), Some(incoming)) => Some(current.merge(incoming)),
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (None, None) => None,
    }
}

pub(crate) fn row_span_write_bytes(rows: (u32, u32), stride: u32) -> usize {
    rows.1.saturating_sub(rows.0) as usize * stride as usize
}

pub(crate) fn damage_rect_write_bytes(rect: DamageRect) -> usize {
    rect.width as usize * rect.height as usize * 4
}

pub(crate) fn scroll_damage_rows(
    damage: ScrollDamage,
    base_y: u32,
    mode_height: u32,
) -> Option<(u32, u32)> {
    let mut rows = None;
    for rect in damage.rects.into_iter().flatten() {
        let start_y = base_y.saturating_add(rect.y.as_u32().unwrap_or(0)).min(mode_height);
        let end_y = start_y.saturating_add(rect.height.as_u32().unwrap_or(0)).min(mode_height);
        rows = merge_row_range(rows, if start_y < end_y { Some((start_y, end_y)) } else { None });
    }
    rows
}

fn layout_box_row_range(
    layout_box: &nexus_layout::LayoutBox,
    base_y: u32,
    mode_height: u32,
) -> Option<(u32, u32)> {
    let height = layout_box.rect.height.as_u32().unwrap_or(0);
    if height == 0 {
        return None;
    }
    let mut start_y = base_y.saturating_add(layout_box.rect.y.as_u32().unwrap_or(0));
    let mut end_y = start_y.saturating_add(height);
    if let Some(clip_rect) = layout_box.clip_rect {
        let clip_start = base_y.saturating_add(clip_rect.y.as_u32().unwrap_or(0));
        let clip_end = clip_start.saturating_add(clip_rect.height.as_u32().unwrap_or(0));
        start_y = start_y.max(clip_start);
        end_y = end_y.min(clip_end);
    }
    let start_y = start_y.min(mode_height);
    let end_y = end_y.min(mode_height);
    (start_y < end_y).then_some((start_y, end_y))
}

fn layout_box_damage_rect(
    layout_box: &nexus_layout::LayoutBox,
    base_x: u32,
    base_y: u32,
    mode_width: u32,
    mode_height: u32,
) -> Option<DamageRect> {
    let width = layout_box.rect.width.as_u32().unwrap_or(0);
    let height = layout_box.rect.height.as_u32().unwrap_or(0);
    if width == 0 || height == 0 {
        return None;
    }
    let mut x = base_x.saturating_add(layout_box.rect.x.as_u32().unwrap_or(0));
    let mut y = base_y.saturating_add(layout_box.rect.y.as_u32().unwrap_or(0));
    let mut end_x = x.saturating_add(width);
    let mut end_y = y.saturating_add(height);
    if let Some(clip_rect) = layout_box.clip_rect {
        let clip_x = base_x.saturating_add(clip_rect.x.as_u32().unwrap_or(0));
        let clip_y = base_y.saturating_add(clip_rect.y.as_u32().unwrap_or(0));
        let clip_end_x = clip_x.saturating_add(clip_rect.width.as_u32().unwrap_or(0));
        let clip_end_y = clip_y.saturating_add(clip_rect.height.as_u32().unwrap_or(0));
        x = x.max(clip_x);
        y = y.max(clip_y);
        end_x = end_x.min(clip_end_x);
        end_y = end_y.min(clip_end_y);
    }
    x = x.min(mode_width);
    y = y.min(mode_height);
    end_x = end_x.min(mode_width);
    end_y = end_y.min(mode_height);
    (x < end_x && y < end_y).then_some(DamageRect { x, y, width: end_x - x, height: end_y - y })
}

#[cfg(test)]
mod tests {
    use super::*;

    use input_live_protocol::VisibleState;
    use nexus_layout_types::{FxPx, Rect};

    #[test]
    fn scroll_damage_rows_merge_both_rects() {
        let damage = ScrollDamage {
            rects: [
                Some(Rect::new(FxPx::new(0), FxPx::new(10), FxPx::new(40), FxPx::new(18))),
                Some(Rect::new(FxPx::new(0), FxPx::new(58), FxPx::new(40), FxPx::new(12))),
            ],
        };
        assert_eq!(scroll_damage_rows(damage, 440, 800), Some((450, 510)));
    }

    #[test]
    fn glass_quality_degrades_deterministically() {
        assert_eq!(select_glass_quality(32), GlassQuality::High);
        assert_eq!(select_glass_quality(160), GlassQuality::Low);
        assert_eq!(select_glass_quality(400), GlassQuality::Opaque);
        assert_eq!(GlassQuality::High.blur_radius(), 20);
        assert_eq!(GlassQuality::Low.blur_radius(), 8);
        assert_eq!(GlassQuality::Opaque.blur_radius(), 0);
    }

    #[test]
    fn damage_rect_merge_bounds_small_target_updates() {
        let left = DamageRect { x: 10, y: 20, width: 30, height: 40 };
        let right = DamageRect { x: 32, y: 10, width: 20, height: 12 };
        assert_eq!(left.merge(right), DamageRect { x: 10, y: 10, width: 42, height: 50 });
    }

    #[test]
    fn damage_premerge_merges_only_bounded_area_growth() {
        let mut rects = [
            DamageRect { x: 10, y: 10, width: 20, height: 20 },
            DamageRect { x: 25, y: 10, width: 20, height: 20 },
            DamageRect { x: 400, y: 400, width: 20, height: 20 },
        ];
        let count = premerge_damage_rects(&mut rects, 3);
        assert_eq!(count, 2);
        assert!(rects[..count].iter().any(|rect| rect.width == 35 && rect.height == 20));
        assert!(rects[..count].iter().any(|rect| rect.x == 400 && rect.y == 400));
    }
}
