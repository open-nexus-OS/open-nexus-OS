// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What size a child is laid out AGAINST — the constraint half of
//! flex placement, split out of `engine.rs` so the placement loops read as
//! geometry.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Covered by src/engine_tests.rs + tests/ui_v4_host/
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md

use crate::engine::LayoutConstraints;
use nexus_layout_types::{Align, FlexItem, FxPx};

/// The default: an upper bound the child may hug inside.
pub(crate) fn child_constraints(
    parent: LayoutConstraints,
    item: FlexItem,
    max_width: FxPx,
    max_height: Option<FxPx>,
) -> LayoutConstraints {
    let height = max_height.or(parent.max_height);
    LayoutConstraints::new(
        max_width.max(FxPx::ZERO),
        height.map(|value| value.saturating_sub(item.margin.vertical())),
    )
}

/// What a ROW child lays its own subtree out against.
///
/// Two cases turn the bound into a DEFINITE size — in both, the parent has
/// already decided how big the child is, and `update_box_geometry` records
/// exactly that. Laying the subtree out against the hugged measurement instead
/// leaves the box and its contents disagreeing:
///
/// - **stretched** (cross axis): a hugging nested row collapsed its `Spacer`,
///   which is how the shell's topbar came out 161px wide inside a 1280px slot;
/// - **flex-grown** (main axis): the container painted its full allocation
///   while everything inside stayed at the intrinsic width — including a
///   `TextInput`'s rect, which is what `View::focus_text_at` resolves a tap
///   against, so a full-width search strip was inert outside its first ~180px.
pub(crate) fn row_child_constraints(
    parent: LayoutConstraints,
    item: FlexItem,
    align: Align,
    child_width: FxPx,
    measured_width: FxPx,
    measured_height: FxPx,
    available_cross: FxPx,
) -> LayoutConstraints {
    if matches!(align, Align::Stretch) && parent.max_height.is_some() {
        LayoutConstraints::definite(
            child_width,
            Some(available_cross.saturating_sub(item.margin.vertical())),
        )
    } else if item.flex_grow > 0 && child_width > measured_width {
        // Width only: the height stays whatever `final_height` records.
        LayoutConstraints::definite(child_width, Some(measured_height))
    } else {
        child_constraints(parent, item, child_width, Some(available_cross))
    }
}
