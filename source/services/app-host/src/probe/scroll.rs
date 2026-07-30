// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` scroll subsystem (pure move out of `main.rs`): the
//! `.scroll(...)` viewport geometry, the wheel-impulse + eased `ScrollMomentum`
//! physics, the WebRender band geometry, the compositor-pushed absolute
//! position mirror, the `EndReached` lazy-load latch, and the retained
//! re-layout. No behavior change.

use super::*;

impl super::DslApp {
    /// The WebRender scroll band geometry `(header_h, footer_h, content_h)`
    /// in surface rows, or `None` when the page has no scrollable region.
    /// `header_h` = the fixed rows ABOVE the viewport (Toolbar), `footer_h`
    /// = the fixed rows BELOW it (composer), `content_h` = the tall resident
    /// scroll-content extent. Derived from the retained layout's scroll
    /// region (the engine's `clip_rect` viewport) — O(boxes), no cached state.
    pub(super) fn band_geometry(&self) -> Option<(u32, u32, u32)> {
        let (clip, _cw, content_h) = self.scroll_region()?;
        let (_, cy0, _, cy1) = clip;
        let content_h = content_h.max(0) as u32;
        if content_h == 0 {
            return None; // content fits — nothing scrolls, keep the plain path
        }
        let header_h = cy0.max(0) as u32;
        let footer_h = (self.h as i32 - cy1).max(0) as u32;
        Some((header_h, footer_h, content_h))
    }

    /// The page's scroll viewport, derived from the RETAINED boxes (the
    /// engine stamps `clip_rect` on every descendant of the one
    /// `.scroll(...)` container): (viewport x0,y0,x1,y1, content_w,
    /// content_h). O(boxes), alloc-free, no cached state to drift.
    pub(super) fn scroll_region(&self) -> Option<((i32, i32, i32, i32), i32, i32)> {
        self.scroll_region_axis().map(|(clip, cw, ch, _)| (clip, cw, ch))
    }

    /// [`Self::scroll_region`] + the DECLARED axis (from the container
    /// box's `Overflow::Scroll(axis)` — the `.scroll(...)` author decides
    /// what scrolls; content shape never guesses it).
    ///
    /// GATED on an actual `Overflow::Scroll` container: keying on "any box
    /// with a `clip_rect`" misdetected pages whose WIDGETS clip internally
    /// (the Skeleton's shimmer band is an `Overflow::Hidden` stack) as
    /// scrollable — which silently flipped them onto the BANDED
    /// compositor-scroll surface path (`render_band`, windowd-owned wheel)
    /// with a bogus 16-row "viewport". The viewport is the scroll
    /// container's own stamped clip; content extents count only boxes
    /// clipped WITHIN it (a widget's internal clip elsewhere is not scroll
    /// content).
    pub(super) fn scroll_region_axis(
        &self,
    ) -> Option<((i32, i32, i32, i32), i32, i32, nexus_layout_types::ScrollAxis)> {
        // A page may declare SEVERAL `.scroll(...)` containers (settings:
        // sidebar + content pane), but the surface protocol carries ONE
        // compositor scroll region. Pick the LARGEST by area — that is the
        // content pane, deterministically — never "first in traversal order",
        // which silently handed the whole band + hit transform to whichever
        // container the markup declared first. The others stay static;
        // `hit_scrolled` still hit-tests them inside their own clip.
        let (container, axis) = self
            .layout
            .boxes
            .iter()
            .filter_map(|b| {
                if let nexus_layout_types::Overflow::Scroll(a) = b.overflow {
                    Some((b, a))
                } else {
                    None
                }
            })
            .max_by_key(|(b, _)| b.rect.width.0 as i64 * b.rect.height.0 as i64)?;
        // The engine stamps the container's own clip (`Overflow::Scroll` is a
        // clipping overflow); the padded rect is the fallback contract.
        let clip = match container.clip_rect {
            Some(c) => (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0),
            None => (
                container.rect.x.0,
                container.rect.y.0,
                container.rect.x.0 + container.rect.width.0,
                container.rect.y.0 + container.rect.height.0,
            ),
        };
        let (mut content_r, mut content_b) = (0i32, 0i32);
        for b in &self.layout.boxes {
            let Some(c) = b.clip_rect else { continue };
            let cr = (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0);
            if cr.0 < clip.0 || cr.1 < clip.1 || cr.2 > clip.2 || cr.3 > clip.3 {
                continue; // clipped by a widget elsewhere, not scroll content
            }
            content_r = content_r.max(b.rect.x.0 + b.rect.width.0);
            content_b = content_b.max(b.rect.y.0 + b.rect.height.0);
        }
        Some((clip, content_r - clip.0, content_b - clip.1, axis))
    }

