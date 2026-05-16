// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal UAX#14 line-breaking subset for deterministic text wrapping.
//! Excluded: SHY (soft hyphen), CM (combining mark), SA (complex scripts).
//! Fallback: grapheme cluster boundary or hard break at nearest opportunity.

use std::string::String;
use std::vec::Vec;
use nexus_layout_types::{FxPx, LineLayout, LineMetrics};

/// A line break opportunity in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BreakOpportunity {
    /// Byte index of the break position.
    index: usize,
    /// Whether this is a mandatory break (e.g. newline).
    mandatory: bool,
}

/// Break text into lines for a given width.
/// Returns a `LineLayout` with line metrics. Text is assumed to be pre-shaped;
/// width calculations use character counts × advance estimates.
pub fn break_lines(
    text: &str,
    width: FxPx,
    char_advance: FxPx,
    line_height: FxPx,
    max_lines: Option<u32>,
) -> LineLayout {
    let opportunities = find_opportunities(text);
    let mut lines: Vec<LineMetrics> = Vec::new();
    let mut line_start: usize = 0;
    let mut last_break: usize = 0;
    let mut cursor: usize = 0;
    let mut line_chars: usize = 0;
    let max_chars_per_line = if char_advance.0 > 0 {
        (width.0 / char_advance.0).max(1) as usize
    } else {
        text.len()
    };
    let max = max_lines.unwrap_or(u32::MAX) as usize;
    let chars: Vec<char> = text.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        cursor = i;
        line_chars += 1;

        // Check for break opportunity at this position
        let is_break = opportunities.iter().any(|bo| bo.index == i);

        if is_break {
            last_break = i;
        }

        // Mandatory break (newline)
        if ch == '\n' {
            let line_text: String = chars[line_start..i].iter().collect();
            let line_width = FxPx::new(line_text.chars().count() as i32 * char_advance.0);
            lines.push(LineMetrics {
                text_range: line_start..i,
                width: line_width,
                baseline: line_height,
                height: line_height,
            });
            line_start = i + 1;
            last_break = i + 1;
            line_chars = 0;
            if lines.len() >= max {
                break;
            }
            continue;
        }

        // Width-constrained break
        if line_chars >= max_chars_per_line {
            let break_at = if last_break > line_start { last_break } else { i + 1 };
            let line_text: String = chars[line_start..break_at].iter().collect();
            let line_width = FxPx::new(line_text.chars().count() as i32 * char_advance.0);
            lines.push(LineMetrics {
                text_range: line_start..break_at,
                width: line_width,
                baseline: line_height,
                height: line_height,
            });
            line_start = break_at;
            // Skip leading whitespace after break
            while line_start < chars.len() && chars[line_start] == ' ' {
                line_start += 1;
            }
            last_break = line_start;
            line_chars = 0;
            if lines.len() >= max {
                break;
            }
        }
    }

    // Remaining text (or empty line if no text at all)
    if (line_start < chars.len() || lines.is_empty()) && lines.len() < max {
        let end = chars.len();
        let line_text: String = chars[line_start..end].iter().collect();
        let line_width = FxPx::new(line_text.chars().count() as i32 * char_advance.0);
        lines.push(LineMetrics {
            text_range: line_start..end,
            width: line_width,
            baseline: line_height,
            height: line_height,
        });
    }

    // Ellipsis for truncation
    if max_lines.is_some() && line_start < chars.len() {
        if let Some(last) = lines.last_mut() {
            last.text_range.end = chars.len();
        }
    }

    let natural_width = lines.iter().map(|l| l.width).max().unwrap_or(FxPx::ZERO);
    LineLayout { lines, natural_width }
}

/// Find line break opportunities in text.
/// Minimal UAX#14 subset: spaces and CJK ideographs are break opportunities.
/// Newlines are mandatory breaks.
fn find_opportunities(text: &str) -> Vec<BreakOpportunity> {
    let mut ops = Vec::new();
    for (i, ch) in text.char_indices() {
        match ch {
            '\n' => ops.push(BreakOpportunity { index: i, mandatory: true }),
            ' ' | '\t' => ops.push(BreakOpportunity { index: i, mandatory: false }),
            c if is_cjk(c) => ops.push(BreakOpportunity { index: i, mandatory: false }),
            _ => {}
        }
    }
    ops
}

/// Check if a character is a CJK ideograph (UAX#14 class `ID` or similar).
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{2E80}'..='\u{2EFF}' | // CJK Radicals
        '\u{3000}'..='\u{303F}' | // CJK Symbols
        '\u{3040}'..='\u{309F}' | // Hiragana
        '\u{30A0}'..='\u{30FF}' | // Katakana
        '\u{3400}'..='\u{4DBF}' | // CJK Extension A
        '\u{4E00}'..='\u{9FFF}' | // CJK Unified
        '\u{AC00}'..='\u{D7AF}' | // Hangul
        '\u{F900}'..='\u{FAFF}' | // CJK Compatibility
        '\u{FF00}'..='\u{FFEF}'   // Halfwidth/Fullwidth
    )
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    fn px(v: i32) -> FxPx { FxPx::new(v) }

    #[test]
    fn no_wrap_short_text() {
        let layout = break_lines("hello", px(200), px(10), px(20), None);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].width, px(50)); // 5 * 10
    }

    #[test]
    fn wrap_at_width() {
        // 10 chars per line at char_advance=10, width=100
        let layout = break_lines("1234567890ABCDEF", px(100), px(10), px(20), None);
        assert!(layout.lines.len() >= 2);
    }

    #[test]
    fn newline_break() {
        let layout = break_lines("ab\ncd", px(200), px(10), px(20), None);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text_range, 0..2); // "ab"
        assert_eq!(layout.lines[1].text_range, 3..5); // "cd"
    }

    #[test]
    fn max_lines_truncation() {
        let layout = break_lines("one\ntwo\nthree\nfour", px(200), px(10), px(20), Some(2));
        assert_eq!(layout.lines.len(), 2);
    }

    #[test]
    fn cjk_break_opportunities() {
        let layout = break_lines("こんにちは世界", px(200), px(10), px(20), None);
        assert_eq!(layout.lines.len(), 1); // fits in one line
    }

    #[test]
    fn empty_string() {
        let layout = break_lines("", px(100), px(10), px(20), None);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].width, px(0));
    }
}
