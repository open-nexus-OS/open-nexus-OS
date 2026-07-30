// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` paint subsystem (pure move out of `main.rs`): renders the
//! scene (fills + glyph runs) into the surface VMO — the damage-limited
//! `render_rows`, the WebRender packed-band `render_band`, and the handler-box
//! diagnostic dump. No behavior change.

use super::*;

mod collect;
pub(super) use collect::collect_texts;
use collect::{caret_input, paint_caret_row};

impl super::DslApp {
    /// The paint-time hover wash for the currently hovered control, or `None`
    /// when nothing is hovered.
    ///
    /// Colour comes from the theme's `GlassHover` role — the handoff's
    /// `--glass-hover-bg`, which flips with the theme (a light wash on dark, an
    /// ink wash on light). It was previously an Accent tint, which read as a
    /// selection rather than a hover.
    pub(super) fn hover_wash(&self) -> Option<nexus_scene_raster::HoverWash> {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let node_id = crate::hover_wash::hover_wash_anchor(&self.layout.boxes, self.hovered?)?;
        let c = tokens_for(self.theme_mode).color(ColorToken::GlassHover);
        Some(nexus_scene_raster::HoverWash {
            node_id,
            color: nexus_layout_types::Rgba8::new(c.r, c.g, c.b, c.a),
            // No ring: the handoff washes chrome buttons and list rows flat.
            // The ring is a slider affordance and does not belong on every
            // hovered control (windowd's own chrome passes 0 for the same
            // reason).
            ring_alpha: 0,
        })
    }

    /// Writes the current scene (fills + glyph runs) into the VMO. The
    /// page base is the theme's Surface token — the scene's own boxes
    /// (surfaceVariant buttons, onSurface text) are specified against it.
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

    pub(super) fn render(&mut self, vmo: u32) -> bool {
        self.render_rows(vmo, 0, self.h as i32)
    }

