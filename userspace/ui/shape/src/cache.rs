// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Paragraph/run cache + line-layout cache (pretext model).
//! - ParagraphCache: keyed by text+style, width-independent.
//! - LineLayoutCache: keyed by paragraph_key+width_bucket, width-dependent.

use std::collections::BTreeMap;
use std::vec::Vec;
use nexus_layout_types::{FxPx, LineLayout, PreparedTextHandle};
use crate::wrap;

/// Maximum number of cached paragraphs.
const MAX_PARAGRAPH_ENTRIES: usize = 256;

/// Maximum number of cached line layouts.
const MAX_LINE_ENTRIES: usize = 512;

/// Cache key for a prepared paragraph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParagraphKey {
    pub text_hash: u64,
    pub font_size: i32,
}

/// A prepared paragraph: shaped text ready for line breaking.
#[derive(Debug, Clone)]
pub struct PreparedParagraph {
    pub text: String,
    pub char_advance: FxPx,
    pub line_height: FxPx,
}

/// Width-independent paragraph cache.
pub struct ParagraphCache {
    entries: BTreeMap<ParagraphKey, PreparedParagraph>,
}

impl ParagraphCache {
    pub fn new() -> Self {
        ParagraphCache { entries: BTreeMap::new() }
    }

    pub fn get_or_insert(
        &mut self,
        key: ParagraphKey,
        text: &str,
        char_advance: FxPx,
        line_height: FxPx,
    ) -> PreparedParagraph {
        if let Some(entry) = self.entries.get(&key) {
            return entry.clone();
        }
        let para = PreparedParagraph {
            text: text.to_string(),
            char_advance,
            line_height,
        };
        self.entries.insert(key, para.clone());
        // LRU-like: evict oldest if over budget
        if self.entries.len() > MAX_PARAGRAPH_ENTRIES {
            // Simple: remove first entry (BTreeMap iteration is ordered)
            if let Some(first_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&first_key);
            }
        }
        para
    }

    pub fn len(&self) -> usize { self.entries.len() }
}

/// Width-dependent line layout cache.
pub struct LineLayoutCache {
    entries: BTreeMap<(usize, i32), LineLayout>,
}

impl LineLayoutCache {
    pub fn new() -> Self {
        LineLayoutCache { entries: BTreeMap::new() }
    }

    pub fn get_or_compute(
        &mut self,
        para_handle: PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
        para: &PreparedParagraph,
    ) -> LineLayout {
        let key = (para_handle.0, width.0);
        if let Some(layout) = self.entries.get(&key) {
            return layout.clone();
        }
        let layout = wrap::break_lines(&para.text, width, para.char_advance, para.line_height, max_lines);
        self.entries.insert(key, layout.clone());
        if self.entries.len() > MAX_LINE_ENTRIES {
            if let Some(first_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&first_key);
            }
        }
        layout
    }

    pub fn len(&self) -> usize { self.entries.len() }
}
