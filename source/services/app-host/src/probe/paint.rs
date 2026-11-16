// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` paint subsystem: the ONE row painter (fills + glyph runs) behind the BASE
//! layer's rows and every scroll layer's band (RFC-0109). A pass picks its boxes once — the
//! base: every box but the content of an uncovered scroll layer, a covered layer's content at
//! its view; a band: one layer's content at identity — and paints row by row: the shared
//! painter (`nexus-scene-raster`), then the glyph pass with its caret and selection. Hover is no
//! paint any more: it is a wash layer the compositor draws (`probe/band.rs`). Every row leaves
//! in straight alpha — the base's, a band's, a motion layer's (RFC-0109; `straight_alpha.rs`).
//! Plus the handler-box diagnostic dump and the tap trace.

use super::surface_mem::SurfaceMem;
use super::*;
use crate::pass_plan::{CaretPaint, PassPlan};
use crate::straight_alpha::{premultiplied, unpremultiply};

mod collect;
mod split;
pub(in crate::probe) use collect::caret_input;
pub(super) use collect::collect_texts;
pub(super) use collect::dump_text_runs as collect_dump_text_runs;

/// The scroll views a pass places covered content at.
type Views<'a> = &'a [nexus_scene_raster::ScrollView];

/// What a pass's rows read beside the picked boxes, resolved once per pass: the animated nodes'
/// transforms and the focused field's caret.
#[derive(Clone, Copy)]
pub(super) struct PassCtx {
    anims: [nexus_scene_raster::NodeAnim; super::anim::MAX_EXPANDED_ANIMS],
    anim_n: usize,
    caret: Option<CaretPaint>,
}

impl PassCtx {
    pub(super) const EMPTY: Self = Self {
        anims: [nexus_scene_raster::NodeAnim::identity(0); super::anim::MAX_EXPANDED_ANIMS],
        anim_n: 0,
        caret: None,
    };

    fn anims(&self) -> &[nexus_scene_raster::NodeAnim] {
        &self.anims[..self.anim_n]
    }
}

/// What a paint pass draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// The base layer: every box but the content of an uncovered scroll layer and of a motion
    /// layer — a covered layer's content at its view.
    Base,
    /// Scroll layer `root`'s content at identity: its band.
    Band(usize),
    /// Motion layer `k`'s subtree at rest (TASK-0354 P2): its pixels in the store.
    Motion(usize),
}

impl super::DslApp {
    /// One-time diagnostic: where the interactive (handler) boxes are.
    ///
    /// States the TOTAL first and prints up to 32 boxes — a former silent
    /// `.take(8)` made a page with 40 handlers look like a page with 8 and
    /// sent a whole debugging round after the wrong hypothesis.
    pub(super) fn dump_handler_boxes(&self) {
        let handlers = self.view.handlers();
        raw_marker(&alloc::format!("apphost: {} handlers registered", handlers.len()));
        for (box_id, _) in handlers.iter().take(32) {
            match self.layout.boxes.iter().find(|b| b.node_id == *box_id) {
                Some(b) => raw_marker(&alloc::format!(
                    "apphost: handler box id={} x={} y={} w={} h={}",
                    box_id,
                    b.rect.x.as_i32(),
                    b.rect.y.as_i32(),
                    b.rect.width.as_i32(),
                    b.rect.height.as_i32()
                )),
                // A handler without a box is itself a finding (id drift,
                // node-cap bail) — say it instead of skipping it.
                None => raw_marker(&alloc::format!("apphost: handler box id={box_id} MISSING")),
            }
        }
        if handlers.len() > 32 {
            raw_marker(&alloc::format!("apphost: … {} more handlers", handlers.len() - 32));
        }
    }

