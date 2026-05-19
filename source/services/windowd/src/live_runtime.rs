// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared live-path helpers for the visible `windowd` runtime.
//! OWNERS: @ui
//! STATUS: Experimental
//! TEST_COVERAGE: Host unit tests in this module

use nexus_layout::{LayoutResult, ScrollDamage};

const PANEL_BAND_ROWS: usize = crate::proof_panel_spec::PANEL_HEIGHT as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlassQuality {
    High,
    Low,
    Opaque,
}

impl GlassQuality {
    pub(crate) const fn blur_radius(self) -> u32 {
        match self {
            Self::High => 3,
            Self::Low => 1,
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
        Self {
            x,
            y,
            width: end_x.saturating_sub(x),
            height: end_y.saturating_sub(y),
        }
    }
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
        let band_end_y = base_y
            .saturating_add(crate::proof_panel_spec::PANEL_HEIGHT as u32)
            .min(mode_height);
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

        for (box_idx, layout_box) in layout.boxes.iter().enumerate() {
            let Some((start_y, end_y)) = layout_box_row_range(layout_box, base_y, mode_height) else {
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
                    filter_panel_rows = merge_row_range(filter_panel_rows, Some((start_y, end_y)))
                }
                Some("filter_list") => {
                    filter_list_rows = merge_row_range(filter_list_rows, Some((start_y, end_y)))
                }
                Some("filter_text_input") => {
                    filter_input_rows = merge_row_range(filter_input_rows, Some((start_y, end_y)))
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
            TargetDamage::FilterPanel | TargetDamage::FilterList | TargetDamage::FilterInput => None,
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
        let end_y = start_y
            .saturating_add(rect.height.as_u32().unwrap_or(0))
            .min(mode_height);
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
    (x < end_x && y < end_y).then_some(DamageRect {
        x,
        y,
        width: end_x - x,
        height: end_y - y,
    })
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
    }

    #[test]
    fn layout_hot_path_indexes_target_rows() {
        let layout = crate::layout_panel::compute_proof_layout(VisibleState::default(), "")
            .expect("proof layout");
        let index = LayoutHotPathIndex::build(&layout, 56, 440, 1280, 800);
        let hover_rows = index.target_rows(TargetDamage::Hover).expect("hover rows");
        let hover_rect = index.target_rect(TargetDamage::Hover).expect("hover rect");
        let filter_rows = index.target_rows(TargetDamage::FilterList).expect("filter rows");
        assert!(hover_rows.0 >= 440);
        assert!(hover_rows.1 > hover_rows.0);
        assert_eq!(hover_rows, (hover_rect.y, hover_rect.end_y()));
        assert!(hover_rect.x >= 56);
        assert!(hover_rect.width > 0);
        assert!(filter_rows.1 > filter_rows.0);
        assert_ne!(index.row_mask(hover_rows.0), 0);
        assert_eq!(index.row_mask(100), 0);
        assert!(!index.overflow_boxes());
    }

    #[test]
    fn damage_rect_merge_bounds_small_target_updates() {
        let left = DamageRect { x: 10, y: 20, width: 30, height: 40 };
        let right = DamageRect { x: 32, y: 10, width: 20, height: 12 };
        assert_eq!(
            left.merge(right),
            DamageRect { x: 10, y: 10, width: 42, height: 50 }
        );
    }

    #[test]
    fn target_rect_write_budget_stays_below_full_width_row_span() {
        let layout = crate::layout_panel::compute_proof_layout(VisibleState::default(), "")
            .expect("proof layout");
        let index = LayoutHotPathIndex::build(&layout, 56, 440, 1280, 800);
        let hover_rows = index.target_rows(TargetDamage::Hover).expect("hover rows");
        let hover_rect = index.target_rect(TargetDamage::Hover).expect("hover rect");

        let full_row_bytes = row_span_write_bytes(hover_rows, 1280 * 4);
        let rect_bytes = damage_rect_write_bytes(hover_rect);
        assert!(rect_bytes < full_row_bytes / 4);
    }
}
