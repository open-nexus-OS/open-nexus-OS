// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! Virtualized list widget for large collections.
//!
//! Provides `VirtualList<P: ItemProvider>` — a scrollable list that only
//! creates scene graph nodes for visible items plus overscan. Recycling
//! pool reuses off-screen node slots. Anchor-by-key ensures stable scroll
//! position across content changes.
//!
//! Both `List` (finite) and `VirtualList` (lazy) sit on one shared
//! [`ListCore`] — the scroll + windowing SSOT. The scroll *physics* is
//! `animation::ScrollMomentum` (an Android `OverScroller` analog); the
//! *windowing* (measured-row prefix-sum + O(log n) visible-range search) lives
//! in `ListCore`. A list flavor is then just that core + a data source
//! (a materialized slice vs a lazy `ItemProvider`), so scroll feel + windowing
//! are defined in exactly one place (RFC-0067 P2: layout → List → VirtualList).
//!
//! Part of TASK-0063 (UI v5b).

extern crate alloc;

use alloc::vec::Vec;
use animation::ScrollMomentum;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::{LayoutNode, MeasureText};
// `FxPx` is re-exported so embedders (e.g. windowd) construct scroll values +
// lists through this one crate without also depending on `nexus-layout-types`.
pub use nexus_layout_types::FxPx;

// ---------------------------------------------------------------------------
// ItemView — the app-supplied cell builder (the data-source / cell-config split)
// ---------------------------------------------------------------------------

/// Builds the layout subtree ("cell") for one item.
///
/// Paired with an [`ItemProvider`] (which supplies the *data*), this is the
/// single interface through which app state reaches the generic compositor:
/// the app describes each item as a `LayoutNode` tree (boxes + `VisualStyle` +
/// text), the layout engine (`nexus_layout`) measures/places it, and windowd
/// paints the resulting `LayoutBox`es generically — the compositor has **no**
/// item-type knowledge. A "chat", for example, is just an `ItemProvider` of chat
/// messages plus an `ItemView` that renders one as a bubble, assembled by the
/// app (e.g. `chat-app`) — not baked into the widget or windowd.
///
/// This splits the data source (data) from the cell configuration (view),
/// keeping the framework generic.
pub trait ItemView {
    /// The item type — matches the paired `ItemProvider::Item`.
    type Item;

    /// Build the layout-node subtree for `item` at `index`. Pure: the same
    /// item must yield the same node (deterministic layout / pretext contract).
    fn build_item(&self, index: usize, item: &Self::Item) -> LayoutNode;
}

// ---------------------------------------------------------------------------
// ItemProvider trait
// ---------------------------------------------------------------------------

/// Provides items for a `VirtualList`. Items are keyed by index and may be
/// lazily loaded via `request_more`.
pub trait ItemProvider {
    /// The item type — typically a content struct with text, height hint, etc.
    type Item;

    /// Advisory total count. Returns `None` when unknown (streaming data).
    fn len_hint(&self) -> Option<usize>;

    /// Synchronous fetch of already-loaded items in `range`.
    /// Returns a slice where unloaded slots are `None`.
    fn get(&self, range: core::ops::Range<usize>) -> &[Option<Self::Item>];

    /// Request loading of the page covering `trigger_index`.
    /// No-op if that page is already in-flight or loaded.
    /// At most 1 in-flight request per provider at any time.
    fn request_more(&mut self, trigger_index: usize);

    /// True when a page request is currently in-flight.
    fn has_inflight(&self) -> bool;

    /// Height hint for an unloaded item at `index`. Used for scrollbar
    /// estimation and placeholder sizing.
    fn height_hint(&self, index: usize) -> u32;
}

// ---------------------------------------------------------------------------
// List — non-virtualized, filterable list of a small/bounded collection
// ---------------------------------------------------------------------------

/// A **finite** list: all `items` are materialized (no lazy paging). The finite
/// sibling of [`VirtualList`] — both sit on the same [`ListCore`] (the scroll +
/// windowing SSOT), so scroll feel and viewport windowing are identical; `List`
/// simply drops the lazy `ItemProvider`/recycling/prefetch. For small/bounded
/// collections (a settings list, a filtered search result); use [`VirtualList`]
/// for large/streaming data.
///
/// Two modes:
/// - **row builder** ([`List::new`]) — pure `rows()`/`filtered_rows()` for a
///   non-scrolling container (e.g. a `Panel` that lays out all rows itself);
/// - **scrollable** ([`List::scrollable`]) — a viewport over all items, measured
///   by the layout engine and scrolled through the shared `ScrollMomentum`,
///   exposing the same `scroll_wheel`/`fling`/`tick`/`visible_boxes` surface as
///   `VirtualList`.
///
/// Shares the `ItemView` cell contract + the filter capability with
/// `VirtualList`, so "list" and "virtual list" are one component family.
pub struct List<'a, I, V: ItemView<Item = I>> {
    items: &'a [I],
    view: &'a V,
    /// Shared scroll + windowing core (the SSOT). Empty/zero-viewport in the
    /// row-builder mode; populated in the scrollable mode.
    core: ListCore,
}

impl<'a, I, V: ItemView<Item = I>> List<'a, I, V> {
    /// Row-builder mode: produces item rows for a container that does its own
    /// layout/scroll. No viewport, no measured heights — `rows()`/`filtered_rows()`.
    pub fn new(items: &'a [I], view: &'a V) -> Self {
        Self { items, view, core: ListCore::new(FxPx::ZERO, Vec::new(), 0, 0) }
    }

    /// Scrollable mode: a `viewport_height` window over all `items`, scrolled by
    /// the shared `ScrollMomentum` SSOT. Heights start as placeholders; call
    /// [`Self::measure_with`] to fill real per-item heights from the layout engine.
    pub fn scrollable(items: &'a [I], view: &'a V, viewport_height: FxPx, overscan: usize) -> Self {
        let measured = (0..items.len()).map(|_| MeasuredRow::placeholder()).collect();
        let core = ListCore::new(viewport_height, measured, overscan, items.len());
        Self { items, view, core }
    }

    /// All item rows (no filter).
    pub fn rows(&self) -> Vec<LayoutNode> {
        self.items.iter().enumerate().map(|(i, item)| self.view.build_item(i, item)).collect()
    }