    /// Renders only rows `[y0, y1)` into the VMO — the damage-limited
    /// path (hover washes re-render two box spans, not 1280×800). The
    /// full render is `render()` = the whole surface span.
    pub(super) fn render_rows(&mut self, vmo: u32, y0: i32, y1: i32) -> bool {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let s = tokens_for(self.theme_mode).color(ColorToken::Surface);
        // Page base = the theme Surface token: OPAQUE for a desktop/
        // fullscreen surface (the base layer), frosted-translucent for
        // floating windows (`base_alpha`).
        let base = [s.b, s.g, s.r, self.base_alpha];
        // HOVER = WASH + MOTION, which is what the handoff specifies
        // ("hover:rgba(255,255,255,0.15)" AND "Icons: scale(1.08) hover").
        //
        // The wash used to be hard-`None` here for a real reason: it follows
        // the hovered box's `corner_radius`, and a HANDLER box often carries
        // none — `Stack { Circle { … } } on Tap` puts the handler on a
        // square-cornered wrapper, so a hovered circle wore a white SQUARE.
        // The answer is to fix the anchor, not to delete the affordance:
        // `hover_wash_anchor` walks from the handler box to the child that
        // actually draws the control, so the wash inherits the radius the user
        // can see. Motion is unchanged — the spring still runs.
        let hover = self.hover_wash();
        let surf_w = self.w as usize;
        let row_bytes = surf_w * 4;
        // Reused scratch — NEVER allocate per render (non-freeing heap).
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        let y_start = y0.max(0);
        let y_end = y1.min(self.h as i32);
        // Paint-time scroll transform of the page's `.scroll(...)`
        // viewport (identity when the page has none).
        let scroll_view = self
            .scroll_param()
            .map(|(clip, dx, dy)| nexus_scene_raster::ScrollView { clip, dx, dy });
        // Visibility index, ONCE per repaint (not per row): which boxes
        // intersect the span — clipped boxes tested in MODEL space (span
        // shifted by the scroll offset), chrome tested directly. `texts`
        // is pre-order sorted (collect_texts counts), so the text run per
        // box resolves by binary search.
        // Snapshot the per-node animation transforms (opacity/translate/scale)
        // the DSL `.animate`/`.transition`/`.effect` binding is driving this
        // frame — a COPY (not a borrow) so the render scratch stays mutable.
        // Bounded by the host's active-animation cap; empty at rest.
        let mut anim_buf =
            [nexus_scene_raster::NodeAnim::identity(0); super::anim::MAX_EXPANDED_ANIMS];
        let anim_n = self.expand_node_anims(&mut anim_buf);
        let anims = &anim_buf[..anim_n];
        // Caret v1 (RFC-0075 8c follow-through): the FOCUSED TextField paints
        // a static 2-px bar after its content (append-only caret model; blink
        // needs a frame pulse = recorded follow-up). Resolved once per repaint.
        let caret = self.caret_probe();
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
            let visible = match (scroll_view, b.clip_rect) {
                (Some(sv), Some(_)) => {
                    let s0 = y_start.max(sv.clip.1);
                    let s1 = y_end.min(sv.clip.3);
                    s0 < s1 && by < s1 + sv.dy && by + bh > s0 + sv.dy
                }
                _ => by < y_end && by + bh > y_start,
            };
            if !visible {
                continue;
            }
            vis_pick.push(bi as u32);
            // Box -> anim mapping ONCE per repaint (not per row).
            vis_anim
                .push(anims.iter().position(|a| a.node_id == b.node_id).map_or(-1, |i| i as i16));
            if let Ok(ti) = self.texts.binary_search_by_key(&b.node_id, |(id, _, _, _)| *id) {
                vis_text.push((bi as u32, ti as u32));
            }
        }
        // Glass occlusion for the GLYPH pass: text runs paint AFTER all box
        // fills, so a run belonging to a node UNDER a later glass box (an
        // overlay panel) would print over the panel's reset fill. Drop runs
        // whose box overlaps a later glass ROOT — only a root resets its
        // fill; NESTED glass blends src-over now, so a pane's caption must
        // keep painting when a `subtle` row follows it (dropping on any
        // later glass erased half the labels of a glass-heavy page).
        // (Overlap, not full cover: a label half-under a panel belongs
        // under it entirely.)
        vis_text.retain(|&(bi, _)| {
            let t = &self.layout.boxes[bi as usize];
            !self.layout.boxes.iter().any(|g| {
                g.node_id > t.node_id
                    && matches!(g.visual.material, nexus_layout_types::SurfaceMaterial::Glass(_))
                    && !g.glass_nested
                    && t.rect.x.0 < g.rect.x.0 + g.rect.width.0
                    && t.rect.x.0 + t.rect.width.0 > g.rect.x.0
                    && t.rect.y.0 < g.rect.y.0 + g.rect.height.0
                    && t.rect.y.0 + t.rect.height.0 > g.rect.y.0
            })
        });
        for y in y_start..y_end {
            for px in row.chunks_exact_mut(4) {
                px.copy_from_slice(&base);
            }
            // Scene fills: the ONE promoted painter (`nexus-scene-raster`,
            // golden-verified) — rounded corners, circles, vector shapes,
            // borders, src-over glass. On-device pixels match the design
            // goldens by construction (the flat rect spans this replaces
            // were the "buttons are square" report).
            {
                let mut canvas = nexus_scene_raster::RowCanvas::new(&mut row, y, self.w as i32);
                nexus_scene_raster::paint_row_picked_indexed(
                    &mut canvas,
                    &self.layout.boxes,
                    &vis_pick,
                    &vis_anim,
                    hover,
                    scroll_view,
                    anims,
                );
            }
            // Glyph pass: the shared text SSOT (same blender windowd uses)
            // blends each run's slice intersecting this row.
            for &(bi, ti) in &vis_text {
                let b = &self.layout.boxes[bi as usize];
                let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                if bw <= 0 || bh <= 0 {
                    continue;
                }
                // Scrolled text: boxes inside the viewport sample at the
                // shifted model row/column; right edge clips at the
                // viewport (left overhang is bounded by the box width).
                let (y_eff, bx_eff, right) = match (scroll_view, b.clip_rect) {
                    (Some(sv), Some(_)) => {
                        if y < sv.clip.1 || y >= sv.clip.3 {
                            continue;
                        }
                        (y + sv.dy, bx - sv.dx, sv.clip.2.min(self.w as i32).max(0) as u32)
                    }
                    _ => (y, bx, self.w),
                };
                if y_eff < by || y_eff >= by + bh {
                    continue;
                }
                {
                    let (_, content, font, color) = &self.texts[ti as usize];
                    // Animated text node (`.animate(fade)`/`.effect(wiggle)` on a
                    // Text): fade its glyphs by the node opacity and shift them
                    // by the horizontal translate. Vertical translate + scale on
                    // TEXT are the fill path's domain (documented scope: glyphs
                    // fade + wiggle, filled nodes take the full transform).
                    let (color, bx_glyph) =
                        match anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity()) {
                            Some(a) => {
                                let mut c = *color;
                                c[3] = (c[3] as u32 * a.opacity as u32 / 255) as u8;
                                (c, bx_eff - a.dx)
                            }
                            None => (*color, bx_eff),
                        };
                    // A run wider than its own box is CUT — the platform does
                    // not wrap, so `layout_lines` clamps the line to the
                    // available width and the rest simply fell off the edge
                    // mid-glyph. Mark the cut so a truncated label reads as
                    // truncated instead of as a shorter label.
                    let (cut, ell) = nexus_text_baked::ellipsis_cut(
                        content,
                        *font,
                        b.rect.width.0.max(0) as u32,
                    );
                    let run = || {
                        content
                            .chars()
                            .take(cut)
                            .chain(core::iter::once(nexus_text_baked::ELLIPSIS).take(ell as usize))
                    };
                    // RFC-0082: `.textShadow(soft|strong)` — one extra glyph
                    // pass at the offset, BEFORE the run itself. Not a blur
                    // (the row painter has no offscreen buffer); it is the
                    // separation large type needs to survive a wallpaper.
                    if let Some(sh) = b.visual.text_shadow.filter(|s| s.color.a > 0) {
                        let sc = sh.color;
                        nexus_text_baked::draw_text_row(
                            &mut row,
                            y_eff as u32,
                            by + sh.offset_y.0,
                            (bx_glyph + sh.offset_x.0).max(0) as u32,
                            right,
                            run(),
                            *font,
                            [sc.b, sc.g, sc.r, (u32::from(sc.a) * u32::from(color[3]) / 255) as u8],
                        );
                    }
                    nexus_text_baked::draw_text_row(
                        &mut row,
                        y_eff as u32,
                        by,
                        bx_glyph.max(0) as u32,
                        right,
                        run(),
                        *font,
                        color,
                    );
                    if let Some((cnid, cw, ccol)) = caret {
                        if b.node_id == cnid {
                            paint_caret_row(&mut row, y_eff, by, bh, bx_glyph, cw, right, ccol);
                        }
                    }
                }
            }
            if vmo_write(vmo, y as usize * row_bytes, &row[..row_bytes]).is_err() {
                self.row_scratch = row;
                self.vis_pick = vis_pick;
                self.vis_anim = vis_anim;
                self.vis_text = vis_text;
                return false;
            }
        }
        self.row_scratch = row;
        // Hand the visibility buffers back (mem::take recycling — the
        // SAME close-the-loop rule the inputd events scratch violated).
        self.vis_pick = vis_pick;
        self.vis_anim = vis_anim;
        self.vis_text = vis_text;
        true
    }

    /// WebRender packed-band render (compositor-scroll): paint the WHOLE
    /// resident content into the TALL VMO ONCE — NOT scrolled. The band is
    /// packed `[fixed header][fixed footer][scroll content]`:
    ///   * fixed Toolbar → band rows `[0, header_h)` (IDENTITY, model rows
    ///     `[0, clip.top)`);
    ///   * fixed composer → band rows `[header_h, header_h+footer_h)`
    ///     (model rows `[clip.bottom, h)`);
    ///   * scroll content (boxes with a `clip_rect`) → band rows
    ///     `[band_content_top, …)` at `band_content_top + (model_y -
    ///     clip.top)` with NO paint-time `dy` (the compositor supplies the
    ///     offset via the layer `src_row`).
    /// The compositor then composites 3 slices out of this band. Runs on a
    /// CONTENT change only (mount / resize / LoadMore), never per notch.
    pub(super) fn render_band(&mut self, vmo: u32) -> bool {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        let Some((header_h, footer_h, content_h)) = self.band_geometry() else {
            // No scroll region (content shrank below the viewport): fall back
            // to the plain render so the visible frame still paints.
            return self.render(vmo);
        };
        let Some((clip, _cw, _ch)) = self.scroll_region() else {
            return self.render(vmo);
        };
        let (_vx0, vy0, _vx1, vy1) = clip;
        let header_h = header_h as i32;
        let band_content_top = header_h + footer_h as i32;
        // Bound the band to the VMO allocated at create (a LoadMore that grows
        // content can't grow the VMO — `tail(…)` keeps the content finite).
        let mut band_h = header_h + footer_h as i32 + content_h as i32;
        if self.alloc_band_h > 0 {
            band_h = band_h.min(self.alloc_band_h as i32);
        }
        let s = tokens_for(self.theme_mode).color(ColorToken::Surface);
        let base = [s.b, s.g, s.r, self.base_alpha];
        let surf_w = self.w as usize;
        let row_bytes = surf_w * 4;
        let mut row = core::mem::take(&mut self.row_scratch);
        if row.len() < row_bytes {
            row.resize(row_bytes, 0);
        }
        // The caret paints on the banded path too (a composer in a fixed
        // footer is exactly this path), and so does the hover wash — a
        // scrollable app's rows and buttons hover like any other.
        let caret = self.caret_probe();
        let hover = self.hover_wash();
        // Per-node animation transforms apply on the banded path too (an
        // animated fixed header / a breathing Skeleton in the content) —
        // same snapshot contract as `render_rows`.
        let mut anim_buf =
            [nexus_scene_raster::NodeAnim::identity(0); super::anim::MAX_EXPANDED_ANIMS];
        let anim_n = self.expand_node_anims(&mut anim_buf);
        let anims = &anim_buf[..anim_n];
        // Two region picks (recycled): clipped boxes = the scroll content;
        // unclipped = the fixed header/footer/base. `paint_row_picked` skips a
        // box that does not intersect the row, so one pick per region suffices.
        let mut clipped = core::mem::take(&mut self.vis_pick);
        let mut unclipped = core::mem::take(&mut self.band_pick);
        clipped.clear();
        unclipped.clear();
        for (bi, b) in self.layout.boxes.iter().enumerate() {
            if b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
                continue;
            }
            if b.clip_rect.is_some() {
                clipped.push(bi as u32);
            } else {
                unclipped.push(bi as u32);
            }
        }
        let mut ok = true;
        for br in 0..band_h {
            for px in row[..row_bytes].chunks_exact_mut(4) {
                px.copy_from_slice(&base);
            }
            // Band row → model row + which region's boxes paint here.
            let (model_y, pick): (i32, &[u32]) = if br < header_h {
                (br, &unclipped) // fixed header (identity)
            } else if br < band_content_top {
                (vy1 + (br - header_h), &unclipped) // fixed footer
            } else {
                (vy0 + (br - band_content_top), &clipped) // scroll content (no dy)
            };
            {
                let mut canvas =
                    nexus_scene_raster::RowCanvas::new(&mut row, model_y, self.w as i32);
                // scroll = None: IDENTITY paint (the compositor owns the
                // scroll offset). Clipped boxes still paint at their model
                // position; the band row remap is via `model_y`.
                nexus_scene_raster::paint_row_picked_animated(
                    &mut canvas,
                    &self.layout.boxes,
                    pick,
                    hover,
                    None,
                    anims,
                );
            }
            // Glyph pass: the runs of this region's boxes intersecting model_y.
            for &bi in pick.iter() {
                let b = &self.layout.boxes[bi as usize];
                let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                if bw <= 0 || bh <= 0 || model_y < by || model_y >= by + bh {
                    continue;
                }
                if let Ok(ti) = self.texts.binary_search_by_key(&b.node_id, |(id, _, _, _)| *id) {
                    let (_, content, font, color) = &self.texts[ti];
                    // Animated text node: fade + horizontal shift (same
                    // contract as the plain-path glyph pass).
                    let (color, bx_glyph) =
                        match anims.iter().find(|a| a.node_id == b.node_id && !a.is_identity()) {
                            Some(a) => {
                                let mut c = *color;
                                c[3] = (c[3] as u32 * a.opacity as u32 / 255) as u8;
                                (c, bx - a.dx)
                            }
                            None => (*color, bx),
                        };
                    nexus_text_baked::draw_text_row(
                        &mut row,
                        model_y as u32,
                        by,
                        bx_glyph.max(0) as u32,
                        self.w,
                        content.chars(),
                        *font,
                        color,
                    );
                    if let Some((cnid, cw, ccol)) = caret {
                        if b.node_id == cnid {
                            paint_caret_row(&mut row, model_y, by, bh, bx_glyph, cw, self.w, ccol);
                        }
                    }
                }
            }
            if vmo_write(vmo, br as usize * row_bytes, &row[..row_bytes]).is_err() {
                ok = false;
                break;
            }
        }
        self.row_scratch = row;
        self.vis_pick = clipped;
        self.band_pick = unclipped;
        ok
    }

    /// Caret geometry for the row painters: `(node_id, content_width_px,
    /// color)` of the focused TextInput — `None` without focus. The focus
    /// snapshot names the HANDLER box; the input node lives in its subtree
    /// (same containment contract as `subtree_is_secure`).
    fn caret_probe(&self) -> Option<(usize, i32, [u8; 4])> {
        let f = self.view.text_focus()?;
        let mut idx = 0usize;
        let (nid, content, font, color) =
            caret_input(self.view.scene(), &mut idx, f.box_id, false)?;
        Some((nid, nexus_text_baked::measure(content.chars(), font) as i32, color))
    }
}