    /// ONE line per tap: the whole input→output pipeline in a form that says
    /// WHERE it stops. `hit` = the node the hit-test chose (cross-reference
    /// `dump_text_runs`/`dump_handler_boxes`), `dmg` = what the dispatch
    /// reported, `txt` = the first text runs AFTER the relayout.
    ///
    /// Bounded so a tap storm cannot flood the log (32: the live lane's greeter + shell phases
    /// trace ~16 in one process). Never in the keyboard overlay, where a tap is a keystroke.
    /// Returns whether the tap was traced (it then owes a settle line).
    pub(super) fn trace_tap(
        &self,
        x: i32,
        y: i32,
        hit: Option<usize>,
        damage: Option<nexus_dsl_runtime::Damage>,
    ) -> bool {
        static TAPS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        if self.taps_are_keystrokes
            || TAPS.fetch_add(1, core::sync::atomic::Ordering::Relaxed) >= 32
        {
            return false;
        }
        let mut txt = alloc::string::String::new();
        for t in self.texts.iter().take(2) {
            let _ = core::fmt::write(&mut txt, format_args!("'{}' ", t.content));
        }
        raw_marker(&alloc::format!("apphost: tap ({x},{y}) hit={hit:?} dmg={damage:?} txt={txt}"));
        true
    }

    /// The page base's alpha: the surface's (`base_alpha` — 0 on the desktop, opaque for an
    /// overlay or a fullscreen window) — a floating window's is its material's fill, `glassWindow`
    /// at the token's own alpha (RFC-0116), opaque under solid materials (TASK-0337, reduce
    /// transparency).
    pub(super) fn page_base_alpha(&self) -> u8 {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        if wire::theme_is_solid(self.theme_mode) && self.base_alpha > 0 {
            255
        } else if self.base_alpha == 0 || self.base_alpha == 255 {
            self.base_alpha
        } else {
            tokens_for(self.theme_mode).color(ColorToken::GlassWindow).a
        }
    }

    /// The page base, BGRA premultiplied (RFC-0116): a floating window's is its material's
    /// fill (`glassWindow` at the window's frost), a fullscreen or overlay surface's the opaque
    /// `background`; the desktop's alpha is 0 (the wallpaper is its base). The row painter blends
    /// premultiplied and every row leaves straight (`straight_alpha`), so the base is the role's
    /// colour times its alpha — a transparent base is transparent black. Its colour bytes left
    /// standing bled into every translucent pixel drawn over it: the light desktop's 56 % black
    /// icons came out near-white and every glyph edge white-fringed (board cycle 2026-11-10).
    pub(super) fn page_base(&self) -> [u8; 4] {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let role =
            if self.base_alpha == 255 { ColorToken::Background } else { ColorToken::GlassWindow };
        premultiplied(tokens_for(self.theme_mode).color(role), self.page_base_alpha())
    }

    /// The whole base layer.
    pub(super) fn render(&mut self, mem: &mut SurfaceMem) -> bool {
        // Every dispatch path presents through here: mirror the modal depth to
        // windowd and keep the transient timer honest BEFORE the pixels (TASK-0074).
        self.overlay_sync();
        self.render_rows(mem, (0, self.h as i32), None)
    }

    /// Renders base rows `[y0, y1)` straight into the slot being drawn (RFC-0114) — the
    /// damage-limited path; with `cols`, only the columns `[c0, c1)` of them (TASK-0367 P3: the
    /// frame's damage rectangle — the rest of each row is the slot's, carried forward). The page
    /// base is [`Self::page_base`]; the scene's own boxes are specified against it.
    pub(super) fn render_rows(
        &mut self,
        mem: &mut SurfaceMem,
        (y0, y1): (i32, i32),
        cols: Option<(i32, i32)>,
    ) -> bool {
        self.overlay_sync();
        let base = self.page_base();
        let (y0, y1) = (y0.max(0), y1.min(self.h as i32));
        let cols = cols.filter(|&(c0, c1)| c0 > 0 || c1 < self.w as i32);
        if !self.split_done && y0 == 0 && y1 == self.h as i32 && cols.is_none() {
            return self.render_rows_split(mem); // once: where a row's time goes (TASK-0367 P0)
        }
        let t0 = nsec_now();
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        self.prepare_picks_with(Pass::Base, (y0, y1), cols, &views);
        let row_bytes = self.w as usize * 4;
        let mut ok = true;
        let plan = self.plan(&views);
        for y in y0..y1 {
            // The row in the buffer itself: painted where the GPU reads it, no copy; left in the
            // straight alpha the compositor reads (RFC-0109), like every row.
            let Some(row) = mem.row_mut(y as u32).and_then(|r| r.get_mut(..row_bytes)) else {
                ok = false;
                break;
            };
            match cols {
                Some(cols) => plan.paint_ground_cols(row, y, base, cols),
                None => plan.paint_ground_row(row, y, base),
            }
        }
        self.view_scratch = views;
        // A motion episode's raster cost (`apphost: hops motion paint_us= rows=`).
        self.motion_hops.painted(nsec_now().saturating_sub(t0) / 1000, (y1 - y0).max(0) as u32);
        ok
    }

