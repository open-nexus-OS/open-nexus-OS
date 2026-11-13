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
use crate::straight_alpha::{premultiplied, unpremultiply};

mod collect;
mod split;
pub(in crate::probe) use collect::caret_input;
pub(super) use collect::collect_texts;
pub(super) use collect::dump_text_runs as collect_dump_text_runs;
use collect::{paint_selection_row, CaretPaint};

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
        self.render_rows(mem, 0, self.h as i32)
    }

    /// Renders base rows `[y0, y1)` straight into the slot being drawn (RFC-0114) — the
    /// damage-limited path. The page base is [`Self::page_base`]; the scene's own boxes are
    /// specified against it.
    pub(super) fn render_rows(&mut self, mem: &mut SurfaceMem, y0: i32, y1: i32) -> bool {
        self.overlay_sync();
        let base = self.page_base();
        let (y0, y1) = (y0.max(0), y1.min(self.h as i32));
        if !self.split_done && y0 == 0 && y1 == self.h as i32 {
            return self.render_rows_split(mem); // once: where a row's time goes (TASK-0367 P0)
        }
        let t0 = nsec_now();
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        self.prepare_picks_with(Pass::Base, (y0, y1), None, &views);
        let row_bytes = self.w as usize * 4;
        let mut ok = true;
        for y in y0..y1 {
            // The row in the buffer itself: painted where the GPU reads it, no copy.
            let Some(row) = mem.row_mut(y as u32).and_then(|r| r.get_mut(..row_bytes)) else {
                ok = false;
                break;
            };
            nexus_paint_kernels::fill(row, base);
            self.paint_row(row, y, 0, &views);
            // The compositor reads the base as straight alpha (RFC-0109), like every row.
            unpremultiply(row);
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
            let (sl, st, sr, sb) = nexus_scene_raster::shadow_reach(b);
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
            if let Some((c0, c1)) = cols {
                if b.rect.x.0 - sl >= c1 || b.rect.x.0 + b.rect.width.0 + sr <= c0 {
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

    /// Paints row `y` (surface row of the base; layout row of a band) over what `row` holds:
    /// the picked boxes' fills by the shared painter, then their glyph runs. `shift` moves the
    /// canvas (a horizontal band's run of columns); `views` place a covered layer's content.
    fn paint_row(&self, row: &mut [u8], y: i32, shift: i32, views: Views<'_>) {
        self.paint_row_boxes(row, y, shift, views);
        self.paint_row_text(row, y, shift, views);
    }

    /// Row `y`'s picked boxes, by the shared painter.
    fn paint_row_boxes(&self, row: &mut [u8], y: i32, shift: i32, views: Views<'_>) {
        let mut canvas = nexus_scene_raster::RowCanvas::new(row, y, self.w as i32);
        canvas.shift_x = shift;
        nexus_scene_raster::paint_row_picked_indexed(
            &mut canvas,
            &self.layout.boxes,
            &self.vis_pick,
            &self.vis_anim,
            views,
            self.pass_ctx.anims(),
        );
    }

    /// Row `y`'s picked text runs over its boxes.
    fn paint_row_text(&self, row: &mut [u8], y: i32, shift: i32, views: Views<'_>) {
        let anims = self.pass_ctx.anims();
        // The focused field's selection highlight (TASK-0067B); its caret is the compositor's
        // layer (RFC-0113 §5, `interaction/text_caret.rs`), never painted here.
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
            // The rows its text shadows reach (the halo's blur) are its rows too.
            let (above, below) = b.visual.text_shadows.iter().flatten().fold((0, 0), |r, s| {
                let reach = nexus_text_baked::blur_reach(u32::from(s.blur_radius)) as i32;
                let oy = i32::from(s.offset_y);
                (r.0.max(reach - oy), r.1.max(reach + oy))
            });
            if y_eff < by - above || y_eff >= by + bh + below {
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
            let t = &self.texts[ti as usize];
            let (content, color, tracking) = (&t.content, &t.color, t.tracking);
            // `.textFit`: the LAYOUT chose this run's size from its container's box, so it wins
            // over the style's — re-resolved with the REQUESTED weight, its line box scaled alike.
            let (font, line) = match b.text_px {
                Some(px) => (
                    nexus_text_baked::FontSize::nearest(
                        px.0,
                        nexus_text_baked::measure_text::BakedTextMeasure::weight(t.weight),
                    ),
                    nexus_text_baked::line_height(nexus_text_baked::FontSize::nearest(
                        px.0,
                        nexus_text_baked::measure_text::BakedTextMeasure::weight(t.weight),
                    )) as i32,
                ),
                None => (t.font, t.line),
            };
            let font = &font;
            // RFC-0116: the face's own band sits in the style's line box with half-leading
            // (`body` is a 17 px band in a 20 px line).
            let by = by + (line - nexus_text_baked::line_height(*font) as i32) / 2;
            // Animated text node: fade its glyphs by the node opacity and shift them by the
            // horizontal translate (vertical translate + scale on TEXT are the fill path's).
            let (mut color, bx_glyph) =
                match anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity()) {
                    Some(a) => {
                        let mut c = *color;
                        c[3] = (c[3] as u32 * a.opacity as u32 / 255) as u8;
                        (c, bx - dx - a.dx)
                    }
                    None => (*color, bx - dx),
                };
            // A faded box (its own and its ancestors' opacity — a disabled control) fades its text.
            color[3] = (u32::from(color[3]) * u32::from(b.opacity) / 255) as u8;
            // The focused field shows its WHOLE text, scrolled so its caret stays in view
            // (TASK-0346 P2) and clipped to its box; any other run wider than its box is CUT —
            // marked, so a truncated label reads as truncated instead of as a shorter label.
            let focused = caret.filter(|c| c.nid == b.node_id);
            let (row, bx_glyph, right) = match focused {
                Some(c) => {
                    let box_l = (bx - dx).max(0) as usize;
                    if box_l * 4 >= row.len() {
                        continue;
                    }
                    let box_r = ((bx - dx + bw).max(0) as u32).min(right);
                    let shifted = bx_glyph - c.scroll - box_l as i32;
                    (&mut row[box_l * 4..], shifted, box_r.saturating_sub(box_l as u32))
                }
                None => (row, bx_glyph, right),
            };
            let (cut, ell) = match focused {
                Some(_) => (content.chars().count(), false),
                None => nexus_text_baked::ellipsis_cut(content, *font, tracking, bw.max(0) as u32),
            };
            let run = || {
                content
                    .chars()
                    .take(cut)
                    .chain(core::iter::once(nexus_text_baked::ELLIPSIS).take(ell as usize))
            };
            // `.textAlign`: a run narrower than its box sits at the box's centre or right edge (the
            // focused field keeps its scroll origin).
            let bx_glyph = match (focused, t.align) {
                (
                    None,
                    nexus_layout_types::TextAlign::Center | nexus_layout_types::TextAlign::Right,
                ) => {
                    let w = nexus_text_baked::measure(run(), *font, tracking) as i32;
                    let slack = (bw - w).max(0);
                    let right = t.align == nexus_layout_types::TextAlign::Right;
                    bx_glyph + if right { slack } else { slack / 2 }
                }
                _ => bx_glyph,
            };
            // TASK-0067B: the selection BEHIND the run.
            if let Some(c) = focused {
                paint_selection_row(row, y_eff, by, bh, bx_glyph, c.selection, right, c.highlight);
            }
            // The text shadows (the label halo, `.shadow(halo)` in `labelShadow`), back to front,
            // blurred as the design draws them and faded with the text.
            for sh in b.visual.text_shadows.iter().rev().flatten().filter(|s| s.color.a > 0) {
                let sc = sh.color;
                nexus_text_baked::draw_text_blur_row(
                    row,
                    y_eff as u32,
                    by + i32::from(sh.offset_y),
                    bx_glyph + i32::from(sh.offset_x),
                    right,
                    run(),
                    *font,
                    tracking,
                    [sc.b, sc.g, sc.r, (u32::from(sc.a) * u32::from(color[3]) / 255) as u8],
                    u32::from(sh.blur_radius),
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
                tracking,
                color,
            );
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
