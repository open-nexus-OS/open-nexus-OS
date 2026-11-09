// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp`: the focused field's caret, declared to windowd (RFC-0113 §5, ADR-0090 —
//! the compositor owns the caret). windowd draws and blinks it as a layer: the app wakes for no
//! blink and paints no caret into its raster. Sent after a present or a motion when it changed —
//! a moved caret restarts the blink (solid first); hidden while a selection shows (the
//! highlight says where the edit lands), when no field holds focus, or when the field scrolled out
//! of its view. Same geometry as the selection painter: the run's prefix width at the caret, a
//! 2-px bar inset 2 px from the run's box.

use super::super::*;
use nexus_display_proto::surface_caret::{
    encode_surface_caret, Caret, CARET_BLINK, CARET_RESET, CARET_VISIBLE,
};

impl DslApp {
    /// The caret the focused field shows now, in surface pixels — `None` for none.
    fn caret_now(&self) -> Option<Caret> {
        let f = self.view.text_focus()?;
        if f.selection.is_some() {
            return None;
        }
        let mut idx = 0usize;
        let (nid, content, font, tracking, color) =
            super::super::paint::caret_input(self.view.scene(), &mut idx, f.box_id, false)?;
        let run = self.layout.boxes.iter().find(|b| b.node_id == nid)?;
        let (rx, ry) = self.on_surface(run);
        let caret_x =
            nexus_text_baked::measure(content.chars().take(f.caret), font, tracking) as i32;
        // At the field's horizontal offset (TASK-0346 P2), inside the field's box.
        let scroll = if self.field_scroll.0 == f.session { self.field_scroll.1 } else { 0 };
        let (mut x0, mut y0) = (rx + caret_x - scroll + 1, ry + 2);
        let (mut x1, mut y1) = (x0 + 2, ry + run.rect.height.0 - 2);
        (x0, x1) = (x0.max(rx), x1.min(rx + run.rect.width.0));
        // Inside its scroll view, if it lives in one (a field scrolled away shows no caret).
        let (views, nv) = self.hit_views();
        if let Some(v) = run.scroll_root.and_then(|r| views[..nv].iter().find(|v| v.root == r)) {
            let (vx0, vy0, vx1, vy1) = v.viewport;
            (x0, y0, x1, y1) = (x0.max(vx0), y0.max(vy0), x1.min(vx1), y1.min(vy1));
        }
        if x1 <= x0 || y1 <= y0 || x0 < 0 || y0 < 0 {
            return None;
        }
        let narrow = |v: i32| u16::try_from(v).ok();
        Some(Caret {
            x: narrow(x0)?,
            y: narrow(y0)?,
            w: narrow(x1 - x0)?,
            h: narrow(y1 - y0)?.min(nexus_display_proto::surface_caret::CARET_H_MAX),
            bgra: [color[0], color[1], color[2], 255],
            flags: CARET_VISIBLE | CARET_BLINK,
        })
    }

    /// The caret to windowd when it changed since the last declaration (a move restarts its
    /// blink). Non-blocking: the next change re-declares it.
    pub(crate) fn sync_caret(&mut self, client: &KernelClient, surface_id: u32) {
        let caret = self.caret_now().unwrap_or(Caret::HIDDEN);
        if caret == self.sent_caret {
            return;
        }
        let rect = |c: &Caret| (c.x, c.y, c.w, c.h);
        let mut declared = caret;
        if caret.visible() && rect(&caret) != rect(&self.sent_caret) {
            declared.flags |= CARET_RESET;
        }
        if let Some(frame) = encode_surface_caret(surface_id, &declared) {
            if client.send(&frame, Wait::NonBlocking).is_ok() {
                self.sent_caret = caret;
            }
        }
    }
}
