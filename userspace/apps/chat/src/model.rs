// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Chat app data — the message model + deterministic provider. The chat
//! owns its content; the generic `nexus-virtual-list` widget consumes it through
//! the `ItemProvider` trait (RFC-0067 P2.4, moved out of the widget crate).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 tests
//!
//! Generates a deterministic set of chat messages whose rendered heights vary
//! from a single line ("ok") to multi-paragraph blocks, drawn from a fixed pool
//! of `&'static` bodies. This is the worst-case workload for the virtual list
//! (recycling + anchor stability under mixed heights): heights are estimated
//! here from a wrap width and re-used by `VirtualList` so only the visible window
//! is ever laid out for real.

use alloc::vec::Vec;

use nexus_virtual_list::ItemProvider;

/// A single chat message. `text` is borrowed from a static pool, so the whole
/// collection is `Copy` and allocation-free per item.
#[derive(Debug, Clone, Copy)]
pub struct ChatMessage {
    /// Message body (wraps across lines at the provider's wrap width).
    pub text: &'static str,
    /// True for outgoing messages (right-aligned bubble), false for incoming.
    pub from_me: bool,
}

/// Pool of message bodies spanning one line to several paragraphs. The mix of
/// lengths is what exercises mixed-height virtualization.
const POOL: &[&str] = &[
    "ok",
    "yes",
    "on it",
    "got it, thanks",
    "sounds good to me",
    "Can you take a look when you get a chance?",
    "I think the issue is in the render path, not the layout engine.",
    "Let me check that and get back to you in a bit — need to reproduce it first.",
    "The retained plane keeps the composited scene cached; only the cursor region \
     is re-blitted on a pointer move, so input stays on the fast path.",
    "Here's the plan: first we land the GPU blur with readback verification, then \
     we wire the virtual list into the chat panel, then we measure pacing under a \
     dual-panel blur load and confirm the 120 Hz target holds with virgl active.",
    "Long one incoming. The scene graph is the retained tree and the source of \
     truth. Mutations enqueue nodes in a dirty list, so computing the dirty set \
     is O(dirty) — only changed nodes and their ancestors are touched. Children \
     are intrusive sibling links instead of a per-node vector, which matters a \
     lot under the bump allocator that never frees. None of the per-frame paths \
     allocate; the buffers are reused frame to frame. That is what makes a chat \
     with hundreds of recycled rows actually viable on the emulated target.",
];

/// Deterministic provider over `count` synthetic messages.
///
/// Heights are estimated from a wrap width (characters per line) and a line
/// height in device pixels; the `VirtualList` consumes these via `height_hint`.
pub struct ChatMessageProvider {
    items: Vec<Option<ChatMessage>>,
    inflight: bool,
    chars_per_line: usize,
    line_height: u32,
    vertical_padding: u32,
}

impl ChatMessageProvider {
    /// Build `count` deterministic mixed-height messages.
    ///
    /// - `chars_per_line`: wrap width used to estimate row heights.
    /// - `line_height`: device-pixel height of one text line.
    pub fn synthetic(count: usize, chars_per_line: usize, line_height: u32) -> Self {
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            // Spread across the pool so heights vary run to run but stay stable
            // for a given index (deterministic — no RNG, testable).
            let pick = pool_index(i);
            items.push(Some(ChatMessage { text: POOL[pick], from_me: i % 3 == 0 }));
        }
        Self {
            items,
            inflight: false,
            chars_per_line: chars_per_line.max(1),
            line_height: line_height.max(1),
            vertical_padding: 8,
        }
    }

    /// Number of wrapped lines a message occupies at the provider's wrap width.
    pub fn line_count(&self, index: usize) -> u32 {
        let text = match self.items.get(index).and_then(|m| *m) {
            Some(m) => m.text,
            None => return 1,
        };
        wrapped_line_count(text, self.chars_per_line)
    }

    /// Total bytes held (diagnostic — the bodies are all `&'static`, so this is
    /// just the `Option<ChatMessage>` array).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Prepend `n` older messages (scroll-up load). Returns the index shift so
    /// callers can preserve the scroll anchor by key.
    pub fn prepend(&mut self, n: usize) -> usize {
        let base = self.items.len();
        let mut prefix = Vec::with_capacity(n);
        for k in 0..n {
            let pick = pool_index(base + k);
            prefix.push(Some(ChatMessage { text: POOL[pick], from_me: k % 3 == 0 }));
        }
        prefix.extend(self.items.drain(..));
        self.items = prefix;
        n
    }
}

/// Deterministic pool selection biased toward shorter messages (like real
/// chat), with periodic long ones. Always in-bounds for `POOL`.
fn pool_index(i: usize) -> usize {
    // A cheap integer hash → spread, then fold into the pool with a short bias.
    let h = (i.wrapping_mul(2654435761)) >> 13;
    let r = h % 16;
    let pick = match r {
        0..=6 => r % 5,         // short (0..4)
        7..=11 => 5 + (r % 3),  // medium (5..7)
        12..=14 => 8 + (r % 2), // long (8..9)
        _ => 9 + (r % 2),       // very long (9..10)
    };
    pick.min(POOL.len() - 1)
}

