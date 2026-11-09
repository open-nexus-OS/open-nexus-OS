// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp`: the pointer's caret in the focused field (TASK-0346 P2, RFC-0113 §4) — a
//! click places the caret where it landed, Shift+click and a drag extend the selection, a double
//! click takes the word, a triple click everything. The char index comes from the glyph metrics
//! the field is painted with (prefix widths of the DISPLAYED run — a masked password has the same
//! char count), so the caret lands between the two chars the person sees.

use super::super::*;
use nexus_display_proto::surface_input::{PointerFacts, MOD_SHIFT};

impl DslApp {
    /// The char index of the focused field's run nearest surface point `(x, y)` — `None` without
    /// focus or when the point is not on the field.
    fn caret_index_at(&self, x: i32, y: i32) -> Option<usize> {
        let f = self.view.text_focus()?;
        let mut idx = 0usize;
        let (nid, content, font, tracking, _) =
            super::super::paint::caret_input(self.view.scene(), &mut idx, f.box_id, false)?;
        let field = self.layout.boxes.iter().find(|b| b.node_id == f.box_id)?;
        let (fx, fy) = self.on_surface(field);
        let inside = |v: i32, at: i32, len: i32| v >= at && v < at + len;
        if !inside(x, fx, field.rect.width.0) || !inside(y, fy, field.rect.height.0) {
            return None;
        }
        let run = self.layout.boxes.iter().find(|b| b.node_id == nid)?;
        // At the field's horizontal offset (TASK-0346 P2): where the person sees the chars.
        let scroll = if self.field_scroll.0 == f.session { self.field_scroll.1 } else { 0 };
        let local = x - self.on_surface(run).0 + scroll;
        // The boundary nearest the point: before char `n` while the point is left of the middle.
        let mut best = (0usize, local.unsigned_abs());
        for n in 1..=content.chars().count() {
            let w = nexus_text_baked::measure(content.chars().take(n), font, tracking) as i32;
            let d = (local - w).unsigned_abs();
            if d < best.1 {
                best = (n, d);
            }
        }
        Some(best.0)
    }

    /// A press at `(x, y)` (the tap that focused the field, or a later one on it): the caret
    /// where it landed — Shift extends, two clicks take the word, three everything. Returns the
    /// repaint need; arms the drag selection when the press was on the field.
    pub(crate) fn press_text(&mut self, x: i32, y: i32, facts: PointerFacts) -> bool {
        let Some(idx) = self.caret_index_at(x, y) else {
            self.text_press = false;
            return false;
        };
        self.text_press = true;
        let result = if facts.clicks >= 2 {
            self.view.select_text_at(idx, facts.clicks)
        } else {
            self.view.place_caret(idx, facts.modifiers & MOD_SHIFT != 0)
        };
        result != nexus_dsl_runtime::EditResult::None
    }

    /// The pointer held after a press on the field: the selection follows it (the press's place
    /// is the anchor). Returns the repaint need; `None` when no field press is held.
    pub(crate) fn drag_text(&mut self, x: i32, y: i32) -> Option<bool> {
        if !self.text_press {
            return None;
        }
        // Beyond the field's box the caret goes to the nearest end of the run.
        let idx = self.caret_index_at(x, y).or_else(|| {
            let f = self.view.text_focus()?;
            let field = self.layout.boxes.iter().find(|b| b.node_id == f.box_id)?;
            let (fx, fy) = self.on_surface(field);
            let y = y.clamp(fy, fy + field.rect.height.0 - 1);
            let x = x.clamp(fx, fx + field.rect.width.0 - 1);
            self.caret_index_at(x, y)
        })?;
        Some(self.view.place_caret(idx, true) != nexus_dsl_runtime::EditResult::None)
    }

    /// The press let go: a later drag selects nothing.
    pub(crate) fn release_text(&mut self) {
        self.text_press = false;
    }
}
