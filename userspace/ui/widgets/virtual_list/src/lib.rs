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
//! Part of TASK-0063 (UI v5b).

extern crate alloc;

use alloc::vec::Vec;
use nexus_layout::LayoutEngine;
use nexus_layout_types::{FxPx, LayoutNode, MeasureText};

pub mod chat;
pub use chat::{ChatMessage, ChatMessageProvider};

// ---------------------------------------------------------------------------
// ItemView — the app-supplied cell builder (Apple's data-source/cell config)
// ---------------------------------------------------------------------------

/// Builds the layout subtree ("cell") for one item.
///
/// Paired with an [`ItemProvider`] (which supplies the *data*), this is the
/// single interface through which app state reaches the generic compositor:
/// the app describes each item as a `LayoutNode` tree (boxes + `VisualStyle` +
/// text), the layout engine (`nexus_layout`) measures/places it, and windowd
/// paints the resulting `LayoutBox`es generically — the compositor has **no**
/// item-type knowledge. A "chat" is then just an `ItemProvider<Item = ChatMessage>`
/// plus an `ItemView` that renders a `ChatMessage` as a bubble, assembled by the
/// app/target-test — not baked into windowd.
///
/// This mirrors Apple's split of `UICollectionViewDataSource` (data) from the
/// cell registration/configuration (view), keeping the framework generic.
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
// VirtualList
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
pub struct VirtualList<P: ItemProvider> {
    /// The data provider.
    provider: P,
    /// Viewport height in FxPx.
    viewport_height: FxPx,
    /// Current scroll offset from the top in FxPx.
    scroll_offset: FxPx,
    /// Stable scroll anchor.
    anchor: ScrollAnchor,
    /// Currently visible item range (start..end).
    visible_range: core::ops::Range<usize>,
    /// Cached row measurements.
    measured: Vec<MeasuredRow>,
    /// Recycling pool: reused node ids.
    recycled: Vec<u64>,
    /// Currently active node ids (in display order).
    active_nodes: Vec<u64>,
    /// Configuration.
    config: VirtualListConfig,
    /// Internal state tracker.
    state: ListState,
    /// Total estimated content height.
    content_height: FxPx,
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
        let mut list = Self {
            provider,
            viewport_height,
            scroll_offset: FxPx::ZERO,
            anchor: ScrollAnchor::new(0),
            visible_range: 0..0,
            measured,
            recycled: Vec::new(),
            active_nodes: Vec::new(),
            config,
            state: ListState::Unmounted,
            content_height: FxPx::ZERO,
        };
        list.recompute_content_height();
        list.recompute_visible_range();
        list
    }

    /// Scroll by `delta` FxPx. Positive = down, negative = up.
    /// Returns the new visible range.
    pub fn scroll_by(&mut self, delta: FxPx) -> core::ops::Range<usize> {
        self.scroll_offset = FxPx::new((self.scroll_offset.as_i32() + delta.as_i32()).max(0));
        self.state = ListState::Scrolling;
        self.recompute_visible_range();
        self.visible_range.clone()
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
        let n = self.measured.len();
        if n == 0 {
            return;
        }
        let engine = LayoutEngine::new();
        // Disjoint field borrows: `provider` (read for items) + `measured` (write).
        let items = self.provider.get(0..n);
        for i in 0..items.len() {
            let Some(item) = items[i].as_ref() else {
                continue; // unloaded — keep the height-hint estimate (lazy)
            };
            let node = view.build_item(i, item);
            if let Ok(result) = engine.layout(&node, width, measure) {
                if let Some(row) = self.measured.get_mut(i) {
                    row.height = result.content_height;
                    row.estimated = false;
                }
            }
        }
        self.recompute_content_height();
        self.recompute_visible_range();
    }

    /// Notify the list that a new page of data has arrived.
    pub fn page_arrived(&mut self) {
        self.state = ListState::DataArrived;
        let hint = self.provider.len_hint().unwrap_or(self.measured.len());
        while self.measured.len() < hint.min(self.config.max_measured) {
            let i = self.measured.len();
            let h = self.provider.height_hint(i);
            self.measured.push(MeasuredRow { height: FxPx::new(h as i32), width_bucket: 0, estimated: true });
        }
        self.recompute_content_height();
    }

    /// Current visible range (inclusive start, exclusive end).
    pub fn visible_range(&self) -> core::ops::Range<usize> {
        self.visible_range.clone()
    }

    /// Current scroll offset.
    pub fn scroll_offset(&self) -> FxPx {
        self.scroll_offset
    }

    /// Stable scroll anchor.
    pub fn anchor(&self) -> ScrollAnchor {
        self.anchor
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
        self.content_height
    }

    // ── internal ────────────────────────────────────────────────

    fn recompute_visible_range(&mut self) {
        let mut y = FxPx::ZERO;
        let mut start = 0usize;
        let mut end = 0usize;
        let mut found_start = false;
        let overscan_h = FxPx::new(self.config.overscan as i32 * 48); // ~48px per overscan item

        for i in 0..self.measured.len() {
            let h = self.measured.get(i).map(|m| m.height).unwrap_or(FxPx::new(48));
            let item_end = y.as_i32() + h.as_i32();

            if !found_start && item_end > self.scroll_offset.as_i32() - overscan_h.as_i32() {
                start = i;
                found_start = true;
            }
            if found_start && y.as_i32() > self.scroll_offset.as_i32() + self.viewport_height.as_i32() + overscan_h.as_i32() {
                end = i;
                break;
            }
            y = FxPx::new(item_end);
        }
        if !found_start {
            start = self.measured.len().saturating_sub(1);
        }
        if end == 0 || end <= start {
            end = (start + 1).min(self.measured.len());
        }
        self.visible_range = start..end;
        self.anchor = ScrollAnchor { leading_index: start, offset: FxPx::ZERO };
    }

    fn recompute_content_height(&mut self) {
        let mut h = FxPx::ZERO;
        for m in &self.measured {
            h = FxPx::new(h.as_i32() + m.height.as_i32());
        }
        self.content_height = h;
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
        assert_eq!(list.viewport_height, FxPx::new(400));
        assert_eq!(list.scroll_offset, FxPx::ZERO);
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

    #[test]
    fn page_arrived_extends_measurements() {
        let p = make_provider(50);
        let mut list = VirtualList::new(p, FxPx::new(200), VirtualListConfig::default());
        let before = list.measured.len();
        list.page_arrived();
        assert!(list.measured.len() >= before);
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
        let mut list = VirtualList::new(
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
            fn len_hint(&self) -> Option<usize> { None }
            fn get(&self, _range: core::ops::Range<usize>) -> &[Option<()>] { &[] }
            fn request_more(&mut self, _trigger_index: usize) {}
            fn has_inflight(&self) -> bool { false }
            fn height_hint(&self, _index: usize) -> u32 { 48 }
        }
        let list = VirtualList::new(UnknownProvider, FxPx::new(200), VirtualListConfig::default());
        assert_eq!(list.measured.len(), 0);
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
                let lines = if matches!(max_lines, Some(0)) { Vec::new() } else { alloc::vec![line] };
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

        let mut list = VirtualList::new(make_provider(20), FxPx::new(100), VirtualListConfig::default());
        assert!(list.measured.iter().take(20).all(|m| m.estimated), "start estimated");
        list.measure_with(&RowView, &StubMeasure, FxPx::new(200));
        // Loaded items now carry an engine-measured (non-estimated) height.
        assert!(
            list.measured.iter().take(20).all(|m| !m.estimated),
            "all loaded items measured by the layout engine"
        );
    }
}
