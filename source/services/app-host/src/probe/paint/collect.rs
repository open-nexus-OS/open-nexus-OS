// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Scene WALKING for the paint pass: the pre-order text collection whose
//! numbering the row painter indexes by, the absolute-pass rule that
//! numbering has to mirror, and the caret probe that rides the same walk.
//!
//! Split from `paint.rs` because it is traversal, not rasterisation: nothing
//! here touches a pixel, and everything here has to agree with `path_to_box_id`
//! on the order nodes are counted in. Keeping the three walkers together makes
//! that agreement visible instead of spread across a 600-line file.

// Deliberately no `use super::*`: every type this walker touches is named in
// full, so the module stays readable next to `paint.rs`'s glob.

/// Pre-order text collection (index parallels `LayoutBox::node_id` − 1;
/// the same three-consumer numbering as windowd's demo mount — do not
/// reorder emission).
pub(in crate::probe) fn collect_texts(
    node: &nexus_layout_types::LayoutNode,
    index: &mut usize,
    out: &mut alloc::vec::Vec<crate::layout_diff::TextRun>,
) {
    use nexus_layout_types::LayoutNode as N;
    *index += 1;
    match node {
        N::Text(text, _) => {
            let font = nexus_text_baked::measure_text::BakedTextMeasure::font(&text.style);
            let c = text.style.color;
            out.push((
                *index,
                alloc::string::String::from(text.content.as_str()),
                font,
                [c.b, c.g, c.r, c.a],
                text.style.font_weight,
            ));
        }
        N::TextInput(input, _) => {
            // RFC-0075 Phase 8c: a TextField's typed content (already
            // bullet-masked for `secure`) — or its dimmed placeholder
            // while empty. This arm was MISSING: no TextField ever
            // painted its text (the store/insert side was always fine).
            let font = nexus_text_baked::measure_text::BakedTextMeasure::font(&input.style);
            let c = input.style.color;
            if !input.content.as_str().is_empty() {
                out.push((
                    *index,
                    alloc::string::String::from(input.content.as_str()),
                    font,
                    [c.b, c.g, c.r, c.a],
                    input.style.font_weight,
                ));
            } else if let Some(placeholder) = &input.placeholder {
                // Placeholder at ~55% of the content color (dimmed).
                let dim = |v: u8| ((u16::from(v) * 140) / 255) as u8;
                out.push((
                    *index,
                    alloc::string::String::from(placeholder.as_str()),
                    font,
                    [dim(c.b), dim(c.g), dim(c.r), c.a],
                    input.style.font_weight,
                ));
            } else {
                // Empty field without placeholder: keep an (empty) run —
                // the caret bar anchors to a paint entry.
                out.push((
                    *index,
                    alloc::string::String::new(),
                    font,
                    [c.b, c.g, c.r, c.a],
                    input.style.font_weight,
                ));
            }
        }
        N::Stack(_, _, children) => {
            // Engine visit order: in-flow children first, `.overlay()`
            // (Position::Absolute) children AFTER them — node ids are
            // assigned at visit time, and a counter walking declaration
            // order bound every text behind a non-last open overlay to the
            // wrong box (menu labels painted into placeholder stacks).
            for child in children.iter().filter(|c| !is_absolute(c)) {
                collect_texts(child, index, out);
            }
            for child in children.iter().filter(|c| is_absolute(c)) {
                collect_texts(child, index, out);
            }
        }
        N::Grid(_, _, children) => {
            for child in children {
                collect_texts(child, index, out);
            }
        }
        _ => {}
    }
}

/// Whether the engine defers this child to the absolute pass (a Stack's
/// `.overlay()` children get their node ids AFTER every in-flow sibling).
pub(super) fn is_absolute(node: &nexus_layout_types::LayoutNode) -> bool {
    matches!(node.item().position, nexus_layout_types::Position::Absolute)
}

/// Pre-order walk to the focused TextInput: same traversal/count contract as
/// `collect_texts` (ids must line up with the paint entries).
pub(in crate::probe) fn caret_input<'a>(
    node: &'a nexus_layout_types::LayoutNode,
    index: &mut usize,
    target: usize,
    inside: bool,
) -> Option<(usize, &'a str, nexus_text_baked::FontSize, [u8; 4])> {
    use nexus_layout_types::LayoutNode as N;
    *index += 1;
    let here = *index;
    let inside = inside || here == target;
    match node {
        N::TextInput(input, _) if inside => {
            let font = nexus_text_baked::measure_text::BakedTextMeasure::font(&input.style);
            let c = input.style.color;
            Some((here, input.content.as_str(), font, [c.b, c.g, c.r, c.a]))
        }
        N::Stack(_, _, children) => {
            // Same visit order as the engine and `collect_texts`: in-flow
            // children first, absolute (`.overlay()`) children after.
            for child in children.iter().filter(|c| !is_absolute(c)) {
                if let Some(hit) = caret_input(child, index, target, inside) {
                    return Some(hit);
                }
            }
            for child in children.iter().filter(|c| is_absolute(c)) {
                if let Some(hit) = caret_input(child, index, target, inside) {
                    return Some(hit);
                }
            }
            None
        }
        N::Grid(_, _, children) => {
            for child in children {
                if let Some(hit) = caret_input(child, index, target, inside) {
                    return Some(hit);
                }
            }
            None
        }
        _ => None,
    }
}

/// Paints the caret's 2-px slice of one row (vertical inset 2 px; clipped to
/// `right` and the row buffer).
#[allow(clippy::too_many_arguments)]
pub(in crate::probe) fn paint_caret_row(
    row: &mut [u8],
    y: i32,
    by: i32,
    bh: i32,
    bx: i32,
    content_w: i32,
    right: u32,
    color: [u8; 4],
) {
    if y < by + 2 || y >= by + bh - 2 {
        return;
    }
    let x = (bx + content_w + 1).max(0);
    for px in x..x + 2 {
        if px < 0 || px as u32 >= right {
            continue;
        }
        let o = px as usize * 4;
        if o + 4 <= row.len() {
            row[o] = color[0];
            row[o + 1] = color[1];
            row[o + 2] = color[2];
            row[o + 3] = 255;
        }
    }
}

/// One-shot, bounded dump of the TEXT runs and the face each will actually be
/// painted with — the counterpart of `dump_handler_boxes`.
///
/// Exists because "renders fine on host, blank on device" has no other signal:
/// the scene, the layout and the run count can all be correct while the glyph
/// pass draws nothing, and `texts=N` only reports the count. Printing the
/// content, the resolved face and the box turns that into one boot.
pub(in crate::probe) fn dump_text_runs(
    texts: &[crate::layout_diff::TextRun],
    boxes: &[nexus_layout::LayoutBox],
) {
    super::raw_marker(&alloc::format!("apphost: {} text runs", texts.len()));
    for (idx, content, font, _, weight) in texts.iter().take(8) {
        let b = boxes.iter().find(|b| b.node_id == *idx);
        let painted = b.and_then(|b| b.text_px).map_or(*font, |px| {
            nexus_text_baked::FontSize::nearest(
                px.0,
                nexus_text_baked::measure_text::BakedTextMeasure::weight(*weight),
            )
        });
        super::raw_marker(&alloc::format!(
            "apphost: run '{}' face={:?} px={:?} box={:?}",
            content,
            painted,
            b.and_then(|b| b.text_px).map(|p| p.0),
            b.map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0)),
        ));
    }
}