    /// How many `.scroll(...)` containers the retained layout carries — the
    /// surface protocol supports ONE compositor scroll region, so the mount
    /// path logs when a page declares more (the extras stay static).
    pub(super) fn scroll_container_count(&self) -> usize {
        self.layout
            .boxes
            .iter()
            .filter(|b| matches!(b.overflow, nexus_layout_types::Overflow::Scroll(_)))
            .count()
    }

    /// Whether STATIC (unclipped) painted content shares surface rows with
    /// the scroll viewport — the condition under which the 3-slice band
    /// model CANNOT render this page.
    ///
    /// The compositor's band slices are full-width row tiles: header rows,
    /// footer rows, and a content block it shifts by the scroll offset.
    /// Anything painted BESIDE the viewport in those rows (a sidebar, the
    /// content pane's own glass, a page-root overlay) would either vanish
    /// from the band (the old seam: panes ended at `header_h`, overlays were
    /// invisible-but-hittable) or scroll along with the content. A page
    /// shaped like that must take the plain path, which paints everything
    /// and scrolls by re-render.
    pub(super) fn band_statics_intersect_viewport(&self) -> bool {
        let Some((clip, _, _)) = self.scroll_region() else {
            return false;
        };
        let (_, vy0, _, vy1) = clip;
        self.layout.boxes.iter().any(|b| {
            if b.clip_rect.is_some() || b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
                return false;
            }
            let (by0, by1) = (b.rect.y.0, b.rect.y.0 + b.rect.height.0);
            if by1 <= vy0 || by0 >= vy1 {
                return false; // entirely inside the fixed header/footer rows
            }
            // Only boxes that PAINT count — layout-only stacks (the page
            // root, spacers-in-disguise) span everything and paint nothing.
            b.visual.background.is_some()
                || !matches!(b.visual.material, nexus_layout_types::SurfaceMaterial::Opaque)
                || self.texts.binary_search_by_key(&b.node_id, |(id, _, _, _)| *id).is_ok()
        })
    }

    /// The band the surface protocol can actually carry: the raw geometry
    /// gated by the resident-row budget AND the full-width-slice condition.
    /// Every create / re-negotiation site MUST use this one predicate — a
    /// detector that disagrees with the re-create re-creates onto the wrong
    /// path (the too-tall chat thread vanished exactly that way).
    pub(super) fn negotiated_band(&self) -> Option<(u32, u32, u32)> {
        // AXIS GUARD: the band is a VERTICAL compositor-scroll path (its
        // geometry is header/footer/content ROWS). A horizontal or paged
        // viewport whose content is also taller than the viewport would
        // otherwise negotiate a band — and a banded surface short-circuits
        // `wheel_event`, killing horizontal scrolling entirely.
        if let Some((_, _, _, axis)) = self.scroll_region_axis() {
            if axis != nexus_layout_types::ScrollAxis::Vertical {
                return None;
            }
        }
        self.band_geometry()
            .filter(|&(h, f, c)| h + f + c <= super::MAX_BAND_ROWS)
            .filter(|_| !self.band_statics_intersect_viewport())
    }

    /// [`Self::negotiated_band`] plus the mount-time markers that say WHY a
    /// scrolling page landed on the plain path. Both gates are honest
    /// fallbacks (visible-sized VMO, wheel-driven re-emit scroll — slower,
    /// but complete and correct):
    ///
    ///   * the resident-row budget — the gpud GL atlas is SHARED with every
    ///     resident surface; a taller band still ALLOCATES but gpud clamps
    ///     its upload and the window "vanishes" (the chat-thread re-create
    ///     bug);
    ///   * full-width slices — the band tiles full-width rows, so a page
    ///     whose static content shares rows with the viewport (sidebar
    ///     beside a scrolling pane, the pane's own glass, page-root
    ///     overlays) either lost those statics below the header (the
    ///     settings seam) or would scroll them along.
    ///
    /// Also states when SEVERAL `.scroll(...)` containers exist — the
    /// surface protocol carries ONE compositor scroll region, the extras
    /// stay static. That assumption used to be silent, and a page whose
    /// FIRST container was a sidebar handed the band to the wrong region
    /// with every content tap "missing".
    pub(super) fn negotiated_band_at_mount(&self) -> Option<(u32, u32, u32)> {
        let n = self.scroll_container_count();
        if n > 1 {
            super::raw_marker(&alloc::format!(
                "apphost: {n} scroll containers, largest is the band"
            ));
        }
        let band = self.negotiated_band();
        if band.is_none() {
            if let Some((h, f, c)) = self.band_geometry() {
                let horizontal = self
                    .scroll_region_axis()
                    .is_some_and(|(_, _, _, a)| a != nexus_layout_types::ScrollAxis::Vertical);
                if horizontal {
                    super::raw_marker("apphost: horizontal viewport, plain-path fallback");
                } else if h + f + c > super::MAX_BAND_ROWS {
                    super::raw_marker("apphost: band too tall, plain-path fallback");
                } else {
                    super::raw_marker("apphost: statics beside the viewport, plain-path fallback");
                }
            }
        }
        band
    }

    /// The active paint/hit scroll transform (`None` = nothing scrolls).
    pub(super) fn scroll_param(&self) -> Option<((i32, i32, i32, i32), i32, i32)> {
        if self.scroll_x == 0 && self.scroll_y == 0 {
            // Identity transform still needs the clip for correctness,
            // but the zero case is the common path — skip the box walk.
            if self.scroll_region().is_none() {
                return None;
            }
        }
        self.scroll_region().map(|(clip, _, _)| (clip, self.scroll_x, self.scroll_y))
    }

    /// Wheel notches over the viewport: an IMPULSE into the scroll
    /// physics — the target moves by `notches × STEP_PX`, the position
    /// EASES toward it across the loop's ticks (`momentum_tick`). Returns
    /// (repaint row span of the VIEWPORT ONLY, end-reached?) for the
    /// immediate first step. Paint-only — the retained boxes stay
    /// untouched; the span is bounded by the viewport, never the window.
    pub(super) fn scroll_wheel(&mut self, delta_notches: i32) -> (Option<(i32, i32)>, bool) {
        const STEP_PX: i32 = 72;
        let Some((clip, content_w, content_h, axis)) = self.scroll_region_axis() else {
            return (None, false);
        };
        let view_w = clip.2 - clip.0;
        let view_h = clip.3 - clip.1;
        let max_x = (content_w - view_w).max(0);
        let max_y = (content_h - view_h).max(0);
        // Linux REL_WHEEL convention: +1 = wheel UP (away from the user).
        // Wheel DOWN (toward the user, delta −1) moves the CONTENT up,
        // i.e. the offset target GROWS — hence the inversion.
        let delta = -delta_notches * STEP_PX;
        // The DECLARED axis decides — never the content shape (a wrapped
        // tile grid is taller than its viewport yet scrolls horizontally).
        if axis == nexus_layout_types::ScrollAxis::Vertical && max_y > 0 {
            self.momentum.set_extent(view_h as f32, content_h as f32);
            let _ = self.momentum.scroll_wheel(delta as f32);
            self.momentum_last_ns = nsec_now();
            // The eased position advances on ticks; apply the first step
            // now so a single notch responds within THIS frame.
            return self.momentum_step(clip, max_y, view_h);
        }
        // Paged viewports never reach here — `wheel_event` routes them to
        // `pager_wheel` (page snap + trigger). Plain horizontal viewports
        // stay direct-stepped v1.
        if axis == nexus_layout_types::ScrollAxis::Horizontal && max_x > 0 {
            let old = self.scroll_x;
            self.scroll_x = (self.scroll_x + delta).clamp(0, max_x);
            if self.scroll_x != old {
                let span = (clip.1.max(0), clip.3.min(self.h as i32));
                let near_end = self.scroll_x >= max_x - view_w / 2;
                let fire = near_end && !self.end_fired;
                if fire {
                    self.end_fired = true;
                }
                return (Some(span), fire);
            }
        }
        (None, false)
    }

    /// Advance the scroll physics (vertical ease OR pager glide — a page has
    /// ONE scroll region, so only one is ever live) by real elapsed time and
    /// apply the eased position. Returns the viewport repaint span while
    /// moving.
    pub(super) fn momentum_tick(&mut self) -> (Option<(i32, i32)>, bool) {
        if !self.momentum.is_animating() && !self.pager.is_animating() {
            return (None, false);
        }
        let now = nsec_now();
        let dt = now.saturating_sub(self.momentum_last_ns).min(100_000_000);
        self.momentum_last_ns = now;
        if self.momentum.is_animating() {
            let Some((clip, _, content_h)) = self.scroll_region() else {
                return (None, false);
            };
            let view_h = clip.3 - clip.1;
            let max_y = (content_h - view_h).max(0);
            let _ = self.momentum.tick(dt);
            return self.momentum_step(clip, max_y, view_h);
        }
        // Pager glide toward the snapped page offset.
        let Some((clip, content_w, _ch, axis)) = self.scroll_region_axis() else {
            // The paged container left the scene (page swap): settle cleanly.
            self.pager.set_offset(0.0);
            return (None, false);
        };
        if axis != nexus_layout_types::ScrollAxis::Paged {
            self.pager.set_offset(0.0);
            return (None, false);
        }
        let view_w = clip.2 - clip.0;
        let max_x = (content_w - view_w).max(0);
        let _ = self.pager.tick(dt);
        self.pager_step(clip, max_x)
    }

    /// Apply the physics position to the paint offset + the lazy-load
    /// latch. Shared by the impulse (first step) and the ticks.
    pub(super) fn momentum_step(
        &mut self,
        clip: (i32, i32, i32, i32),
        max_y: i32,
        view_h: i32,
    ) -> (Option<(i32, i32)>, bool) {
        let pos = self.momentum.offset_px().clamp(0, max_y);
        let near_end = max_y > 0 && pos >= max_y - view_h / 2;
        let fire = near_end && !self.end_fired;
        if fire {
            self.end_fired = true;
        }
        if pos == self.scroll_y {
            return (None, fire);
        }
        self.scroll_y = pos;
        let span = (clip.1.max(0), clip.3.min(self.h as i32));
        (Some(span), fire)
    }

    /// Whether the physics still eases/coasts (the loop keeps ticking).
    pub(super) fn momentum_active(&self) -> bool {
        self.momentum.is_animating() || self.pager.is_animating()
    }

    /// Wheel over a `.scroll(paged)` viewport: ONE notch turns ONE page —
    /// the glide target snaps to the next/previous whole page multiple and
    /// a 360 ms lock swallows the rest of the flick (the handoff's "Delta
    /// akkumulieren, ab 30 blättern, dann 360ms Sperre" for a ±1-notch
    /// wheel). Returns (repaint span, page delta: −1/0/+1) — the caller
    /// fires the `PageNext`/`PagePrev` trigger so the store's page index
    /// tracks the snapped offset.
    pub(super) fn pager_wheel(&mut self, delta_notches: i32) -> (Option<(i32, i32)>, i32) {
        let Some((clip, content_w, _ch, axis)) = self.scroll_region_axis() else {
            return (None, 0);
        };
        if axis != nexus_layout_types::ScrollAxis::Paged || delta_notches == 0 {
            return (None, 0);
        }
        let view_w = clip.2 - clip.0;
        let max_x = (content_w - view_w).max(0);
        if view_w <= 0 || max_x == 0 {
            return (None, 0); // one page — nothing to turn
        }
        let now = nsec_now();
        let Some(turn) = crate::pager_math::page_turn(
            self.pager.target() as i32,
            view_w,
            max_x,
            delta_notches,
            now < self.pager_lock_until_ns,
        ) else {
            return (None, 0);
        };
        self.pager.set_extent(view_w as f32, content_w as f32);
        let glide = turn.target_px as f32 - self.pager.target();
        let _ = self.pager.scroll_wheel(glide);
        self.momentum_last_ns = now;
        self.pager_lock_until_ns = now + 360_000_000;
        // First eased step within THIS frame (same contract as the vertical
        // impulse).
        let (span, _) = self.pager_step(clip, max_x);
        (span, turn.dir)
    }

    /// Glide the pager to an absolute page index (the store's `SetPage` dot
    /// tap, via the `svc.shell.scrollToPage` effect). NO trigger fires for
    /// this move — the store already knows the page it asked for (firing
    /// would double-count).
    pub(super) fn pager_scroll_to(&mut self, page: i32) {
        let Some((clip, content_w, _ch, axis)) = self.scroll_region_axis() else {
            return;
        };
        if axis != nexus_layout_types::ScrollAxis::Paged {
            return;
        }
        let view_w = clip.2 - clip.0;
        let max_x = (content_w - view_w).max(0);
        if view_w <= 0 {
            return;
        }
        self.pager.set_extent(view_w as f32, content_w as f32);
        let target_px = crate::pager_math::page_target_px(page, view_w, max_x) as f32;
        let glide = target_px - self.pager.target();
        let _ = self.pager.scroll_wheel(glide);
        self.momentum_last_ns = nsec_now();
    }

    /// Apply the pager physics position to the paint offset. The span is the
    /// viewport rows (the glide repaints the whole pager strip).
    pub(super) fn pager_step(
        &mut self,
        clip: (i32, i32, i32, i32),
        max_x: i32,
    ) -> (Option<(i32, i32)>, bool) {
        let pos = self.pager.offset_px().clamp(0, max_x);
        if pos == self.scroll_x {
            return (None, false);
        }
        self.scroll_x = pos;
        let span = (clip.1.max(0), clip.3.min(self.h as i32));
        (Some(span), false)
    }

    /// Dispatches the page's declarative `on WindowsChanged` handler
    /// (RFC-0086): the compositor's window set moved, so the shell re-runs
    /// its enumerate effect and picks up the merged running/minimized/focused
    /// flags. Container-scoped BY NAME, like `EndReached` — "the windows
    /// changed" has no pixel to hit-test.
    pub(super) fn fire_windows_changed(&mut self) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let damage = self
            .view
            .fire_trigger(tokens, &device, &locale, &mut self.host, "WindowsChanged")
            .ok()
            .flatten();
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
        true
    }

    /// Dispatches the pager container's declarative `on PageNext`/`on
    /// PagePrev` handler (container-scoped, BY NAME — the `EndReached`
    /// contract) so the store's page index follows a wheel-turned page.
    /// Returns whether the model changed (caller full-repaints).
    pub(super) fn fire_pager_trigger(&mut self, next: bool) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let name = if next { "PageNext" } else { "PagePrev" };
        let damage =
            self.view.fire_trigger(tokens, &device, &locale, &mut self.host, name).ok().flatten();
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
        true
    }

    /// Dispatches the declarative `on EndReached` handler of the scroll
    /// container (lazy loading: the app decides what "more" means — e.g.
    /// `dispatch(LoadMore)` continuing a QuerySpec page token). Returns
    /// whether the model changed (caller full-repaints like a tap).
    pub(super) fn fire_end_reached(&mut self) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        // Container-scoped event: dispatched by NAME, never by hit-test —
        // the handler may sit on a (scrolled-away) content node, and "the
        // end was reached" has no pixel anyway.
        let damage = self
            .view
            .fire_trigger(tokens, &device, &locale, &mut self.host, "EndReached")
            .ok()
            .flatten();
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
        true
    }

    /// Re-run layout for the CURRENT scene (model changed) and reconcile
    /// scroll state: offsets clamp to the new content, the EndReached
    /// latch re-arms. Shared by tap/EndReached layout damage.
    pub(super) fn relayout_retained(&mut self) {
        let engine = nexus_layout::LayoutEngine::new();
        let Ok(layout) = engine.layout_with_viewport(
            self.view.scene(),
            nexus_layout_types::FxPx::new(self.w as i32),
            Some(nexus_layout_types::FxPx::new(self.h as i32)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        ) else {
            return;
        };
        self.layout = layout;
        self.texts.clear();
        collect_texts(self.view.scene(), &mut 0, &mut self.texts);
        // Store-window proof: with `tail(messages, 256)` the resident text
        // run count stays bounded no matter how many pages are loaded —
        // without the cap this grew unbounded and OOM'd the bump heap.
        {
            let mut m = alloc::string::String::new();
            let _ = core::fmt::write(
                &mut m,
                format_args!("apphost: scroll window texts={}", self.texts.len()),
            );
            raw_marker(&m);
        }
        self.end_fired = false;
        if let Some((clip, content_w, content_h, axis)) = self.scroll_region_axis() {
            let view_w = clip.2 - clip.0;
            let view_h = clip.3 - clip.1;
            let max_x = (content_w - view_w).max(0);
            let max_y = (content_h - view_h).max(0);
            self.scroll_x = self.scroll_x.clamp(0, max_x);
            self.scroll_y = self.scroll_y.clamp(0, max_y);
            // Content grew/shrank: the physics keeps position + target
            // (set_extent re-clamps both) so a LoadMore append continues
            // the ease seamlessly instead of snapping.
            self.momentum.set_extent(view_h as f32, content_h as f32);
            if axis == nexus_layout_types::ScrollAxis::Paged {
                // Pager: keep the glide consistent with the (possibly
                // resized) page strip — extent re-clamps position + target.
                self.pager.set_extent(view_w as f32, content_w as f32);
            }
        } else {
            self.scroll_x = 0;
            self.scroll_y = 0;
            self.pager.set_offset(0.0);
        }
        // A relayout follows a re-emit (tap Layout damage / EndReached
        // LoadMore): reconcile the animation driver with the new intents.
        // Idempotent when the tap path also calls it (values already `seen`).
        self.anim_sync();
    }

    /// Compositor-owned scroll position push (`INPUT_KIND_SCROLL_POS`):
    /// windowd is the scroll authority, so mirror the pushed ABSOLUTE offset
    /// into `scroll_y` (keeps tap hit-testing + the EndReached lazy-load
    /// check correct) WITHOUT re-rendering. Returns `true` only when the
    /// near-end check fired the declarative `EndReached` and the model
    /// changed (LoadMore) — the caller re-renders the tall band + re-presents.
    pub(super) fn scroll_pos(&mut self, rows: i32) -> bool {
        let Some((clip, _cw, content_h)) = self.scroll_region() else {
            return false;
        };
        let view_h = clip.3 - clip.1;
        let max_y = (content_h - view_h).max(0);
        self.scroll_y = rows.clamp(0, max_y);
        let near_end = max_y > 0 && self.scroll_y >= max_y - view_h / 2;
        if near_end && !self.end_fired {
            self.end_fired = true;
            return self.fire_end_reached();
        }
        false
    }
}
