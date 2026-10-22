// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` paint subsystem: the ONE row painter (fills + glyph runs) behind the BASE
//! layer's rows and every scroll layer's band (RFC-0109). A pass picks its boxes once — the
//! base: every box but the content of an uncovered scroll layer, a covered layer's content at
//! its view; a band: one layer's content at identity — and paints row by row: the shared
//! painter (`nexus-scene-raster`), then the glyph pass with its caret and selection. Hover is no
//! paint any more: it is a wash layer the compositor draws (`probe/band.rs`). Plus the
//! handler-box diagnostic dump and the tap trace.

use super::*;

mod collect;
pub(super) use collect::collect_texts;
pub(super) use collect::dump_text_runs as collect_dump_text_runs;
use collect::{caret_input, paint_caret_row, paint_selection_row, CaretPaint};

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
    /// The base layer: every box but the content of an uncovered scroll layer — a covered
    /// layer's content at its view.
    Base,
    /// Scroll layer `root`'s content at identity: its band.
    Band(usize),
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
        for (_, content, _, _, _) in self.texts.iter().take(2) {
            let _ = core::fmt::write(&mut txt, format_args!("'{content}' "));
        }
        raw_marker(&alloc::format!("apphost: tap ({x},{y}) hit={hit:?} dmg={damage:?} txt={txt}"));
        true
    }

    /// The page base's alpha: the surface's (`base_alpha` — 0 on the desktop, opaque for an
    /// overlay or a fullscreen window, frosted for a floating one) — and opaque for a floating
    /// window too under solid materials (TASK-0337, reduce transparency).
    fn page_base_alpha(&self) -> u8 {
        if wire::theme_is_solid(self.theme_mode) && self.base_alpha > 0 {
            255
        } else {
            self.base_alpha
        }
    }

    /// The whole base layer.
    pub(super) fn render(&mut self, vmo: u32) -> bool {
        // Every dispatch path presents through here: mirror the modal depth to
        // windowd and keep the transient timer honest BEFORE the pixels (TASK-0074).
        self.overlay_sync();
        self.render_rows(vmo, 0, self.h as i32)
    }

    /// Renders base rows `[y0, y1)` into the VMO — the damage-limited path. The page base is the
    /// theme's Surface token; the scene's own boxes are specified against it.
    pub(super) fn render_rows(&mut self, vmo: u32, y0: i32, y1: i32) -> bool {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        self.overlay_sync();
        let s = tokens_for(self.theme_mode).color(ColorToken::Surface);
        // Page base = the theme Surface token: OPAQUE for a desktop/fullscreen surface (the
        // base layer), frosted-translucent for floating windows (`base_alpha`).
        let base = [s.b, s.g, s.r, self.page_base_alpha()];
        let (y0, y1) = (y0.max(0), y1.min(self.h as i32));
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        self.prepare_picks_with(Pass::Base, (y0, y1), None, &views);
        let row_bytes = self.w as usize * 4;
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        let mut ok = true;
        for y in y0..y1 {
            for px in row[..row_bytes].chunks_exact_mut(4) {
                px.copy_from_slice(&base);
            }
            self.paint_row(&mut row[..row_bytes], y, 0, &views);
            if vmo_write(vmo, y as usize * row_bytes, &row[..row_bytes]).is_err() {
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
    /// the compositor samples, and written at byte `dst` of the surface. `false` when the write
    /// was refused.
    pub(super) fn paint_band_row(
        &mut self,
        vmo: u32,
        model_y: i32,
        shift: i32,
        cols: (i32, i32),
        dst: usize,
    ) -> bool {
        let row_bytes = self.w as usize * 4;
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        row[..row_bytes].fill(0);
        self.paint_row(&mut row[..row_bytes], model_y, shift, &[]);
        let (c0, c1) =
            (cols.0.clamp(0, self.w as i32) as usize, cols.1.clamp(0, self.w as i32) as usize);
        let span = &mut row[c0 * 4..c1.max(c0) * 4];
        unpremultiply(span);
        let ok = vmo_write(vmo, dst, span).is_ok();
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
        ctx.caret = self.caret_probe();
        let anims = ctx.anims();
        let mut vis_pick = core::mem::take(&mut self.vis_pick);
        let mut vis_anim = core::mem::take(&mut self.vis_anim);
        let mut vis_text = core::mem::take(&mut self.vis_text);
        vis_pick.clear();
        vis_anim.clear();
        vis_text.clear();
        for (bi, b) in self.layout.boxes.iter().enumerate() {
            let (by, bh) = (b.rect.y.0, b.rect.height.0);
            if bh <= 0 || b.rect.width.0 <= 0 {
                continue;
            }
            let (row0, row1) = match pass {
                Pass::Band(root) => {
                    if b.scroll_root != Some(root) {
                        continue;
                    }
                    (y0, y1)
                }
                Pass::Base => {
                    if self.in_band(b) {
                        continue; // drawn in its layer's band
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
            if let Some((c0, c1)) = cols {
                if b.rect.x.0 >= c1 || b.rect.x.0 + b.rect.width.0 <= c0 {
                    continue;
                }
            }
            vis_pick.push(bi as u32);
            vis_anim
                .push(anims.iter().position(|a| a.node_id == b.node_id).map_or(-1, |i| i as i16));
            if let Ok(ti) = self.texts.binary_search_by_key(&b.node_id, |(id, _, _, _, _)| *id) {
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
            Pass::Base => !self.in_band(b),
            Pass::Band(root) => b.scroll_root == Some(root),
        };
        vis_text.retain(|&(bi, _)| {
            let t = &self.layout.boxes[bi as usize];
            !self.layout.boxes.iter().any(|g| {
                g.node_id > t.node_id
                    && matches!(g.visual.material, nexus_layout_types::SurfaceMaterial::Glass(_))
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

    /// Paints row `y` (surface row of the base; layout row of a band) over what `row` holds:
    /// the picked boxes' fills by the shared painter, then their glyph runs. `shift` moves the
    /// canvas (a horizontal band's run of columns); `views` place a covered layer's content.
    fn paint_row(
        &self,
        row: &mut [u8],
        y: i32,
        shift: i32,
        views: &[nexus_scene_raster::ScrollView],
    ) {
        let anims = self.pass_ctx.anims();
        {
            let mut canvas = nexus_scene_raster::RowCanvas::new(row, y, self.w as i32);
            canvas.shift_x = shift;
            nexus_scene_raster::paint_row_picked_indexed(
                &mut canvas,
                &self.layout.boxes,
                &self.vis_pick,
                &self.vis_anim,
                views,
                anims,
            );
        }
        // Caret v1 (RFC-0075 8c): the FOCUSED TextInput paints a static 2-px bar.
        let caret = self.pass_ctx.caret;
        let row_px = (row.len() / 4) as i32;
        for &(bi, ti) in &self.vis_text {
            let b = &self.layout.boxes[bi as usize];
            let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
            if bw <= 0 || bh <= 0 {
                continue;
            }
            // Where this run is sampled and shifted: a scroll view's content at its view, inside
            // its viewport; the rest at the row itself.
            let view = b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r));
            let (y_eff, dx, mut left, mut right) = match view {
                Some(v) => {
                    if y < v.clip.1 || y >= v.clip.3 {
                        continue;
                    }
                    (y + v.dy, shift + v.dx, v.clip.0, v.clip.2)
                }
                None => (y, shift, 0, row_px),
            };
            if y_eff < by || y_eff >= by + bh {
                continue;
            }
            // The box's own clip (its content's clips), in its space.
            if let Some(c) = b.clip_rect {
                if y_eff < c.y.0 || y_eff >= c.y.0 + c.height.0 {
                    continue;
                }
                left = left.max(c.x.0 - dx);
                right = right.min(c.x.0 + c.width.0 - dx);
            }
            // The run paints into the row from `left` on — cut there as at `right`.
            let (left, right) = (left.clamp(0, row_px), right.min(row_px));
            if right <= left {
                continue;
            }
            let row = &mut row[left as usize * 4..];
            let (bx, right) = (bx - left, (right - left) as u32);
            let (_, content, font, color, weight) = &self.texts[ti as usize];
            // `.textFit`: the LAYOUT chose this run's size from its container's box, so it wins
            // over the scene's authored `.textSize` — re-resolved with the REQUESTED weight.
            let font = &match b.text_px {
                Some(px) => nexus_text_baked::FontSize::nearest(
                    px.0,
                    nexus_text_baked::measure_text::BakedTextMeasure::weight(*weight),
                ),
                None => *font,
            };
            // Animated text node: fade its glyphs by the node opacity and shift them by the
            // horizontal translate (vertical translate + scale on TEXT are the fill path's).
            let (color, bx_glyph) =
                match anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity()) {
                    Some(a) => {
                        let mut c = *color;
                        c[3] = (c[3] as u32 * a.opacity as u32 / 255) as u8;
                        (c, bx - dx - a.dx)
                    }
                    None => (*color, bx - dx),
                };
            // A run wider than its own box is CUT — mark it so a truncated label reads as
            // truncated instead of as a shorter label.
            let (cut, ell) = nexus_text_baked::ellipsis_cut(content, *font, bw.max(0) as u32);
            let run = || {
                content
                    .chars()
                    .take(cut)
                    .chain(core::iter::once(nexus_text_baked::ELLIPSIS).take(ell as usize))
            };
            // TASK-0067B: the selection BEHIND the run.
            if let Some(c) = caret.filter(|c| c.nid == b.node_id) {
                paint_selection_row(row, y_eff, by, bh, bx_glyph, c.selection, right, c.highlight);
            }
            // RFC-0082: `.textShadow(soft|strong)` — one extra glyph pass at the offset.
            if let Some(sh) = b.visual.text_shadow.filter(|s| s.color.a > 0) {
                let sc = sh.color;
                nexus_text_baked::draw_text_row(
                    row,
                    y_eff as u32,
                    by + sh.offset_y.0,
                    bx_glyph + sh.offset_x.0,
                    right,
                    run(),
                    *font,
                    [sc.b, sc.g, sc.r, (u32::from(sc.a) * u32::from(color[3]) / 255) as u8],
                );
            }
            nexus_text_baked::draw_text_row(
                row,
                y_eff as u32,
                by,
                bx_glyph,
                right,
                run(),
                *font,
                color,
            );
            if let Some(c) = caret.filter(|c| c.nid == b.node_id) {
                paint_caret_row(row, y_eff, by, bh, bx_glyph, c.caret_x, right, c.color);
            }
        }
    }

    /// Caret geometry for the row painters: `(node_id, content_width_px,
    /// color)` of the focused TextInput — `None` without focus. The focus
    /// snapshot names the HANDLER box; the input node lives in its subtree
    /// (same containment contract as `subtree_is_secure`).
    fn caret_probe(&self) -> Option<CaretPaint> {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let f = self.view.text_focus()?;
        let mut idx = 0usize;
        let (nid, content, font, color) =
            caret_input(self.view.scene(), &mut idx, f.box_id, false)?;
        // TASK-0067B: the caret at its char index and the selection as an x span — prefix
        // widths of the DISPLAYED run (a masked password has the same char count).
        let x_at = |n: usize| nexus_text_baked::measure(content.chars().take(n), font) as i32;
        let a = tokens_for(self.theme_mode).color(ColorToken::Accent);
        Some(CaretPaint {
            nid,
            caret_x: x_at(f.caret),
            selection: f.selection.map(|(s, e)| (x_at(s), x_at(e))),
            color,
            highlight: [a.b, a.g, a.r, 96],
        })
    }
}

/// The painter blends onto a transparent row in premultiplied form; the compositor samples a
/// band as straight alpha — divided out once per row, where the alpha is partial.
fn unpremultiply(span: &mut [u8]) {
    for px in span.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a == 0 || a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
        }
    }
}
