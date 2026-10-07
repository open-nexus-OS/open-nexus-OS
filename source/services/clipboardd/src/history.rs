// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the clipboard history — a bounded, newest-first list of text items
//! in fixed storage (no heap: the os-lite allocator never frees, and sixteen
//! items of 248 bytes are a few KiB). Every item carries a monotonic `seq`
//! (newest = largest; restore re-stamps), the writer's kernel sid and its
//! origin device (0 = this device — the field a later sync consumer fills).
//! Writing a text that is already in the history moves it to the top instead
//! of storing it twice; a full history drops its oldest item.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use nexus_wire::clipboardd::{HISTORY_MAX, TEXT_MAX_BYTES};

/// One stored item.
#[derive(Clone, Copy)]
pub struct Item {
    /// Monotonic sequence number (newest = largest).
    pub seq: u64,
    /// Kernel service id of the writer.
    pub writer_sid: u64,
    /// Origin device (0 = this device).
    pub device: u64,
    len: u8,
    text: [u8; TEXT_MAX_BYTES],
}

impl Item {
    const EMPTY: Self =
        Self { seq: 0, writer_sid: 0, device: 0, len: 0, text: [0; TEXT_MAX_BYTES] };

    /// The item's text.
    #[must_use]
    pub fn text(&self) -> &str {
        // Stored only from a `&str` that fit, so this never fails; an empty
        // string is the fail-closed answer if it ever did.
        core::str::from_utf8(&self.text[..usize::from(self.len)]).unwrap_or("")
    }
}

/// The bounded history, newest first.
pub struct History {
    items: [Item; HISTORY_MAX],
    len: usize,
    next_seq: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// An empty history; the first item gets `seq` 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: [Item::EMPTY; HISTORY_MAX], len: 0, next_seq: 1 }
    }

    /// Number of stored items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// No items stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The items, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items[..self.len].iter()
    }

    /// The newest item.
    #[must_use]
    pub fn newest(&self) -> Option<&Item> {
        self.items[..self.len].first()
    }

    /// The item with `seq`.
    #[must_use]
    pub fn get(&self, seq: u64) -> Option<&Item> {
        self.iter().find(|i| i.seq == seq)
    }

    /// Stores `text` as the newest item and returns its `seq`; `None` for an
    /// empty or over-long text. A text already present moves to the top (one
    /// copy, new `seq`); a full history drops its oldest item.
    pub fn write(&mut self, text: &str, writer_sid: u64, device: u64) -> Option<u64> {
        let bytes = text.as_bytes();
        if bytes.is_empty() || bytes.len() > TEXT_MAX_BYTES {
            return None;
        }
        let existing = self.iter().position(|i| i.text() == text);
        if let Some(at) = existing {
            self.remove_at(at);
        }
        let mut item = Item::EMPTY;
        item.text[..bytes.len()].copy_from_slice(bytes);
        item.len = bytes.len() as u8;
        item.writer_sid = writer_sid;
        item.device = device;
        Some(self.push_front(item))
    }

    /// Makes the item with `seq` the newest again under a fresh `seq` (the
    /// copy-back). Returns the new `seq`; `None` for an unknown one.
    pub fn restore(&mut self, seq: u64) -> Option<u64> {
        let at = self.iter().position(|i| i.seq == seq)?;
        let item = self.items[at];
        self.remove_at(at);
        Some(self.push_front(item))
    }

    /// Drops every item (the sequence keeps counting — a `seq` is never reused).
    pub fn clear(&mut self) {
        self.len = 0;
    }

    fn push_front(&mut self, mut item: Item) -> u64 {
        item.seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let keep = self.len.min(HISTORY_MAX - 1);
        self.items.copy_within(0..keep, 1);
        self.items[0] = item;
        self.len = keep + 1;
        item.seq
    }

    fn remove_at(&mut self, at: usize) {
        if at < self.len {
            self.items.copy_within(at + 1..self.len, at);
            self.len -= 1;
        }
    }
}

/// `true` when `query` is empty or occurs in `text`, ASCII letters compared
/// without case (other bytes exactly — no locale folding in v1).
#[must_use]
pub fn matches_query(text: &str, query: &str) -> bool {
    let (t, q) = (text.as_bytes(), query.as_bytes());
    if q.is_empty() {
        return true;
    }
    if q.len() > t.len() {
        return false;
    }
    t.windows(q.len()).any(|w| w.eq_ignore_ascii_case(q))
}
