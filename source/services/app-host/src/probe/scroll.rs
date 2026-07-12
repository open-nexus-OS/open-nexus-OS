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
    pub(super) fn scroll_region_axis(
        &self,
    ) -> Option<((i32, i32, i32, i32), i32, i32, nexus_layout_types::ScrollAxis)> {
        let mut clip: Option<(i32, i32, i32, i32)> = None;
        let mut axis = nexus_layout_types::ScrollAxis::Vertical;
        let (mut content_r, mut content_b) = (0i32, 0i32);
        for b in &self.layout.boxes {
            if let nexus_layout_types::Overflow::Scroll(a) = b.overflow {
                axis = a;
            }
            let Some(c) = b.clip_rect else { continue };
            if clip.is_none() {
                clip = Some((c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0));
            }
            content_r = content_r.max(b.rect.x.0 + b.rect.width.0);
            content_b = content_b.max(b.rect.y.0 + b.rect.height.0);
        }
        let clip = clip?;
        Some((clip, content_r - clip.0, content_b - clip.1, axis))
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
        // Horizontal viewports (launcher pages) stay direct-stepped v1.
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

    /// Advance the vertical scroll physics by real elapsed time and apply
    /// the eased position. Returns the viewport repaint span while moving.
    pub(super) fn momentum_tick(&mut self) -> (Option<(i32, i32)>, bool) {
        if !self.momentum.is_animating() {
            return (None, false);
        }
        let Some((clip, _, content_h)) = self.scroll_region() else {
            return (None, false);
        };
        let view_h = clip.3 - clip.1;
        let max_y = (content_h - view_h).max(0);
        let now = nsec_now();
        let dt = now.saturating_sub(self.momentum_last_ns).min(100_000_000);
        self.momentum_last_ns = now;
        let _ = self.momentum.tick(dt);
        self.momentum_step(clip, max_y, view_h)
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
        self.momentum.is_animating()
    }

    /// Dispatches the declarative `on EndReached` handler of the scroll
    /// container (lazy loading: the app decides what "more" means — e.g.
    /// `dispatch(LoadMore)` continuing a QuerySpec page token). Returns
    /// whether the model changed (caller full-repaints like a tap).
    pub(super) fn fire_end_reached(&mut self) -> bool {
        use nexus_dsl_runtime::{Damage, IdentityLocale};
        let tokens = tokens_for(self.theme_mode);
        let device = device_for(self.shell_profile);
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
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
        if let Some((clip, content_w, content_h)) = self.scroll_region() {
            let view_h = clip.3 - clip.1;
            let max_x = (content_w - (clip.2 - clip.0)).max(0);
            let max_y = (content_h - view_h).max(0);
            self.scroll_x = self.scroll_x.clamp(0, max_x);
            self.scroll_y = self.scroll_y.clamp(0, max_y);
            // Content grew/shrank: the physics keeps position + target
            // (set_extent re-clamps both) so a LoadMore append continues
            // the ease seamlessly instead of snapping.
            self.momentum.set_extent(view_h as f32, content_h as f32);
        } else {
            self.scroll_x = 0;
            self.scroll_y = 0;
        }
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
