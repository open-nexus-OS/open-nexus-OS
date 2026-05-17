// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Paragraph/run cache + line-layout cache (pretext model).
//! - ParagraphCache: keyed by text+style, width-independent.
//! - LineLayoutCache: keyed by paragraph_key+width_bucket, width-dependent.

use crate::wrap;
use crate::{ShapeContext, ShapeResult};
use nexus_layout_types::{
    FxPx, LineLayout, MeasureText, PreparedTextHandle, TextContent, TextStyle, WhiteSpace,
};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::vec::Vec;

/// Maximum number of cached paragraphs.
const MAX_PARAGRAPH_ENTRIES: usize = 256;

/// Maximum number of cached line layouts.
const MAX_LINE_ENTRIES: usize = 512;

/// Cache key for a prepared paragraph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParagraphKey {
    pub text_hash: u64,
    pub font_size: i32,
    pub font_weight: u16,
    pub white_space: u8,
}

/// A prepared paragraph: shaped text ready for line breaking.
#[derive(Debug, Clone)]
pub struct PreparedParagraph {
    pub text: String,
    pub char_advance: FxPx,
    pub line_height: FxPx,
    pub natural_width: FxPx,
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
        natural_width: FxPx,
    ) -> PreparedParagraph {
        if let Some(entry) = self.entries.get(&key) {
            return entry.clone();
        }
        let para =
            PreparedParagraph { text: text.to_string(), char_advance, line_height, natural_width };
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

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ParagraphCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Width-dependent line layout cache.
pub struct LineLayoutCache {
    entries: BTreeMap<(usize, i32, Option<u32>), LineLayout>,
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
        let key = (para_handle.0, width.0, max_lines);
        if let Some(layout) = self.entries.get(&key) {
            return layout.clone();
        }
        let layout =
            wrap::break_lines(&para.text, width, para.char_advance, para.line_height, max_lines);
        self.entries.insert(key, layout.clone());
        if self.entries.len() > MAX_LINE_ENTRIES {
            if let Some(first_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&first_key);
            }
        }
        layout
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for LineLayoutCache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CachedTextMeasure {
    shaper: Option<Arc<ShapeContext>>,
    paragraphs: RefCell<ParagraphCache>,
    line_layouts: RefCell<LineLayoutCache>,
    prepared: RefCell<Vec<PreparedParagraph>>,
}

impl CachedTextMeasure {
    pub fn new() -> Self {
        Self {
            shaper: None,
            paragraphs: RefCell::new(ParagraphCache::new()),
            line_layouts: RefCell::new(LineLayoutCache::new()),
            prepared: RefCell::new(Vec::new()),
        }
    }

    pub fn with_font_dir(font_dir: &Path) -> ShapeResult<Self> {
        Ok(Self {
            shaper: Some(Arc::new(ShapeContext::new(font_dir)?)),
            paragraphs: RefCell::new(ParagraphCache::new()),
            line_layouts: RefCell::new(LineLayoutCache::new()),
            prepared: RefCell::new(Vec::new()),
        })
    }

    pub fn paragraph_cache_len(&self) -> usize {
        self.paragraphs.borrow().len()
    }

    pub fn line_layout_cache_len(&self) -> usize {
        self.line_layouts.borrow().len()
    }

    fn paragraph_key(content: &TextContent, style: &TextStyle) -> ParagraphKey {
        let mut hasher = DefaultHasher::new();
        content.as_str().hash(&mut hasher);
        ParagraphKey {
            text_hash: hasher.finish(),
            font_size: style.font_size.0,
            font_weight: style.font_weight as u16,
            white_space: match style.white_space {
                WhiteSpace::Normal => 0,
                WhiteSpace::Pre => 1,
                WhiteSpace::NoWrap => 2,
            },
        }
    }

    fn approximate_paragraph(&self, content: &TextContent, style: &TextStyle) -> PreparedParagraph {
        let natural_width = if let Some(shaper) = &self.shaper {
            let run = shaper
                .shape(
                    content.as_str(),
                    crate::types::PixelSize(style.font_size.0.max(1) as u16),
                    rustybuzz::Direction::LeftToRight,
                )
                .ok();
            run.map(|run| FxPx::new(run.width as i32)).unwrap_or_else(|| {
                FxPx::new(content.as_str().chars().count() as i32 * (style.font_size.0 / 2).max(1))
            })
        } else {
            FxPx::new(content.as_str().chars().count() as i32 * (style.font_size.0 / 2).max(1))
        };
        let char_count = content.as_str().chars().count().max(1) as i32;
        let char_advance = FxPx::new((natural_width.0 / char_count).max(1));
        PreparedParagraph {
            text: content.as_str().to_string(),
            char_advance,
            line_height: style.line_height.effective(style.font_size),
            natural_width,
        }
    }
}

impl Default for CachedTextMeasure {
    fn default() -> Self {
        Self::new()
    }
}

impl MeasureText for CachedTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let key = Self::paragraph_key(content, style);
        let paragraph = self.approximate_paragraph(content, style);
        let prepared = self.paragraphs.borrow_mut().get_or_insert(
            key,
            content.as_str(),
            paragraph.char_advance,
            paragraph.line_height,
            paragraph.natural_width,
        );
        let mut handles = self.prepared.borrow_mut();
        handles.push(prepared);
        PreparedTextHandle(handles.len() - 1)
    }

    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
        self.prepared
            .borrow()
            .get(handle.0)
            .map(|paragraph| paragraph.natural_width)
            .unwrap_or(FxPx::ZERO)
    }

    fn layout_lines(
        &self,
        handle: &PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
    ) -> LineLayout {
        let prepared = self.prepared.borrow();
        let Some(paragraph) = prepared.get(handle.0) else {
            return LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO };
        };
        self.line_layouts.borrow_mut().get_or_compute(*handle, width, max_lines, paragraph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_layout_types::{FontWeight, LineHeight, TextAlign, WhiteSpace};

    fn px(value: i32) -> FxPx {
        FxPx::new(value)
    }

    fn style() -> TextStyle {
        TextStyle {
            font_size: px(16),
            font_weight: FontWeight::Regular,
            line_height: LineHeight::Absolute(px(20)),
            text_align: TextAlign::Left,
            color: nexus_layout_types::Rgba8::WHITE,
            white_space: WhiteSpace::Normal,
        }
    }

    #[test]
    fn reuses_paragraph_cache_for_same_text() {
        let measure = CachedTextMeasure::new();
        let text = TextContent::new("cache me");
        let a = measure.prepare(&text, &style());
        let b = measure.prepare(&text, &style());
        assert_ne!(a.0, b.0);
        assert_eq!(measure.paragraph_cache_len(), 1);
    }

    #[test]
    fn caches_line_layout_by_width() {
        let measure = CachedTextMeasure::new();
        let handle = measure.prepare(&TextContent::new("line cache"), &style());
        let _ = measure.layout_lines(&handle, px(120), Some(2));
        let _ = measure.layout_lines(&handle, px(120), Some(2));
        assert_eq!(measure.line_layout_cache_len(), 1);
    }
}