    /// Rows `[y0, y1)` of motion layer `k` (TASK-0354 P2): its subtree at rest, painted where its
    /// pixels are in the store — on a clear ground, so the compositor drawing the layer over the
    /// base shows what painting it in place shows (P7: on the page base a window's frost was in
    /// the layer a second time). Picked over the layer's whole rest (a shadow's rows lie outside
    /// its box).
    pub(super) fn render_motion_rows(
        &mut self,
        mem: &mut SurfaceMem,
        k: usize,
        (y0, y1): (i32, i32),
    ) -> bool {
        let Some((rect, (sx, sy))) = self.motion_place(k) else { return true };
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        self.prepare_picks_with(Pass::Motion(k), (rect.1, rect.3), Some((rect.0, rect.2)), &views);
        let row_bytes = self.w as usize * 4;
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        let span = (rect.2 - rect.0).max(0) as usize * 4;
        let mut ok = true;
        for y in y0.max(rect.1)..y1.min(rect.3) {
            row[..row_bytes].fill(0);
            self.paint_row(&mut row[..row_bytes], y, rect.0, &views);
            unpremultiply(&mut row[..span]);
            let at = u32::from(sy) + (y - rect.1) as u32;
            if !mem.write(at, u32::from(sx), &row[..span]) {
                ok = false;
                break;
            }
        }
        self.row_scratch = row;
        self.view_scratch = views;
        ok
    }

    /// One row of a band: content row `model_y` (layout coordinates) painted from transparent at
    /// identity, the canvas shifted by `shift` (a horizontal band's run of columns), its columns
    /// `[cols.0, cols.1)` turned from the painter's premultiplied pixels into the straight alpha
    /// the compositor samples, and written at `(row, col)` of the slot being drawn. `false` past
    /// the slot.
    pub(super) fn paint_band_row(
        &mut self,
        mem: &mut SurfaceMem,
        model_y: i32,
        shift: i32,
        cols: (i32, i32),
        (row_at, col): (u32, u32),
        ground: Option<nexus_layout_types::Rgba8>,
    ) -> bool {
        let row_bytes = self.w as usize * 4;
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        // TASK-0351 P14: a band on a ground starts from the pane's opaque pixel (the painter's
        // premultiplied order b, g, r, a — an opaque pixel is its own premultiplication), so
        // the slice composes without a blend; else from transparent.
        match ground {
            Some(c) => {
                let ground = premultiplied(c, c.a);
                nexus_paint_kernels::fill(&mut row[..row_bytes], ground);
            }
            None => row[..row_bytes].fill(0),
        }
        self.paint_row(&mut row[..row_bytes], model_y, shift, &[]);
        let (c0, c1) =
            (cols.0.clamp(0, self.w as i32) as usize, cols.1.clamp(0, self.w as i32) as usize);
        let span = &mut row[c0 * 4..c1.max(c0) * 4];
        unpremultiply(span);
        let ok = mem.write(row_at, col, span);
        self.row_scratch = row;
        ok
    }

