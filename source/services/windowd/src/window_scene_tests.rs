// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of the window stack (`window_scene.rs`): visibility, the z-order contract, focus,
//! minimize/restore, the order's bound and the registration of grown slots (RFC-0114 §7).

use super::*;

#[test]
fn shown_only_when_visible_and_not_frozen() {
    assert!(should_show(true, false)); // visible, live screen → show
    assert!(!should_show(true, true)); // frozen for a capture → the frame shows it
    assert!(!should_show(false, false)); // hidden → no show
    assert!(!should_show(false, true));
}

#[test]
fn hidden_windows_are_not_composited() {
    // Mirrors the boot case the user hit: chat starts hidden → not in the frame.
    let windows = [
        WindowState::floating(WindowId::App(0), false, 3),
        WindowState::floating(WindowId::App(1), false, 2),
    ];
    assert!(composition_order(&windows, false).is_empty());
}

#[test]
fn visible_window_is_composited() {
    let windows = [
        WindowState::floating(WindowId::App(0), true, 3),
        WindowState::floating(WindowId::App(1), false, 2),
    ];
    assert_eq!(composition_order(&windows, false), vec![WindowId::App(0)]);
}

#[test]
fn composition_is_z_ordered_back_to_front() {
    let windows = [
        WindowState::floating(WindowId::App(0), true, 3),
        WindowState::floating(WindowId::App(1), true, 2),
    ];
    // Search (z=2) composites before Chat (z=3) → chat on top.
    assert_eq!(composition_order(&windows, false), vec![WindowId::App(1), WindowId::App(0)]);
}

#[test]
fn a_frozen_screen_composes_no_window_and_no_overlay() {
    let mut osk = WindowState::floating(WindowId::App(2), true, 1);
    osk.role = WindowRole::Overlay;
    let windows = [
        WindowState::floating(WindowId::App(0), true, 3),
        WindowState::floating(WindowId::App(1), true, 2),
        osk,
    ];
    assert!(composition_order(&windows, true).is_empty());
}

#[test]
fn empty_composition_is_not_a_black_frame() {
    // The black-screen guard: no visible windows is valid — the base layer
    // (wallpaper) is always present, so the desktop shows, not black.
    assert!(BASE_ALWAYS_PRESENT, "compositor must always draw a base layer");
    let nothing: [WindowState; 0] = [];
    assert!(composition_order(&nothing, false).is_empty());
}

#[test]
fn desktop_role_composites_below_all_floating_windows() {
    // The shell/greeter DESKTOP surface is the base layer: it composites
    // FIRST (bottom), under every floating window, regardless of z. Here the
    // desktop has a high z (5) yet still sorts below floating chat (z=1).
    let windows = [
        WindowState::floating(WindowId::App(0), true, 1),
        WindowState::desktop(WindowId::Desktop, true),
    ];
    assert_eq!(
        composition_order(&windows, false),
        vec![WindowId::Desktop, WindowId::App(0)],
        "desktop base first, floating window on top"
    );
}

#[test]
fn desktop_role_shows_even_when_frozen() {
    // A floating window is suppressed while the screen is frozen, but the
    // DESKTOP surface itself must still show — it hosts the capture UI.
    let windows = [
        WindowState::floating(WindowId::App(0), true, 1),
        WindowState::desktop(WindowId::Desktop, true),
    ];
    assert_eq!(
        composition_order(&windows, true),
        vec![WindowId::Desktop],
        "desktop base shows; floating window suppressed while frozen"
    );
}

#[test]
fn desktop_role_stays_below_a_fullscreen_window() {
    // Banding is strict: DESKTOP < floating < FULLSCREEN. A fullscreen window
    // still composites (it's not the desktop), with the desktop beneath it.
    let mut chat = WindowState::floating(WindowId::App(0), true, 1);
    chat.fullscreen = true;
    let windows = [chat, WindowState::desktop(WindowId::Desktop, true)];
    assert_eq!(
        composition_order(&windows, false),
        vec![WindowId::Desktop, WindowId::App(0)],
        "desktop stays at the bottom band even under a fullscreen window"
    );
}