/// Number of lines `text` wraps to at `chars_per_line`, counting explicit
/// breaks. Word boundaries are honored so a word never splits mid-token.
fn wrapped_line_count(text: &str, chars_per_line: usize) -> u32 {
    let mut lines = 1u32;
    let mut col = 0usize;
    let mut word = 0usize;
    let mut flush_word = |col: &mut usize, word: &mut usize, lines: &mut u32| {
        if *word == 0 {
            return;
        }
        if *col + *word > chars_per_line && *col > 0 {
            *lines += 1;
            *col = 0;
        }
        *col += *word;
        *word = 0;
    };
    for ch in text.chars() {
        if ch == '\n' {
            flush_word(&mut col, &mut word, &mut lines);
            lines += 1;
            col = 0;
        } else if ch == ' ' {
            flush_word(&mut col, &mut word, &mut lines);
            if col > 0 && col < chars_per_line {
                col += 1; // the space itself
            }
        } else {
            word += 1;
            // A single word longer than the line hard-wraps.
            if word >= chars_per_line {
                lines += 1;
                col = 0;
                word = 0;
            }
        }
    }
    flush_word(&mut col, &mut word, &mut lines);
    lines.max(1)
}

impl ItemProvider for ChatMessageProvider {
    type Item = ChatMessage;

    fn len_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }

    fn get(&self, range: core::ops::Range<usize>) -> &[Option<Self::Item>] {
        let end = range.end.min(self.items.len());
        let start = range.start.min(end);
        &self.items[start..end]
    }

    fn request_more(&mut self, _trigger_index: usize) {
        // Synthetic data is fully resident; nothing to load. A real provider
        // would set `inflight` and dispatch a page fetch here.
        self.inflight = false;
    }

    fn has_inflight(&self) -> bool {
        self.inflight
    }

    fn height_hint(&self, index: usize) -> u32 {
        self.line_count(index) * self.line_height + 2 * self.vertical_padding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::FxPx;
    use nexus_virtual_list::{VirtualList, VirtualListConfig};

    #[test]
    fn synthetic_has_requested_count() {
        let p = ChatMessageProvider::synthetic(500, 40, 16);
        assert_eq!(p.len(), 500);
        assert_eq!(p.len_hint(), Some(500));
    }

    #[test]
    fn heights_are_mixed() {
        let p = ChatMessageProvider::synthetic(500, 40, 16);
        let mut min = u32::MAX;
        let mut max = 0u32;
        for i in 0..500 {
            let h = p.height_hint(i);
            min = min.min(h);
            max = max.max(h);
        }
        // Single-line "ok" vs multi-paragraph block → a wide spread.
        assert!(min < max, "heights should vary: {min}..{max}");
        assert!(max >= min * 3, "expected tall messages: {min}..{max}");
    }

    #[test]
    fn deterministic_for_same_index() {
        let a = ChatMessageProvider::synthetic(64, 40, 16);
        let b = ChatMessageProvider::synthetic(64, 40, 16);
        for i in 0..64 {
            assert_eq!(a.height_hint(i), b.height_hint(i));
            let am = a.get(i..i + 1)[0].unwrap();
            let bm = b.get(i..i + 1)[0].unwrap();
            assert_eq!(am.text, bm.text);
            assert_eq!(am.from_me, bm.from_me);
        }
    }

    #[test]
    fn wrap_count_honors_width_and_breaks() {
        // 1 line.
        assert_eq!(wrapped_line_count("ok", 40), 1);
        // Explicit newline forces a break.
        assert_eq!(wrapped_line_count("a\nb", 40), 2);
        // Long body wraps to multiple lines at a narrow width.
        let n = wrapped_line_count(POOL[POOL.len() - 1], 40);
        assert!(n >= 8, "long message should wrap to many lines, got {n}");
        // Narrower width → at least as many lines.
        let wide = wrapped_line_count(POOL[9], 80);
        let narrow = wrapped_line_count(POOL[9], 30);
        assert!(narrow >= wide);
    }

    #[test]
    fn virtual_list_over_500_keeps_window_small() {
        let p = ChatMessageProvider::synthetic(500, 40, 16);
        let list = VirtualList::new(p, FxPx::new(640), VirtualListConfig::default());
        let range = list.visible_range();
        // A 640px viewport over 500 messages must render only a tiny window.
        assert!(range.end - range.start < 60, "window too large: {range:?}");
        assert!(list.content_height().as_i32() > 640 * 5, "content should be tall");
    }

    #[test]
    fn scroll_advances_window_and_recovers_anchor() {
        let p = ChatMessageProvider::synthetic(500, 40, 16);
        let mut list = VirtualList::new(p, FxPx::new(400), VirtualListConfig::default());
        let top = list.visible_range();
        // Scroll far down — the window must move forward.
        list.scroll_by(FxPx::new(4000));
        let mid = list.visible_range();
        assert!(mid.start > top.start, "scroll did not advance: {top:?} -> {mid:?}");
        assert!(list.is_scrolling());
        // Anchor tracks the new leading index.
        assert_eq!(list.anchor().leading_index, mid.start);
    }

    #[test]
    fn prepend_preserves_existing_messages() {
        let mut p = ChatMessageProvider::synthetic(100, 40, 16);
        let first_before = p.get(0..1)[0].unwrap().text;
        let shift = p.prepend(20);
        assert_eq!(shift, 20);
        assert_eq!(p.len(), 120);
        // The old leading message is now at index 20 (anchor-by-key shift).
        assert_eq!(p.get(20..21)[0].unwrap().text, first_before);
    }
}
