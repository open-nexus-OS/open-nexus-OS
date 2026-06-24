// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Search app data + filter + geometry — the app owns its content, not
//! the compositor (RFC-0065 / ADR-0037).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 tests
//!
//! Search app data + filter + geometry — the app's own logic (RFC-0065).
//!
//! Ported from windowd's baked search window so the app, not the compositor, owns
//! its content. windowd keeps only the window chrome (title bar / close / drag);
//! the app owns the word list, the filter, and its content layout.

use alloc::vec::Vec;

/// Content width of the search window (matches the shell window chrome width).
pub const SEARCH_W: u32 = 360;
/// Inner padding.
pub const SEARCH_PAD: u32 = 14;
/// Filter-field band height.
pub const FILTER_H: u32 = 30;
/// One result row height.
pub const ROW_H: u32 = 28;
/// Visible result rows (the filtered list scrolls within these).
pub const VISIBLE_ROWS: u32 = 10;

/// The app's searchable word list (its own data).
pub const SEARCH_WORDS: &[&str] = &[
    "apple", "application", "apt", "arrow", "asset", "atom", "audio", "batch", "binary", "block",
    "buffer", "build", "cache", "canvas", "channel", "clock", "cluster", "codec", "compile",
    "component", "config", "context", "cursor", "daemon", "device", "display", "driver", "engine",
    "event", "filter", "fragment", "frame", "gradient", "handle", "kernel", "layer", "module",
    "neuron", "packet", "pipeline", "pointer", "process", "render", "scanout", "scene", "shader",
    "shell", "socket", "surface", "texture", "thread", "vector", "vertex", "widget", "window",
];

/// Returns the words whose name starts with `prefix` (case-insensitive). Empty
/// prefix returns all words.
pub fn filter(prefix: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for w in SEARCH_WORDS {
        let hit = prefix.is_empty()
            || (w.len() >= prefix.len()
                && w.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()));
        if hit {
            out.push(*w);
        }
    }
    out
}

/// Content height: pad + filter field + pad + visible rows + pad.
pub const fn content_height() -> u32 {
    SEARCH_PAD + FILTER_H + SEARCH_PAD + ROW_H * VISIBLE_ROWS + SEARCH_PAD
}

/// Max scroll offset (in rows) for a filtered list of `count` results.
pub fn max_scroll(count: usize) -> u32 {
    (count as u32).saturating_sub(VISIBLE_ROWS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_prefix_is_case_insensitive_startswith() {
        let r = filter("ap");
        assert!(r.contains(&"apple"));
        assert!(r.contains(&"application"));
        assert!(r.contains(&"apt"));
        assert!(!r.contains(&"batch"));
        // Case-insensitive.
        assert_eq!(filter("AP"), r);
    }

    #[test]
    fn empty_prefix_returns_all() {
        assert_eq!(filter("").len(), SEARCH_WORDS.len());
    }

    #[test]
    fn no_match_is_empty() {
        assert!(filter("zzzz").is_empty());
    }

    #[test]
    fn max_scroll_bounds() {
        assert_eq!(max_scroll(VISIBLE_ROWS as usize), 0);
        assert_eq!(max_scroll(VISIBLE_ROWS as usize + 3), 3);
        assert_eq!(max_scroll(0), 0);
    }
}
