// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `.textFit(pct, min, max)` — box-relative type. The container turns
//! its settled content-box HEIGHT into one inherited font size (`TextFit`), and
//! each text descendant then steps DOWN that size until its run fits its own
//! width. Own module because `engine.rs` sits against the module-size ratchet.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: src/engine_tests.rs (`textfit_*`)
//! ADR: docs/adr/0030-layout-engine-deterministic-pretext.md

use nexus_layout_types::{FxPx, MeasureText, TextContent, TextStyle};

/// The resolved `.textFit` context inherited by a subtree: the container's
/// target size plus the floor the per-text width step-down may not go below.
/// Both are already clamped by [`nexus_layout_types::TextFit::target`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FitCtx {
    pub(crate) target: FxPx,
    pub(crate) min: FxPx,
}

/// How many candidate sizes the step-down may try before giving up. The baked
/// ladder has a handful of rungs and consecutive pixel sizes collapse onto the
/// same one, so this is a runaway guard, not a working limit.
const MAX_STEPS: usize = 128;

/// The font size a text run should actually be drawn at.
///
/// `target` is the container's inherited size. If the run is wider than
/// `width_limit` the size steps down one pixel at a time (the measure
/// implementation quantises to its baked ladder, so many steps are free) until
/// it fits or `min` is reached.
///
/// Stepping down on WIDTH is what reproduces the handoff's `52 / 38 / 30`
/// display ramp without hard-coding digit counts: a long number simply stops
/// fitting at the larger rungs. It is also idempotent — the chosen size is the
/// largest that fits, so re-running against that same box returns it again,
/// which is what keeps a relayout stable.
pub(crate) fn fit_size(
    measure: &dyn MeasureText,
    content: &TextContent,
    style: &TextStyle,
    target: FxPx,
    min: FxPx,
    width_limit: FxPx,
) -> FxPx {
    let mut probe = style.clone();
    let mut size = target.max(min);
    let mut steps = 0;
    while steps < MAX_STEPS && size > min {
        probe.font_size = size;
        let handle = measure.prepare(content, &probe);
        if measure.measure_width(&handle) <= width_limit {
            break;
        }
        size = FxPx::new(size.0 - 1);
        steps += 1;
    }
    size.max(min)
}

/// The style a text node should be measured and painted with, given whatever
/// `.textFit` context it sits in. `None` inherited size = the node keeps its
/// own authored `.textSize`.
pub(crate) fn fitted_style(
    measure: &dyn MeasureText,
    content: &TextContent,
    style: &TextStyle,
    inherited: Option<FitCtx>,
    width_limit: FxPx,
) -> TextStyle {
    let mut out = style.clone();
    if let Some(ctx) = inherited {
        out.font_size = fit_size(measure, content, style, ctx.target, ctx.min, width_limit);
    }
    out
}
