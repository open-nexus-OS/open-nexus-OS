// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Top-bar controls must be HITTABLE, not merely visible.
//!
//! The shell's status pills are 28px tall by design (the bar is 36). That is
//! below every touch-target guideline, and it showed up as "the control
//! centre doesn't react": a press lands a few pixels off the pill and reaches
//! nothing at all. `.hitSlop(n)` grows the INPUT rect without touching a
//! pixel of layout — these tests hold both halves of that contract.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::FxPx;

/// Guideline minimum for a pointer/touch target, in px. The pills are painted
/// at 28; `.hitSlop(2)` (2 spacing steps = 8px per side) brings the input rect
/// to 44 without moving anything on screen.
const MIN_TARGET: i32 = 44;

fn mount() -> (Vec<u8>, FixtureEnv) {
    (common::compile("desktop-shell"), FixtureEnv::tablet("landscape"))
}

/// Every Tap target inside the top bar reaches the guideline minimum, and the
/// PAINTED rect stays exactly as small as it was — slop is input-only.
#[test]
fn top_bar_pills_are_44px_targets_without_growing_a_pixel() {
    let (nxir, device) = mount();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let boxes = common::layout_boxes(&view);

    // Top-bar strip = the shell chrome contract (windowd SHELL_TOPBAR_H = 36).
    let pills: Vec<_> = boxes
        .iter()
        .filter(|b| {
            b.rect.y.0 >= 0
                && b.rect.y.0 + b.rect.height.0 <= 36
                && b.rect.height.0 > 0
                && b.hit_slop > FxPx::ZERO
        })
        .collect();
    assert!(!pills.is_empty(), "the top bar must have slop-carrying targets");

    for pill in &pills {
        let painted_h = pill.rect.height.0;
        let target_h = painted_h + 2 * pill.hit_slop.0;
        let target_w = pill.rect.width.0 + 2 * pill.hit_slop.0;
        assert!(
            painted_h <= 36,
            "a top-bar pill must stay visually small (painted {painted_h}px in a 36px bar)"
        );
        assert!(
            target_h >= MIN_TARGET,
            "input target {target_h}px < {MIN_TARGET}px (painted {painted_h}px, slop {}px)",
            pill.hit_slop.0
        );
        assert!(target_w >= MIN_TARGET, "input target {target_w}px wide < {MIN_TARGET}px");
    }
}

/// The concrete miss: a press just BELOW a pill used to reach nothing. It now
/// resolves to that pill — and the pill's own rect still wins on a direct hit.
#[test]
fn a_press_just_below_a_pill_reaches_it() {
    let (nxir, device) = mount();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let boxes = common::layout_boxes(&view);

    // The rightmost slop-carrying top-bar target = the control-centre pill.
    let pill = boxes
        .iter()
        .filter(|b| b.rect.y.0 + b.rect.height.0 <= 36 && b.hit_slop > FxPx::ZERO)
        .max_by_key(|b| b.rect.x.0)
        .expect("a rightmost top-bar pill exists");

    let cx = FxPx::new(pill.rect.x.0 + pill.rect.width.0 / 2);
    let inside = FxPx::new(pill.rect.y.0 + pill.rect.height.0 / 2);
    let below = FxPx::new(pill.rect.y.0 + pill.rect.height.0 + pill.hit_slop.0 - 1);
    let far = FxPx::new(pill.rect.y.0 + pill.rect.height.0 + pill.hit_slop.0 + 8);

    let hit_inside = view.hover_box_id_scrolled(&boxes, "Tap", cx, inside, None);
    assert_eq!(hit_inside, Some(pill.node_id), "a direct press must hit the pill");

    let hit_below = view.hover_box_id_scrolled(&boxes, "Tap", cx, below, None);
    assert_eq!(
        hit_below,
        Some(pill.node_id),
        "a press {}px below the pill must still reach it (this is the regression)",
        pill.hit_slop.0 - 1
    );

    let hit_far = view.hover_box_id_scrolled(&boxes, "Tap", cx, far, None);
    assert_ne!(hit_far, Some(pill.node_id), "slop must stay bounded, not swallow the desktop");
}