    /// The visibility index of a pass over layout rows `rows` (and, for a horizontal band, the
    /// columns `cols`): which boxes paint there, their animation slots, their text runs — once
    /// per pass, so a row costs what is on screen, not the page.
    pub(super) fn prepare_picks(&mut self, pass: Pass, rows: (i32, i32), cols: Option<(i32, i32)>) {
        self.prepare_picks_with(pass, rows, cols, &[]);
    }

    fn prepare_picks_with(
        &mut self,
        pass: Pass,
        (y0, y1): (i32, i32),
        cols: Option<(i32, i32)>,
        views: &[nexus_scene_raster::ScrollView],
    ) {
        // Snapshot the per-node animation transforms the DSL binding is driving this frame, and
        // the caret — once per pass, never per row.
        let mut ctx = PassCtx::EMPTY;
        ctx.anim_n = self.expand_node_anims(&mut ctx.anims);
        self.update_field_scroll();
        ctx.caret = self.caret_probe();
        let anims = ctx.anims();
        let mut vis_pick = core::mem::take(&mut self.vis_pick);
        let mut vis_anim = core::mem::take(&mut self.vis_anim);
        let mut vis_text = core::mem::take(&mut self.vis_text);
        vis_pick.clear();
        vis_anim.clear();
        vis_text.clear();
        for (bi, b) in self.layout.boxes.iter().enumerate() {
            if b.rect.height.0 <= 0 || b.rect.width.0 <= 0 {
                continue;
            }
            // Its painted extent: the box and the rows its own shadow reaches.
            let (_, st, _, sb) = nexus_scene_raster::shadow_reach(b);
            let (by, bh) = (b.rect.y.0 - st, b.rect.height.0 + st + sb);
            let (row0, row1) = match pass {
                Pass::Band(root) => {
                    if b.scroll_root != Some(root) {
                        continue;
                    }
                    (y0, y1)
                }
                Pass::Base | Pass::Motion(_) => {
                    // Drawn in its layer's band, or in a motion layer this pass is not.
                    let motion = match pass {
                        Pass::Motion(k) => Some(k),
                        _ => None,
                    };
                    if self.in_band(b)
                        || self.motion_of(b.node_id) != motion
                        || motion.is_some_and(|k| self.motion_skips(k, b.node_id))
                    {
                        continue;
                    }
                    // A covered layer's content: its rows on the surface are its layout rows
                    // moved by the view.
                    match b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r)) {
                        Some(v) => (y0.max(v.clip.1) + v.dy, y1.min(v.clip.3) + v.dy),
                        None => (y0, y1),
                    }
                }
            };
            if by >= row1 || by + bh <= row0 {
                continue;
            }
            if let Some(cols) = cols {
                // TASK-0367 P3: the pass paints only these columns — picked by what may write
                // there (`pass_plan::meets_cols`, host-tested with the painter).
                let dx = b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r));
                let animated = anims.iter().any(|a| a.node_id == b.node_id && !a.is_identity());
                if !crate::pass_plan::meets_cols(b, dx.map_or(0, |v| v.dx), animated, cols) {
                    continue;
                }
            }
            vis_pick.push(bi as u32);
            vis_anim
                .push(anims.iter().position(|a| a.node_id == b.node_id).map_or(-1, |i| i as i16));
            if let Ok(ti) = self.texts.binary_search_by_key(&b.node_id, |t| t.node) {
                vis_text.push((bi as u32, ti as u32));
            }
        }
        // Glass occlusion for the GLYPH pass: text runs paint AFTER all box fills, so a run
        // belonging to a node UNDER a later glass ROOT (an overlay panel) would print over the
        // panel's reset fill. Drop runs whose box overlaps a later glass root — nested glass
        // blends src-over, so a pane's caption keeps painting under a `subtle` row.
        // Only a glass root of the same pass occludes (a covered layer's content and the modal
        // over it are both the base's).
        let in_pass = |b: &nexus_layout::LayoutBox| match pass {
            Pass::Base => !self.in_band(b) && self.motion_of(b.node_id).is_none(),
            Pass::Band(root) => b.scroll_root == Some(root),
            Pass::Motion(k) => !self.in_band(b) && self.motion_of(b.node_id) == Some(k),
        };
        vis_text.retain(|&(bi, _)| {
            let t = &self.layout.boxes[bi as usize];
            !self.layout.boxes.iter().any(|g| {
                g.node_id > t.node_id
                    && g.visual.material.is_some_and(|m| m.blurs() || m.opaque())
                    && !g.glass_nested
                    && in_pass(g)
                    && t.rect.x.0 < g.rect.x.0 + g.rect.width.0
                    && t.rect.x.0 + t.rect.width.0 > g.rect.x.0
                    && t.rect.y.0 < g.rect.y.0 + g.rect.height.0
                    && t.rect.y.0 + t.rect.height.0 > g.rect.y.0
            })
        });
        self.vis_pick = vis_pick;
        self.vis_anim = vis_anim;
        self.vis_text = vis_text;
        self.pass_ctx = ctx;
    }

    /// The pass `prepare_picks_with` planned, as the row painter reads it (TASK-0367 P1): any
    /// band of its rows paints from it, on any thread (`pass_plan.rs`).
    pub(super) fn plan<'a>(&'a self, views: Views<'a>) -> PassPlan<'a> {
        PassPlan {
            width: self.w as i32,
            boxes: &self.layout.boxes,
            picks: &self.vis_pick,
            pick_anims: &self.vis_anim,
            runs: &self.vis_text,
            texts: &self.texts,
            anims: self.pass_ctx.anims(),
            caret: self.pass_ctx.caret,
            views,
        }
    }

    /// Row `y` of the planned pass over what `row` holds (`PassPlan::paint_row`).
    fn paint_row(&self, row: &mut [u8], y: i32, shift: i32, views: Views<'_>) {
        self.plan(views).paint_row(row, y, shift);
    }

    /// Caret geometry for the row painters: `(node_id, content_width_px,
    /// color)` of the focused TextInput — `None` without focus. The focus
    /// snapshot names the HANDLER box; the input node lives in its subtree
    /// (same containment contract as `subtree_is_secure`).
    fn caret_probe(&self) -> Option<CaretPaint> {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let f = self.view.text_focus()?;
        let mut idx = 0usize;
        let (nid, content, font, tracking, _) =
            caret_input(self.view.scene(), &mut idx, f.box_id, false)?;
        // TASK-0067B: the selection as an x span — prefix widths of the DISPLAYED run (a masked
        // password has the same char count).
        let x_at =
            |n: usize| nexus_text_baked::measure(content.chars().take(n), font, tracking) as i32;
        let a = tokens_for(self.theme_mode).color(ColorToken::Accent);
        Some(CaretPaint {
            nid,
            selection: f.selection.map(|(s, e)| (x_at(s), x_at(e))),
            highlight: [a.b, a.g, a.r, 96],
            scroll: self.field_scroll.1,
        })
    }

    /// The focused field's horizontal offset, followed to its caret (TASK-0346 P2) — once per
    /// pass, before the probe that paints with it; a new focus session starts at 0.
    fn update_field_scroll(&mut self) {
        let Some(f) = self.view.text_focus() else {
            self.field_scroll = (0, 0);
            return;
        };
        let mut idx = 0usize;
        let Some((nid, content, font, tracking, _)) =
            caret_input(self.view.scene(), &mut idx, f.box_id, false)
        else {
            return;
        };
        let x_at =
            |n: usize| nexus_text_baked::measure(content.chars().take(n), font, tracking) as i32;
        let (caret_x, text_w) = (x_at(f.caret), x_at(content.chars().count()));
        let Some(box_w) =
            self.layout.boxes.iter().find(|b| b.node_id == nid).map(|b| b.rect.width.0)
        else {
            return;
        };
        let prev = if self.field_scroll.0 == f.session { self.field_scroll.1 } else { 0 };
        let off = nexus_textedit::view::follow(prev, caret_x, box_w, text_w, 8);
        self.field_scroll = (f.session, off);
    }
}