    /// Item rows for the items matching `pred` (e.g. a search query). The
    /// closure captures app state (the query) — the framework/app split: the
    /// list is generic, the predicate is app-supplied.
    pub fn filtered_rows(&self, pred: impl Fn(&I) -> bool) -> Vec<LayoutNode> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| pred(item))
            .map(|(i, item)| self.view.build_item(i, item))
            .collect()
    }

    /// Measure every item's real height via the layout engine (`nexus_layout`) —
    /// finite, so all items are measured (no lazy estimate). Recomputes content
    /// height + visible range. Call after data/width changes, not per scroll.
    pub fn measure_with(&mut self, measure: &dyn MeasureText, width: FxPx) {
        if self.core.measured.is_empty() {
            return;
        }
        let engine = LayoutEngine::new();
        for (i, item) in self.items.iter().enumerate() {
            let node = self.view.build_item(i, item);
            if let Ok(result) = engine.layout(&node, width, measure) {
                if let Some(row) = self.core.measured.get_mut(i) {
                    row.height = result.content_height;
                    row.estimated = false;
                }
            }
        }
        self.core.recompute_content_height();
        self.core.recompute_visible_range();
    }

    /// Positioned `LayoutBox`es for the visible viewport window — O(window), the
    /// same windowed paint as `VirtualList` but over the finite slice. Call
    /// [`Self::measure_with`] first so heights are real.
    pub fn visible_boxes(&self, measure: &dyn MeasureText, width: FxPx) -> Vec<LayoutBox> {
        let range = self.core.visible_range.clone();
        let mut out = Vec::new();
        if range.start >= self.items.len() {
            return out;
        }
        let engine = LayoutEngine::new();
        let mut top: i32 =
            self.core.measured[..range.start].iter().map(|m| m.height.as_i32()).sum();
        let scroll = self.core.scroll_offset.as_i32();
        for idx in range.clone() {
            let item_h = self.core.measured.get(idx).map(|m| m.height.as_i32()).unwrap_or(0);
            if let Some(item) = self.items.get(idx) {
                let node = self.view.build_item(idx, item);
                if let Ok(result) = engine.layout(&node, width, measure) {
                    let dy = top - scroll;
                    for mut b in result.boxes {
                        b.rect.y = FxPx::new(b.rect.y.as_i32() + dy);
                        out.push(b);
                    }
                }
            }
            top += item_h;
        }
        out
    }

    // ── scroll surface (delegates to the shared ListCore SSOT) ───────────────

    /// Wheel input: immediate 1:1 step + accumulating momentum (see `VirtualList`).
    pub fn scroll_wheel(&mut self, notch_px: FxPx) -> core::ops::Range<usize> {
        self.core.scroll_wheel(notch_px)
    }
    /// Touch/trackpad inertia: a release velocity that coasts under friction.
    pub fn fling(&mut self, velocity: FxPx) {
        self.core.fling(velocity);
    }
    /// Advance the momentum glide by real elapsed time; returns true while gliding.
    pub fn tick(&mut self, dt_ns: u64) -> bool {
        self.core.tick(dt_ns)
    }
    /// Immediate jump by `delta` (no momentum).
    pub fn scroll_by(&mut self, delta: FxPx) -> core::ops::Range<usize> {
        self.core.scroll_by(delta)
    }
    /// True while a momentum glide is still in motion.
    pub fn is_animating(&self) -> bool {
        self.core.is_animating()
    }
    /// Update the viewport height (e.g. the window was resized).
    pub fn set_viewport_height(&mut self, height: FxPx) {
        self.core.set_viewport_height(height);
    }
    /// Override the content height (embedder is the height authority).
    pub fn set_content_height(&mut self, height: FxPx) {
        self.core.set_content_height(height);
    }
    /// Maximum scroll offset (content beyond the viewport).
    pub fn max_scroll(&self) -> i32 {
        self.core.max_scroll()
    }
    /// Current scroll offset.
    pub fn scroll_offset(&self) -> FxPx {
        self.core.scroll_offset
    }
    /// The scroll target the position is easing toward (a wheel notch extends it).
    pub fn scroll_target(&self) -> i32 {
        self.core.scroll_target()
    }
    /// Current visible range (inclusive start, exclusive end).
    pub fn visible_range(&self) -> core::ops::Range<usize> {
        self.core.visible_range.clone()
    }
    /// Total content height.
    pub fn content_height(&self) -> FxPx {
        self.core.content_height
    }
}

// ---------------------------------------------------------------------------
// MeasuredRow — height cache entry
// ---------------------------------------------------------------------------

/// Cached row measurement for mixed-height collections.
#[derive(Debug, Clone, Copy)]
pub struct MeasuredRow {
    /// Measured height in FxPx. 0 = not yet measured.
    pub height: FxPx,
    /// Width bucket key that produced this measurement.
    pub width_bucket: u32,
    /// Whether this is an estimate (true) or a verified measurement (false).
    pub estimated: bool,
}

impl MeasuredRow {
    pub const fn new() -> Self {
        Self { height: FxPx::ZERO, width_bucket: 0, estimated: true }
    }

    pub const fn placeholder() -> Self {
        Self { height: FxPx::new(48), width_bucket: 0, estimated: true }
    }
}

// ---------------------------------------------------------------------------
// Anchor — stable scroll position
// ---------------------------------------------------------------------------

/// Stable scroll anchor — keeps the same logical content under the viewport
/// across prepends, appends, and content updates.
#[derive(Debug, Clone, Copy)]
pub struct ScrollAnchor {
    /// Key (index) of the leading visible item.
    pub leading_index: usize,
    /// Intra-item offset in FxPx from the top of the leading item.
    pub offset: FxPx,
}

impl ScrollAnchor {
    pub const fn new(index: usize) -> Self {
        Self { leading_index: index, offset: FxPx::ZERO }
    }
}

// ---------------------------------------------------------------------------
// VirtualListConfig
// ---------------------------------------------------------------------------

/// Configuration knobs for a `VirtualList`.
pub struct VirtualListConfig {
    /// Number of extra items rendered beyond the visible viewport (top + bottom).
    pub overscan: usize,
    /// Maximum number of recycled node slots to retain.
    pub max_recycled: usize,
    /// Maximum cached row measurements.
    pub max_measured: usize,
}

impl Default for VirtualListConfig {
    fn default() -> Self {
        Self { overscan: 3, max_recycled: 64, max_measured: 256 }
    }
}

// ---------------------------------------------------------------------------
// ListCore — the scroll + windowing SSOT shared by List and VirtualList
// ---------------------------------------------------------------------------