// ── WindowStack: the z/focus stack ──

fn stack() -> WindowStack {
    WindowStack::new(&[WindowId::App(1), WindowId::App(0)])
}

#[test]
fn stack_starts_hidden_and_unfocused() {
    let s = stack();
    assert_eq!(s.order(false).1, 0);
    assert_eq!(s.focused(), None);
    assert_eq!(s.top(), None);
    assert!(!s.is_visible(WindowId::App(0)));
}

#[test]
fn show_raises_and_focuses() {
    let mut s = stack();
    s.show(WindowId::App(1));
    assert_eq!(s.focused(), Some(WindowId::App(1)));
    assert!(s.is_top(WindowId::App(1)));
    // Opening chat puts it on top and moves focus.
    s.show(WindowId::App(0));
    assert_eq!(s.focused(), Some(WindowId::App(0)));
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(1), WindowId::App(0)]);
}

#[test]
fn raise_reorders_and_reports_change() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    // Raising the bottom window flips the order (the user's z bug: chat was
    // ALWAYS on top because emit order was hardcoded — raise must win now).
    assert!(s.raise(WindowId::App(1)));
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(0), WindowId::App(1)]);
    assert_eq!(s.focused(), Some(WindowId::App(1)));
    // Raising the already-top window changes nothing (no damage needed).
    assert!(!s.raise(WindowId::App(1)));
}

#[test]
fn hide_refocuses_topmost_remaining() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    s.hide(WindowId::App(0));
    assert_eq!(s.focused(), Some(WindowId::App(1)));
    assert!(s.is_top(WindowId::App(1)));
    s.hide(WindowId::App(1));
    assert_eq!(s.focused(), None);
    assert_eq!(s.order(false).1, 0);
}

#[test]
fn hit_order_is_reverse_of_composite_order() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    let (order, n) = s.order(false);
    let (hits, hn) = s.hit_order(false);
    assert_eq!(n, hn);
    let mut reversed = order;
    reversed[..n].reverse();
    assert_eq!(&hits[..hn], &reversed[..n]);
    // Front-to-back: the topmost (chat) is hit-tested first.
    assert_eq!(hits[0], WindowId::App(0));
}

#[test]
fn a_frozen_screen_suppresses_stack_windows_too() {
    let mut s = stack();
    s.show(WindowId::App(0));
    assert_eq!(s.order(true).1, 0);
    assert_eq!(s.hit_order(true).1, 0);
}

// ── Phase 2: minimize / dock / fullscreen ──

#[test]
fn minimize_leaves_composition_and_refocuses() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    s.minimize(WindowId::App(0));
    // Off screen but still OPEN (in the dock), search takes focus.
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(1)]);
    assert!(s.is_visible(WindowId::App(0)));
    assert!(s.is_minimized(WindowId::App(0)));
    assert_eq!(s.focused(), Some(WindowId::App(1)));
    let (dock, dn) = s.minimized_list();
    assert_eq!(&dock[..dn], &[WindowId::App(0)]);
}

#[test]
fn restore_returns_raised_and_focused() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    s.minimize(WindowId::App(0));
    s.restore(WindowId::App(0));
    assert!(!s.is_minimized(WindowId::App(0)));
    assert!(s.is_top(WindowId::App(0)));
    assert_eq!(s.focused(), Some(WindowId::App(0)));
    assert_eq!(s.minimized_list().1, 0);
}

#[test]
fn dock_order_is_stable_registration_order() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    // Minimize chat FIRST, then search — dock still lists registration order.
    s.minimize(WindowId::App(0));
    s.minimize(WindowId::App(1));
    let (dock, dn) = s.minimized_list();
    assert_eq!(&dock[..dn], &[WindowId::App(1), WindowId::App(0)]);
    assert_eq!(s.focused(), None);
    assert_eq!(s.order(false).1, 0);
}

#[test]
fn close_clears_dock_membership_and_fullscreen() {
    let mut s = stack();
    s.show(WindowId::App(0));
    s.set_fullscreen(WindowId::App(0), true);
    s.minimize(WindowId::App(0));
    s.hide(WindowId::App(0));
    assert_eq!(s.minimized_list().1, 0);
    assert!(!s.is_fullscreen(WindowId::App(0)));
    // Reopening starts floating, not fullscreen.
    s.show(WindowId::App(0));
    assert_eq!(s.fullscreen_active(), None);
}