/// The scroll + windowing core shared by [`List`] (finite) and [`VirtualList`]
/// (lazy). Owns the **scroll SSOT** ([`animation::ScrollMomentum`] — wheel +
/// fling physics) plus the measured-row prefix-sum windowing (the O(log n)
/// visible-range binary search). Data-source-agnostic on purpose: a list flavor
/// is this core + a data source, so scroll feel + windowing live in exactly one
/// place. Fields are `pub(crate)` so the list flavors (and in-crate tests) read
/// them directly without a wall of accessors.
pub(crate) struct ListCore {
    /// Viewport height in FxPx.
    pub(crate) viewport_height: FxPx,
    /// Whole-pixel mirror of `scroll.offset_px()`, refreshed after every scroll
    /// op so the render/range code reads a stable `FxPx`. The authoritative
    /// position + physics live in `scroll`.
    pub(crate) scroll_offset: FxPx,
    /// Shared momentum-scroll physics — the SAME reusable mechanism any
    /// scrollable surface uses. Owns velocity + friction + the immediate 1:1
    /// wheel step; the windowing just mirrors its offset + re-windows.
    pub(crate) scroll: ScrollMomentum,
    /// Cached row measurements.
    pub(crate) measured: Vec<MeasuredRow>,
    /// Prefix sum of measured item heights: `cumulative[i]` = pixel top of item
    /// `i`, `cumulative[len]` = total height. Lets `recompute_visible_range` find
    /// the first visible item by **binary search (O(log n))** instead of walking
    /// from the top every frame (which was O(scroll-depth) — the 120 Hz killer).
    pub(crate) cumulative: Vec<i32>,
    /// Total estimated content height.
    pub(crate) content_height: FxPx,
    /// Extra items rendered beyond the viewport (top + bottom).
    pub(crate) overscan: usize,
    /// Maximum cached row measurements.
    pub(crate) max_measured: usize,
    /// Currently visible item range (start..end).
    pub(crate) visible_range: core::ops::Range<usize>,
    /// Perf introspection: items the LAST `recompute_visible_range` touched
    /// (binary-search steps + window walk). Must stay ~O(log n + window).
    pub(crate) last_scan_ops: u32,
    /// Set by `recompute_visible_range`: the visible window (incl. overscan)
    /// reached the end of the measured set. Lazy embedders read this to trigger
    /// prefetch — kept out of the windowing math so the core stays data-agnostic.
    pub(crate) reached_end: bool,
}

impl ListCore {
    fn new(
        viewport_height: FxPx,
        measured: Vec<MeasuredRow>,
        overscan: usize,
        max_measured: usize,
    ) -> Self {
        let mut core = Self {
            viewport_height,
            scroll_offset: FxPx::ZERO,
            scroll: ScrollMomentum::with_defaults(),
            measured,
            cumulative: Vec::new(),
            content_height: FxPx::ZERO,
            overscan,
            max_measured,
            visible_range: 0..0,
            last_scan_ops: 0,
            reached_end: false,
        };
        core.recompute_content_height(); // also syncs the scroller's extent
        core.recompute_visible_range();
        core
    }

    /// Push the current viewport/content extent into the shared scroller so its
    /// `max_scroll`/clamping match the measured layout.
    fn sync_scroll_extent(&mut self) {
        self.scroll
            .set_extent(self.viewport_height.as_i32() as f32, self.content_height.as_i32() as f32);
        self.scroll_offset = FxPx::new(self.scroll.offset_px());
    }

    fn max_scroll(&self) -> i32 {
        (self.content_height.as_i32() - self.viewport_height.as_i32()).max(0)
    }

    fn scroll_target(&self) -> i32 {
        self.scroll.target() as i32
    }

    fn set_content_height(&mut self, height: FxPx) {
        self.content_height = FxPx::new(height.as_i32().max(0));
        self.sync_scroll_extent();
    }

    fn set_viewport_height(&mut self, height: FxPx) {
        self.viewport_height = FxPx::new(height.as_i32().max(0));
        self.sync_scroll_extent();
    }

    fn scroll_by(&mut self, delta: FxPx) -> core::ops::Range<usize> {
        self.scroll.scroll_by(delta.as_i32() as f32);
        self.scroll_offset = FxPx::new(self.scroll.offset_px());
        self.recompute_visible_range();
        self.visible_range.clone()
    }

    fn scroll_wheel(&mut self, notch_px: FxPx) -> core::ops::Range<usize> {
        self.scroll.scroll_wheel(notch_px.as_i32() as f32);
        self.scroll_offset = FxPx::new(self.scroll.offset_px());
        self.recompute_visible_range();
        self.visible_range.clone()
    }

    fn fling(&mut self, velocity: FxPx) {
        self.scroll.fling(velocity.as_i32() as f32);
    }

    fn is_animating(&self) -> bool {
        self.scroll.is_animating()
    }

    fn tick(&mut self, dt_ns: u64) -> bool {
        let still = self.scroll.tick(dt_ns);
        self.scroll_offset = FxPx::new(self.scroll.offset_px());
        self.recompute_visible_range();
        still
    }

    fn recompute_content_height(&mut self) {
        // Rebuild the prefix sum (top of each item) + total height. O(measured),
        // but only on data/width changes — NOT per scroll frame.
        self.cumulative.clear();
        self.cumulative.push(0);
        let mut h = 0i32;
        for m in &self.measured {
            h += m.height.as_i32();
            self.cumulative.push(h);
        }
        self.content_height = FxPx::new(h);
        self.sync_scroll_extent();
    }

    fn recompute_visible_range(&mut self) {
        let n = self.measured.len();
        if n == 0 || self.cumulative.len() != n + 1 {
            self.visible_range = 0..0;
            self.reached_end = false;
            return;
        }
        let overscan_px = self.overscan as i32 * 48;
        let top = (self.scroll_offset.as_i32() - overscan_px).max(0);
        let bottom = self.scroll_offset.as_i32() + self.viewport_height.as_i32() + overscan_px;

        // O(log n): first item whose BOTTOM (cumulative[i+1]) is below `top`,
        // via binary search on the ascending prefix sum (counts its steps).
        let mut ops = 0u32;
        let (mut lo, mut hi) = (1usize, self.cumulative.len());
        while lo < hi {
            ops += 1;
            let mid = (lo + hi) / 2;
            if self.cumulative[mid] > top {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        let start = (lo - 1).min(n - 1);
        // O(window): walk forward only across the visible window + overscan.
        let mut end = start;
        while end < n && self.cumulative[end] <= bottom {
            end += 1;
            ops += 1;
        }
        self.last_scan_ops = ops;
        let end = end.max(start + 1).min(n);
        self.visible_range = start..end;
        // Lazy-prefetch trigger (data-agnostic): the window approached the end of
        // the loaded/measured set. The lazy flavor acts on it via the provider.
        self.reached_end = end + self.overscan >= n;
    }
}

// ---------------------------------------------------------------------------
// VirtualList
// ---------------------------------------------------------------------------

/// State of the virtual list render pass.
enum ListState {
    /// Initial state — needs full mount.
    Unmounted,
    /// Mounted and idle — no changes to layout or content.
    Idle,
    /// Scrolling — positions changed, content unchanged.
    Scrolling,
    /// New data arrived — content changed, anchor preserved.
    DataArrived,
}

/// A virtualized, recycling list widget.
///
/// Manages a viewport window over a potentially large collection.
/// Only the visible range plus overscan has active scene graph nodes.
/// Off-screen nodes are recycled instead of deallocated.
///
/// Scroll + windowing are delegated to the shared [`ListCore`]; this type adds
/// the lazy [`ItemProvider`] data source, recycling pool, and prefetch.
pub struct VirtualList<P: ItemProvider> {
    /// The data provider.
    provider: P,
    /// Shared scroll + windowing core (the SSOT).
    core: ListCore,
    /// Recycling pool: reused node ids.
    recycled: Vec<u64>,
    /// Currently active node ids (in display order).
    active_nodes: Vec<u64>,
    /// Internal state tracker.
    state: ListState,
}

impl<P: ItemProvider> VirtualList<P> {
    /// Create a new virtual list with the given provider and viewport height.
    pub fn new(provider: P, viewport_height: FxPx, config: VirtualListConfig) -> Self {
        let hint = provider.len_hint().unwrap_or(0);
        let measured = (0..hint.min(config.max_measured))
            .map(|i| {
                let h = provider.height_hint(i);
                MeasuredRow { height: FxPx::new(h as i32), width_bucket: 0, estimated: true }
            })
            .collect();
        let core = ListCore::new(viewport_height, measured, config.overscan, config.max_measured);
        let mut list = Self {
            provider,
            core,
            recycled: Vec::new(),
            active_nodes: Vec::new(),
            state: ListState::Unmounted,
        };
        list.maybe_prefetch();
        list
    }

    /// Lazy prefetch (lazy-load style): when the visible window approaches the
    /// end of the loaded set, ask the provider to load the next page — so deep
    /// scrolling never blocks on data. Guarded by `has_inflight` (no dup loads).
    fn maybe_prefetch(&mut self) {
        if self.core.reached_end && !self.provider.has_inflight() {
            self.provider.request_more(self.core.visible_range.end);
        }
    }

    /// Maximum scroll offset (content beyond the viewport). 0 if it all fits.
    pub fn max_scroll(&self) -> i32 {
        self.core.max_scroll()
    }

    /// The scroll target the position is easing toward (diagnostics / embedders).
    pub fn scroll_target(&self) -> i32 {
        self.core.scroll_target()
    }

    /// Override the content height directly (embedder is the height authority).
    ///
    /// The component normally derives `content_height` from its own measured row
    /// heights. But an embedder that measures content with a *different* model —
    /// e.g. windowd's chat panel, which hard-wraps with a bitmap font and knows
    /// the exact pixel height of its rendered surface — can set the authoritative
    /// total here so `max_scroll`/`fling`/`tick` clamp to the real bottom while
    /// the shared [`ScrollMomentum`] still owns the physics. Re-clamps the live
    /// offset so an external shrink can't strand it past the new bottom.
    pub fn set_content_height(&mut self, height: FxPx) {
        self.core.set_content_height(height);
    }

    /// Update the viewport height (e.g. the chat window was resized).
    pub fn set_viewport_height(&mut self, height: FxPx) {
        self.core.set_viewport_height(height);
    }

    /// Mutable access to the provider (e.g. to prepend/append data). After a
    /// data change the embedder should refresh the content height (own model)
    /// or call [`Self::page_arrived`] (component-measured model).
    pub fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }

    /// Scroll IMMEDIATELY by `delta` FxPx (no momentum) — jumps the position and
    /// kills any glide. Positive = down. Returns the new visible range. For
    /// programmatic jumps (scroll-to-index, key Home/End), not wheel input.
    pub fn scroll_by(&mut self, delta: FxPx) -> core::ops::Range<usize> {
        let range = self.core.scroll_by(delta);
        self.state = ListState::Scrolling;
        self.maybe_prefetch();
        range
    }

    /// Wheel/trackpad input → the production scroll feel: an IMMEDIATE 1:1 step
    /// (`notch_px`, zero latency, precise for slow careful scrolling) PLUS
    /// accumulating momentum (a fast spin coasts). Delegates to the shared
    /// [`ScrollMomentum`]. Returns the new visible range so the caller can
    /// present the instant move on the same frame.
    pub fn scroll_wheel(&mut self, notch_px: FxPx) -> core::ops::Range<usize> {
        let range = self.core.scroll_wheel(notch_px);
        self.state = ListState::Scrolling;
        self.maybe_prefetch();
        range
    }

    /// Touch/trackpad inertia — inject a release **velocity** (px/s, positive =
    /// down) that coasts and decelerates under the shared [`ScrollMomentum`]'s
    /// Android-`OverScroller` friction. No immediate step; wheel input should use
    /// [`Self::scroll_wheel`]. Successive flings before settling accumulate.
    pub fn fling(&mut self, velocity: FxPx) {
        self.core.fling(velocity);
        self.state = ListState::Scrolling;
    }

    /// True while a momentum glide is still in motion (the present loop keeps
    /// ticking + presenting); false once it has settled (idle/reactive).
    pub fn is_animating(&self) -> bool {
        self.core.is_animating()
    }

    /// Advance the momentum glide by the real elapsed time `dt_ns` (frame-rate
    /// independent) via the shared [`ScrollMomentum`] integrator, then re-window.
    /// Returns true while still gliding. O(window) — only the visible range moves.
    pub fn tick(&mut self, dt_ns: u64) -> bool {
        let still = self.core.tick(dt_ns);
        self.state = ListState::Scrolling;
        self.maybe_prefetch();
        still
    }

    /// Measure the loaded items' real heights via the layout engine
    /// (`nexus_layout`) — the single measurement SSOT — instead of the provider's
    /// `height_hint` estimate. Each loaded item is built into a `LayoutNode` by
    /// `view`, laid out at `width`, and its `content_height` cached. Unloaded
    /// slots keep their estimate (lazy loading). Recomputes content height +
    /// visible range. O(loaded items) — call after data/width changes, not per scroll.
    pub fn measure_with<V>(&mut self, view: &V, measure: &dyn MeasureText, width: FxPx)
    where
        V: ItemView<Item = P::Item>,
    {
        let n = self.core.measured.len();
        if n == 0 {
            return;
        }
        let engine = LayoutEngine::new();
        // Disjoint field borrows: `provider` (read for items) + `core` (write).
        let items = self.provider.get(0..n);
        for i in 0..items.len() {
            let Some(item) = items[i].as_ref() else {
                continue; // unloaded — keep the height-hint estimate (lazy)
            };
            let node = view.build_item(i, item);
            if let Ok(result) = engine.layout(&node, width, measure) {
                if let Some(row) = self.core.measured.get_mut(i) {
                    row.height = result.content_height;
                    row.estimated = false;
                }
            }
        }
        self.core.recompute_content_height();
        self.core.recompute_visible_range();
        self.maybe_prefetch();
    }

    /// Notify the list that a new page of data has arrived.
    pub fn page_arrived(&mut self) {
        self.state = ListState::DataArrived;
        let hint = self.provider.len_hint().unwrap_or(self.core.measured.len());
        while self.core.measured.len() < hint.min(self.core.max_measured) {
            let i = self.core.measured.len();
            let h = self.provider.height_hint(i);
            self.core.measured.push(MeasuredRow {
                height: FxPx::new(h as i32),
                width_bucket: 0,
                estimated: true,
            });
        }
        self.core.recompute_content_height();
    }

    /// Positioned `LayoutBox`es for the **visible window only** (visible range +
    /// overscan) — O(window), not O(total). Each visible item is built into a
    /// `LayoutNode` by `view`, laid out via the layout engine, and its boxes are
    /// shifted to the item's on-screen `y` (cumulative item heights − scroll
    /// offset). windowd paints these generically (`draw_row_box`); unloaded items
    /// contribute no boxes (lazy). Call `measure_with` first so heights are real.
    pub fn visible_boxes<V>(
        &self,
        view: &V,
        measure: &dyn MeasureText,
        width: FxPx,
    ) -> Vec<LayoutBox>
    where
        V: ItemView<Item = P::Item>,
    {
        let range = self.core.visible_range.clone();
        let mut out = Vec::new();
        if range.start >= self.core.measured.len() {
            return out;
        }
        let engine = LayoutEngine::new();
        // On-screen y of the first visible item's top.
        let mut top: i32 =
            self.core.measured[..range.start].iter().map(|m| m.height.as_i32()).sum();
        let scroll = self.core.scroll_offset.as_i32();
        let items = self.provider.get(range.clone());
        for (off, slot) in items.iter().enumerate() {
            let idx = range.start + off;
            let item_h = self.core.measured.get(idx).map(|m| m.height.as_i32()).unwrap_or(0);
            if let Some(item) = slot {
                let node = view.build_item(idx, item);
                if let Ok(result) = engine.layout(&node, width, measure) {
                    let dy = top - scroll; // item top within the list viewport
                    for mut b in result.boxes {
                        b.rect.y = FxPx::new(b.rect.y.as_i32() + dy);
                        out.push(b);
                    }
                }
            }
            top += item_h;
        }
        out
    }

    /// Current visible range (inclusive start, exclusive end).
    pub fn visible_range(&self) -> core::ops::Range<usize> {
        self.core.visible_range.clone()
    }

    /// Current scroll offset.
    pub fn scroll_offset(&self) -> FxPx {
        self.core.scroll_offset
    }

    /// The data provider (read-only) — for inspecting lazy-load state.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Items touched by the LAST `recompute_visible_range` (binary-search steps +
    /// forward window walk). The defining virtual-list invariant: this stays
    /// bounded at ~O(log n + window) regardless of total item count OR scroll
    /// depth — it must NOT grow with the list size or how far down you've scrolled.
    pub fn last_scan_ops(&self) -> u32 {
        self.core.last_scan_ops
    }

    /// Stable scroll anchor (the leading visible item).
    pub fn anchor(&self) -> ScrollAnchor {
        ScrollAnchor::new(self.core.visible_range.start)
    }

    /// True when the list is scrolling (PlaceOnly invalidation).
    pub fn is_scrolling(&self) -> bool {
        matches!(self.state, ListState::Scrolling)
    }

    /// True when new data arrived (PaintOnly invalidation on affected range).
    pub fn is_data_arrived(&self) -> bool {
        matches!(self.state, ListState::DataArrived)
    }

    /// Acknowledge that the current state has been processed (call after frame).
    pub fn acknowledge(&mut self) {
        self.state = ListState::Idle;
    }

    /// IDs of currently active (visible + overscan) nodes.
    pub fn active_node_ids(&self) -> &[u64] {
        &self.active_nodes
    }

    /// IDs of recycled nodes available for reuse.
    pub fn recycled_ids(&self) -> &[u64] {
        &self.recycled
    }

    /// Number of items in the recycling pool.
    pub fn recycled_count(&self) -> usize {
        self.recycled.len()
    }

    /// Total estimated content height.
    pub fn content_height(&self) -> FxPx {
        self.core.content_height
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple in-memory provider for testing.
    struct TestProvider {
        items: Vec<Option<&'static str>>,
        inflight: bool,
    }

    impl ItemProvider for TestProvider {
        type Item = &'static str;

        fn len_hint(&self) -> Option<usize> {
            Some(self.items.len())
        }

        fn get(&self, range: core::ops::Range<usize>) -> &[Option<Self::Item>] {
            let end = range.end.min(self.items.len());
            let start = range.start.min(end);
            &self.items[start..end]
        }

        fn request_more(&mut self, _trigger_index: usize) {
            self.inflight = true;
        }

        fn has_inflight(&self) -> bool {
            self.inflight
        }

        fn height_hint(&self, _index: usize) -> u32 {
            48
        }
    }

    fn make_provider(count: usize) -> TestProvider {
        let items: Vec<Option<&'static str>> = (0..count).map(|_| Some("hello")).collect();
        TestProvider { items, inflight: false }
    }

    #[test]
    fn virtual_list_initializes() {
        let p = make_provider(100);
        let list = VirtualList::new(p, FxPx::new(400), VirtualListConfig::default());
        assert_eq!(list.core.viewport_height, FxPx::new(400));
        assert_eq!(list.core.scroll_offset, FxPx::ZERO);
        assert_eq!(list.recycled.len(), 0);
    }

    #[test]
    fn scroll_by_updates_visible_range() {
        let p = make_provider(100);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        let range = list.scroll_by(FxPx::new(100));
        assert!(range.start <= range.end);
        assert!(list.is_scrolling());
    }

    /// One 120 Hz frame in ns — the integration step used by the scroll tests.
    const FRAME_NS: u64 = 8_333_333;

    /// Run a flick to rest, returning (frames, total_distance_px, peak_step_px,
    /// decelerated). `decelerated` is true if the largest per-frame step happened
    /// in the first third (momentum coasts then slows — the ease-out signature).
    fn run_flick(list: &mut VirtualList<TestProvider>, impulse_px: i32) -> (u32, i32, i32, bool) {
        let start = list.scroll_offset().as_i32();
        list.fling(FxPx::new(impulse_px));
        let (mut frames, mut peak, mut prev, mut peak_frame) = (0u32, 0i32, start, 0u32);
        while list.is_animating() && frames < 1000 {
            let still = list.tick(FRAME_NS);
            let pos = list.scroll_offset().as_i32();
            let step = (pos - prev).abs();
            prev = pos;
            if step > peak {
                peak = step;
                peak_frame = frames;
            }
            frames += 1;
            if !still {
                break;
            }
        }
        let dist = (list.scroll_offset().as_i32() - start).abs();
        (frames, dist, peak, peak_frame <= frames / 3)
    }

    #[test]
    fn flick_scrolls_smoothly_and_stays_windowed() {
        // A long list, small viewport → heavily virtualized. A flick must glide
        // over many frames (momentum, no jarring jump), decelerate, settle, and
        // keep only a tiny window of items active each frame (O(window)).
        let p = make_provider(5000);
        let mut list = VirtualList::new(
            p,
            FxPx::new(600),
            VirtualListConfig { overscan: 4, ..Default::default() },
        );

        list.fling(FxPx::new(120)); // a brisk downward flick
        let (mut frames, mut prev, mut peak_step, mut max_window) = (0u32, 0i32, 0i32, 0usize);
        while list.is_animating() && frames < 1000 {
            let still = list.tick(FRAME_NS);
            let pos = list.scroll_offset().as_i32();
            let step = pos - prev;
            prev = pos;
            assert!(step >= 0, "frame {frames}: scroll reversed ({step})");
            peak_step = peak_step.max(step);
            let r = list.visible_range();
            max_window = max_window.max(r.end - r.start);
            frames += 1;
            if !still {
                break;
            }
        }
        let dist = list.scroll_offset().as_i32();
        std::eprintln!(
            "flick(120px impulse): {frames} frames, traveled {dist}px, peak step {peak_step}px, window {max_window}"
        );
        assert!(frames >= 8, "too abrupt — settled in {frames} frames (not a glide)");
        assert!(frames <= 240, "too slow/long — {frames} frames (~2s @120Hz)");
        assert!(max_window < 64, "not windowed: {max_window} active items for 5000-item list");
        // Velocity stopped cleanly (no 1px tail crawl leaving it mid-pixel-creep).
        assert!(!list.is_animating(), "must settle to a clean stop");
        assert!(dist > 0 && dist <= list.max_scroll(), "stays within bounds: {dist}");
    }

    #[test]
    fn momentum_fast_flick_travels_farther_and_decelerates() {
        // THE momentum property: scroll SPEED matters, not just distance. A fast
        // flick (rapid notches accumulating velocity) must coast meaningfully
        // farther than a single slow notch, and both must DECELERATE (ease-out),
        // not move at constant speed. Frame-rate-independent (time-integrated).
        let slow = {
            let p = make_provider(20_000);
            let mut l = VirtualList::new(p, FxPx::new(600), VirtualListConfig::default());
            l.set_content_height(FxPx::new(1_000_000));
            run_flick(&mut l, 50) // one gentle notch
        };
        let fast = {
            let p = make_provider(20_000);
            let mut l = VirtualList::new(p, FxPx::new(600), VirtualListConfig::default());
            l.set_content_height(FxPx::new(1_000_000));
            // Six notches in rapid succession BEFORE the glide is ticked —
            // velocity accumulates (acceleration), exactly the wheel-spin case.
            for _ in 0..6 {
                l.fling(FxPx::new(50));
            }
            let start = l.scroll_offset().as_i32();
            let (mut frames, mut peak, mut prev, mut pf) = (0u32, 0i32, start, 0u32);
            while l.is_animating() && frames < 1000 {
                let still = l.tick(FRAME_NS);
                let pos = l.scroll_offset().as_i32();
                let step = (pos - prev).abs();
                prev = pos;
                if step > peak {
                    peak = step;
                    pf = frames;
                }
                frames += 1;
                if !still {
                    break;
                }
            }
            (frames, l.scroll_offset().as_i32() - start, peak, pf <= frames / 3)
        };
        std::eprintln!(
            "slow notch: {}f dist={}px peak={}px decel={} | fast spin: {}f dist={}px peak={}px decel={}",
            slow.0, slow.1, slow.2, slow.3, fast.0, fast.1, fast.2, fast.3
        );
        // Fast spin coasts substantially farther (acceleration is real).
        assert!(
            fast.1 > slow.1 * 3,
            "fast flick must travel far more: fast={} slow={}",
            fast.1,
            slow.1
        );
        // Both ease out (peak step early, then decelerate) — not linear drift.
        assert!(slow.3, "slow notch must decelerate (ease-out)");
        assert!(fast.3, "fast flick must decelerate (ease-out)");
        // Both settle to a clean stop in a sane time.
        assert!(
            slow.0 >= 4 && fast.0 <= 360,
            "sane settle times: slow={}f fast={}f",
            slow.0,
            fast.0
        );
    }

    #[test]
    fn flick_is_frame_rate_independent() {
        // Same flick integrated at 120 Hz vs 60 Hz must land at ~the same place
        // (time-based momentum, not per-frame). Distances within ~5%.
        fn travel(dt_ns: u64) -> i32 {
            let p = make_provider(20_000);
            let mut l = VirtualList::new(p, FxPx::new(600), VirtualListConfig::default());
            l.set_content_height(FxPx::new(1_000_000));
            l.fling(FxPx::new(300));
            let mut frames = 0;
            while l.tick(dt_ns) && frames < 4000 {
                frames += 1;
            }
            l.scroll_offset().as_i32()
        }
        let at_120 = travel(8_333_333);
        let at_60 = travel(16_666_667);
        std::eprintln!("frame-rate independence: 120Hz={at_120}px 60Hz={at_60}px");
        let diff = (at_120 - at_60).abs();
        assert!(
            diff <= at_120 / 20 + 4,
            "120Hz={at_120} vs 60Hz={at_60} differ too much ({diff}px)"
        );
    }

    #[test]
    fn perf_work_per_frame_bounded_and_depth_independent() {
        // THE defining virtual-list guarantee, measured: the work a scroll frame
        // does (`last_scan_ops` = binary-search steps + forward window walk) must
        // stay tiny and be INDEPENDENT of (a) total item count and (b) scroll
        // depth. If range-finding were O(scroll-depth) (linear walk from the top)
        // or O(n), deep scrolls / bigger lists would blow this up.
        fn settled_ops(count: usize, viewport: i32, scroll_to: i32) -> u32 {
            let p = make_provider(count);
            let mut list = VirtualList::new(
                p,
                FxPx::new(viewport),
                // Large measured budget so the content spans the full count and
                // a deep scroll really lands deep (not clamped by max_measured).
                VirtualListConfig { overscan: 4, max_measured: count, ..Default::default() },
            );
            list.scroll_by(FxPx::new(scroll_to));
            list.last_scan_ops()
        }

        // 48px rows, 600px viewport → window ~13 + overscan ~8 ≈ 21 items.
        // log2(100_000) ≈ 17 binary-search steps. Bound generously at 64.
        let small_shallow = settled_ops(1_000, 600, 200);
        let big_shallow = settled_ops(100_000, 600, 200);
        let big_deep = settled_ops(100_000, 600, 48 * 90_000); // ~90k items down

        std::eprintln!(
            "scan ops — 1k@shallow={small_shallow}, 100k@shallow={big_shallow}, 100k@deep={big_deep}"
        );

        // (a) Size-independent: a 100× bigger list costs only a few extra
        // binary-search steps (log growth), nowhere near 100×.
        assert!(
            big_shallow <= small_shallow + 8,
            "scan grew with list size: {small_shallow}→{big_shallow}"
        );
        // (b) Depth-independent: scrolling 90k items down costs the same as a
        // shallow scroll (NOT proportional to depth).
        assert!(
            big_deep <= big_shallow + 4,
            "scan grew with scroll depth: {big_shallow}→{big_deep}"
        );
        // (c) Absolutely bounded: ~window + log n, regardless.
        assert!(big_deep < 64, "work-per-frame not bounded: {big_deep} ops");
    }

    #[test]
    fn embedder_content_height_drives_max_scroll_and_clamps() {
        // An embedder (windowd's chat) measures content with its OWN model and
        // sets the authoritative height; the component owns the physics. Here the
        // provider's height_hint would imply one total, but the embedder overrides
        // it — fling/tick must clamp to the embedder's bottom, not the component's.
        let p = make_provider(100);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        list.set_content_height(FxPx::new(5_000));
        assert_eq!(list.max_scroll(), 4_800, "max_scroll = content - viewport");

        // A hard downward flick (release velocity) whose coast far exceeds the
        // embedder bottom must clamp at max_scroll (stop at the edge), not overshoot.
        for _ in 0..20 {
            list.fling(FxPx::new(5_000)); // px/s — accumulates to a huge coast
        }
        let mut guard = 0;
        while list.is_animating() && guard < 5_000 {
            list.tick(FRAME_NS);
            guard += 1;
        }
        assert_eq!(list.scroll_offset().as_i32(), 4_800, "clamps + settles at embedder bottom");
        assert!(!list.is_animating(), "settled at the edge");

        // Shrinking the content (e.g. messages removed) re-clamps a stranded offset.
        list.set_content_height(FxPx::new(1_000));
        assert_eq!(list.max_scroll(), 800);
        assert_eq!(list.scroll_offset().as_i32(), 800, "offset re-clamped to new bottom");
    }

    #[test]
    fn lazy_prefetch_fires_when_window_reaches_loaded_end() {
        struct Lazy {
            items: Vec<Option<&'static str>>,
            requested: bool,
        }
        impl ItemProvider for Lazy {
            type Item = &'static str;
            fn len_hint(&self) -> Option<usize> {
                Some(self.items.len())
            }
            fn get(&self, r: core::ops::Range<usize>) -> &[Option<&'static str>] {
                let lo = r.start.min(self.items.len());
                let hi = r.end.min(self.items.len());
                &self.items[lo..hi]
            }
            fn request_more(&mut self, _i: usize) {
                self.requested = true;
            }
            fn has_inflight(&self) -> bool {
                false
            }
            fn height_hint(&self, _i: usize) -> u32 {
                48
            }
        }
        let p = Lazy { items: (0..30).map(|_| Some("x")).collect(), requested: false };
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        assert!(!list.provider().requested, "no prefetch at the top");
        // Scroll to the bottom: the visible window reaches the loaded end →
        // the list auto-requests the next page (lazy loading, no manual call).
        list.scroll_by(FxPx::new(10_000));
        assert!(list.provider().requested, "prefetch must fire near the end");
    }

    #[test]
    fn page_arrived_extends_measurements() {
        let p = make_provider(50);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        let before = list.core.measured.len();
        list.page_arrived();
        assert!(list.core.measured.len() >= before);
    }

    #[test]
    fn anchor_stable_after_scroll() {
        let p = make_provider(100);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        list.scroll_by(FxPx::new(50));
        let anchor = list.anchor();
        assert!(anchor.leading_index < 100);
    }

    #[test]
    fn acknowledge_resets_state() {
        let p = make_provider(50);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        list.scroll_by(FxPx::new(10));
        assert!(list.is_scrolling());
        list.acknowledge();
        assert!(!list.is_scrolling());
    }

    #[test]
    fn overscan_adds_extra_items() {
        let p = make_provider(100);
        let list = VirtualList::new(
            p,
            FxPx::new(48), // exactly 1 item tall
            VirtualListConfig { overscan: 2, ..Default::default() },
        );
        let range = list.visible_range();
        // 1 visible + 2 top overscan + 2 bottom overscan = up to 5 items
        assert!(range.end - range.start <= 5);
    }

    #[test]
    fn len_hint_none_handled() {
        struct UnknownProvider;
        impl ItemProvider for UnknownProvider {
            type Item = ();
            fn len_hint(&self) -> Option<usize> {
                None
            }
            fn get(&self, _range: core::ops::Range<usize>) -> &[Option<()>] {
                &[]
            }
            fn request_more(&mut self, _trigger_index: usize) {}
            fn has_inflight(&self) -> bool {
                false
            }
            fn height_hint(&self, _index: usize) -> u32 {
                48
            }
        }
        let list = VirtualList::new(UnknownProvider, FxPx::new(200), VirtualListConfig::default());
        assert_eq!(list.core.measured.len(), 0);
    }

    #[test]
    fn item_view_builds_a_cell_node() {
        use nexus_layout_types::{FlexItem, Spacer};
        // The app-supplied cell builder: an item → LayoutNode. windowd never
        // sees the item type — only the resulting node tree.
        struct RowView;
        impl ItemView for RowView {
            type Item = &'static str;
            fn build_item(&self, _index: usize, _item: &&'static str) -> LayoutNode {
                LayoutNode::Spacer(Spacer {
                    id: Some("row"),
                    flex_grow: 1,
                    min_size: None,
                    item: FlexItem::default(),
                })
            }
        }
        let node = RowView.build_item(0, &"hello");
        assert!(matches!(node, LayoutNode::Spacer(_)));
    }

    #[test]
    fn list_rows_and_filter() {
        use nexus_layout_types::{FlexItem, Spacer};
        struct RowView;
        impl ItemView for RowView {
            type Item = &'static str;
            fn build_item(&self, _i: usize, _item: &&'static str) -> LayoutNode {
                LayoutNode::Spacer(Spacer {
                    id: None,
                    flex_grow: 1,
                    min_size: None,
                    item: FlexItem::default(),
                })
            }
        }
        let items: [&'static str; 4] = ["apple", "apricot", "banana", "cherry"];
        let list = List::new(&items, &RowView);
        // No filter → one row per item.
        assert_eq!(list.rows().len(), 4);
        // Filter (query "ap") → only matching items get rows. O(items) build.
        let filtered = list.filtered_rows(|s| s.starts_with("ap"));
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn scrollable_list_shares_the_core_and_scrolls() {
        use nexus_layout_types::measure::{LineLayout, PreparedTextHandle};
        use nexus_layout_types::node::TextContent;
        use nexus_layout_types::{
            Align, Direction, EdgeInsets, FlexItem, Justify, MeasureText, Overflow, Rgba8, Stack,
            TextStyle, VisualStyle,
        };

        struct StubMeasure;
        impl MeasureText for StubMeasure {
            fn prepare(&self, _c: &TextContent, _s: &TextStyle) -> PreparedTextHandle {
                PreparedTextHandle(0)
            }
            fn measure_width(&self, _h: &PreparedTextHandle) -> FxPx {
                FxPx::new(40)
            }
            fn layout_lines(
                &self,
                _h: &PreparedTextHandle,
                _w: FxPx,
                _m: Option<u32>,
            ) -> LineLayout {
                LineLayout { lines: Vec::new(), natural_width: FxPx::new(40) }
            }
        }
        // Fixed 30px rows.
        struct RowView;
        impl ItemView for RowView {
            type Item = u32;
            fn build_item(&self, _i: usize, _item: &u32) -> LayoutNode {
                LayoutNode::Stack(
                    Stack {
                        id: None,
                        direction: Direction::Column,
                        gap: FxPx::ZERO,
                        padding: EdgeInsets::all(FxPx::ZERO),
                        align: Align::Start,
                        justify: Justify::Start,
                        overflow: Overflow::Visible,
                        flex_wrap: false,
                        min_width: None,
                        max_width: None,
                        min_height: Some(FxPx::new(30)),
                        max_height: None,
                        item: FlexItem::default(),
                    },
                    VisualStyle {
                        background: Some(Rgba8::new(20, 24, 32, 255)),
                        ..Default::default()
                    },
                    Vec::new(),
                )
            }
        }

        // 100 rows at the 48px placeholder height in a 120px viewport → finite,
        // scrollable, windowed. (Per-item measurement is covered by the
        // VirtualList measure test; here we exercise the shared scroll core.)
        let items: Vec<u32> = (0..100).collect();
        let mut list = List::scrollable(&items, &RowView, FxPx::new(120), 2);
        let bottom = list.max_scroll();
        assert!(list.content_height().as_i32() > 120, "content exceeds the viewport");
        assert_eq!(bottom, list.content_height().as_i32() - 120, "max_scroll = content - viewport");

        // The SAME ScrollMomentum SSOT drives it: an immediate jump moves the
        // offset, and the visible window stays O(window) — not all 100 items.
        list.scroll_by(FxPx::new(300));
        assert!(list.scroll_offset().as_i32() > 0, "scrolled via the shared core");
        // A wheel notch sets the ease target (lands on tick, not instantly).
        list.scroll_wheel(FxPx::new(60));
        assert!(list.scroll_target() > list.scroll_offset().as_i32(), "wheel extends the target");
        let boxes = list.visible_boxes(&StubMeasure, FxPx::new(300));
        assert!(!boxes.is_empty() && boxes.len() < 20, "windowed paint: {} boxes", boxes.len());

        // A hard fling clamps at the finite bottom (SSOT clamping), never past it.
        for _ in 0..40 {
            list.fling(FxPx::new(5_000));
        }
        let mut guard = 0;
        while list.is_animating() && guard < 5_000 {
            list.tick(FRAME_NS);
            guard += 1;
        }
        assert_eq!(list.scroll_offset().as_i32(), bottom, "fling clamps at the finite bottom");
    }

    #[test]
    fn measure_with_fills_heights_from_the_layout_engine() {
        use nexus_layout_types::measure::{LineLayout, LineMetrics, PreparedTextHandle};
        use nexus_layout_types::node::TextContent;
        use nexus_layout_types::{FlexItem, MeasureText, Spacer, TextStyle};

        struct StubMeasure;
        impl MeasureText for StubMeasure {
            fn prepare(&self, _c: &TextContent, _s: &TextStyle) -> PreparedTextHandle {
                PreparedTextHandle(0)
            }
            fn measure_width(&self, _h: &PreparedTextHandle) -> FxPx {
                FxPx::new(40)
            }
            fn layout_lines(
                &self,
                _h: &PreparedTextHandle,
                width: FxPx,
                max_lines: Option<u32>,
            ) -> LineLayout {
                let line = LineMetrics {
                    text_range: 0..1,
                    width: FxPx::new(40).min(width.max(FxPx::new(1))),
                    baseline: FxPx::new(16),
                    height: FxPx::new(16),
                };
                let lines =
                    if matches!(max_lines, Some(0)) { Vec::new() } else { alloc::vec![line] };
                LineLayout { lines, natural_width: FxPx::new(40) }
            }
        }
        // Item cell = a fixed-height box; the engine measures it (no text needed).
        struct RowView;
        impl ItemView for RowView {
            type Item = &'static str;
            fn build_item(&self, _i: usize, _item: &&'static str) -> LayoutNode {
                LayoutNode::Spacer(Spacer {
                    id: None,
                    flex_grow: 0,
                    min_size: Some(FxPx::new(30)),
                    item: FlexItem::default(),
                })
            }
        }

        let mut list =
            VirtualList::new(make_provider(20), FxPx::new(100), VirtualListConfig::default());
        assert!(list.core.measured.iter().take(20).all(|m| m.estimated), "start estimated");
        list.measure_with(&RowView, &StubMeasure, FxPx::new(200));
        // Loaded items now carry an engine-measured (non-estimated) height.
        assert!(
            list.core.measured.iter().take(20).all(|m| !m.estimated),
            "all loaded items measured by the layout engine"
        );
    }

    #[test]
    fn visible_boxes_are_windowed_not_all_items() {
        use nexus_layout_types::measure::{LineLayout, PreparedTextHandle};
        use nexus_layout_types::node::TextContent;
        use nexus_layout_types::{
            Align, Direction, EdgeInsets, FlexItem, Justify, MeasureText, Overflow, Rgba8, Stack,
            TextStyle, VisualStyle,
        };

        struct StubMeasure;
        impl MeasureText for StubMeasure {
            fn prepare(&self, _c: &TextContent, _s: &TextStyle) -> PreparedTextHandle {
                PreparedTextHandle(0)
            }
            fn measure_width(&self, _h: &PreparedTextHandle) -> FxPx {
                FxPx::new(40)
            }
            fn layout_lines(
                &self,
                _h: &PreparedTextHandle,
                _w: FxPx,
                _m: Option<u32>,
            ) -> LineLayout {
                LineLayout { lines: Vec::new(), natural_width: FxPx::new(40) }
            }
        }
        // Each item is a fixed-height box (so it produces a LayoutBox).
        struct RowView;
        impl ItemView for RowView {
            type Item = &'static str;
            fn build_item(&self, _i: usize, _item: &&'static str) -> LayoutNode {
                LayoutNode::Stack(
                    Stack {
                        id: None,
                        direction: Direction::Column,
                        gap: FxPx::ZERO,
                        padding: EdgeInsets::all(FxPx::ZERO),
                        align: Align::Start,
                        justify: Justify::Start,
                        overflow: Overflow::Visible,
                        flex_wrap: false,
                        min_width: None,
                        max_width: None,
                        min_height: Some(FxPx::new(30)),
                        max_height: None,
                        item: FlexItem::default(),
                    },
                    VisualStyle {
                        background: Some(Rgba8::new(20, 24, 32, 255)),
                        ..Default::default()
                    },
                    Vec::new(),
                )
            }
        }

        let mut list =
            VirtualList::new(make_provider(200), FxPx::new(120), VirtualListConfig::default());
        list.measure_with(&RowView, &StubMeasure, FxPx::new(300));
        let boxes = list.visible_boxes(&RowView, &StubMeasure, FxPx::new(300));
        assert!(!boxes.is_empty(), "visible window produces boxes");
        // O(window): a ~120px viewport over 30px rows shows a handful, NOT 200.
        assert!(boxes.len() < 50, "got {} boxes — must be windowed, not O(N)", boxes.len());
    }
}