#[test]
fn fullscreen_sorts_above_floating_and_reports_active() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    s.set_fullscreen(WindowId::App(1), true);
    // Search entered fullscreen → raised + focused + the ONLY composited
    // window (covered floating windows are excluded — user decision:
    // nothing but the wallpaper base beneath a fullscreen surface).
    assert_eq!(s.fullscreen_active(), Some(WindowId::App(1)));
    assert_eq!(s.focused(), Some(WindowId::App(1)));
    s.raise(WindowId::App(0));
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(1)], "fullscreen composites alone");
    // The fullscreen window stays on top of the raw z ordering too.
    assert_eq!(s.fullscreen_active(), Some(WindowId::App(1)));
    // Leaving fullscreen restores plain z ordering (chat was raised last).
    s.set_fullscreen(WindowId::App(1), false);
    assert_eq!(s.fullscreen_active(), None);
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(1), WindowId::App(0)]);
}

#[test]
fn fullscreen_excludes_covered_floating_windows() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    s.set_fullscreen(WindowId::App(0), true);
    // Only the fullscreen window composites (search is fully covered);
    // hit order follows, so the covered window is unreachable too.
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(0)]);
    let (hits, hn) = s.hit_order(false);
    assert_eq!(&hits[..hn], &[WindowId::App(0)]);
    // Leaving fullscreen brings the floating window back.
    s.set_fullscreen(WindowId::App(0), false);
    assert_eq!(s.order(false).1, 2);
}

#[test]
fn minimized_fullscreen_window_is_not_active_until_restored() {
    let mut s = stack();
    s.show(WindowId::App(0));
    s.set_fullscreen(WindowId::App(0), true);
    s.minimize(WindowId::App(0));
    // Docked → the chrome is NOT covered.
    assert_eq!(s.fullscreen_active(), None);
    s.restore(WindowId::App(0));
    // Restore returns to fullscreen (mode survives the dock).
    assert_eq!(s.fullscreen_active(), Some(WindowId::App(0)));
}

/// RFC-0114 §7: the stack grows with the windows — no registration cap; an order lists
/// the topmost `ORDER_MAX` (what one frame can draw), back-to-front.
#[test]
fn the_stack_grows_and_an_order_keeps_the_topmost() {
    let mut s = WindowStack::new(&[WindowId::Desktop]);
    s.show(WindowId::Desktop);
    let n = ORDER_MAX as u16 + 8;
    for i in 0..n {
        assert!(s.register(WindowId::App(i)));
        s.show(WindowId::App(i));
    }
    assert!(s.is_top(WindowId::App(n - 1)), "the last shown is on top");
    let (order, len) = s.order(false);
    assert_eq!(len, ORDER_MAX);
    assert_eq!(order[len - 1], WindowId::App(n - 1), "the topmost composes last");
    assert!(!order[..len].contains(&WindowId::App(0)), "the bottom-most give way");
    assert_eq!(order[0], WindowId::Desktop, "the base never gives way");
    s.raise(WindowId::App(0));
    let (order, len) = s.order(false);
    assert_eq!(order[len - 1], WindowId::App(0), "raised: drawn again, on top");
}

#[test]
fn z_normalization_preserves_order() {
    let mut s = stack();
    s.show(WindowId::App(1));
    s.show(WindowId::App(0));
    // Force many raises to cross the renormalization bound.
    for _ in 0..40_000 {
        s.raise(WindowId::App(1));
        s.raise(WindowId::App(0));
    }
    s.raise(WindowId::App(1));
    let (order, n) = s.order(false);
    assert_eq!(&order[..n], &[WindowId::App(0), WindowId::App(1)]);
    assert!(s.z_of(WindowId::App(1)) < Z_NORMALIZE_LIMIT);
    assert!(s.z_of(WindowId::App(0)) < Z_NORMALIZE_LIMIT);
}
