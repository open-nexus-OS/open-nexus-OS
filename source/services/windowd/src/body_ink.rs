// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the desktop body as pieces (RFC-0121) — the shell's and the greeter's base is
//! composed only where the app declared it holds pixels: one piece per ink rectangle
//! (`KIND_BODY`, index `i`), the one full-screen piece of before for a base that declares none
//! (`INK_WHOLE`), no piece for an empty ink. Every piece is cut to the surface's base and the
//! display — a declaration can only leave the app's own pixels out, never sample past its
//! surface. The pieces join the desktop's layer share all or none (`scene/quotas.rs`): short of
//! it, the one full-screen piece. Pure, host-TESTED; the OS-only scene builder draws the pieces
//! and the present path says the line.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below
//! INVARIANTS: a piece lies inside the surface's base and the display; pieces of a validated ink
//!   share no pixel; at most `MAX_INK` pieces.

use nexus_display_proto::surface_layers::{Rect16, MAX_INK};

/// A rectangle `(x, y, w, h)` in surface pixels (the desktop's surface is at the display's
/// origin).
pub(crate) type Rect = (u32, u32, u32, u32);

/// The desktop body's pieces: the most a frame draws.
pub(crate) type Pieces = [Rect; MAX_INK];

// The quota counts the pieces beside the one body slot every frame keeps: the bound fits the
// frame's layers with room for the bodies of every window it can hold.
const _: () = assert!(MAX_INK < crate::layer_quota::FRAME_LAYERS / 4);

/// `r` cut to `(0, 0, w, h)`; `None` when nothing of it is left.
fn cut((x, y, rw, rh): Rect, (w, h): (u32, u32)) -> Option<Rect> {
    let (x1, y1) = (x.saturating_add(rw).min(w), y.saturating_add(rh).min(h));
    (x1 > x && y1 > y).then(|| (x, y, x1 - x, y1 - y))
}

/// The pieces of a body of `base` size (already cut at the display) whose ink is `ink`
/// (`None`: the whole base), written into `out`: each cut to the base; empties dropped. Returns
/// how many.
pub(crate) fn pieces(ink: Option<&[Rect16]>, base: (u32, u32), out: &mut Pieces) -> usize {
    let Some(ink) = ink else {
        return match cut((0, 0, base.0, base.1), base) {
            Some(r) => {
                out[0] = r;
                1
            }
            None => 0,
        };
    };
    let mut n = 0;
    for r in ink.iter().take(MAX_INK) {
        if let Some(c) = cut(r.wide(), base) {
            out[n] = c;
            n += 1;
        }
    }
    n
}

/// The layers the pieces take beyond the body slot every frame keeps: what the desktop's share
/// must also cover for them to be drawn (all or none).
pub(crate) fn extra(ink: Option<&[Rect16]>, base: (u32, u32)) -> usize {
    let mut out = [(0, 0, 0, 0); MAX_INK];
    pieces(ink, base, &mut out).saturating_sub(1)
}

/// Whether the body takes its slot at all: a declared empty ink draws no body.
pub(crate) fn drawn(ink: Option<&[Rect16]>) -> bool {
    ink.is_none_or(|i| !i.is_empty())
}

/// The line said when the desktop's ink changes (`windowd: body ink (n= px=)`): how many
/// pieces and the pixels they hold; `n=whole` for a base that declares none. Counts only.
pub(crate) fn line(ink: Option<&[Rect16]>, base: (u32, u32)) -> alloc::string::String {
    let mut out = [(0, 0, 0, 0); MAX_INK];
    let n = pieces(ink, base, &mut out);
    let px: u64 = out[..n].iter().map(|r| u64::from(r.2) * u64::from(r.3)).sum();
    match ink {
        None => alloc::format!("windowd: body ink (n=whole px={px})"),
        Some(_) => alloc::format!("windowd: body ink (n={n} px={px})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: (u32, u32) = (1920, 1080);

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect16 {
        Rect16 { x, y, w, h }
    }

    fn of(ink: Option<&[Rect16]>, base: (u32, u32)) -> alloc::vec::Vec<Rect> {
        let mut out = [(0, 0, 0, 0); MAX_INK];
        let n = pieces(ink, base, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn a_base_without_ink_is_the_one_full_screen_piece_of_before() {
        assert_eq!(of(None, BASE), [(0, 0, 1920, 1080)]);
        assert_eq!(extra(None, BASE), 0, "the body slot alone");
        assert!(drawn(None));
        assert_eq!(line(None, BASE), "windowd: body ink (n=whole px=2073600)");
    }

    #[test]
    fn each_ink_rect_is_one_piece_and_an_empty_ink_draws_no_body() {
        let ink = [r(0, 0, 1920, 40), r(560, 980, 800, 92)];
        assert_eq!(of(Some(&ink), BASE), [(0, 0, 1920, 40), (560, 980, 800, 92)]);
        assert_eq!(extra(Some(&ink), BASE), 1);
        assert_eq!(line(Some(&ink), BASE), "windowd: body ink (n=2 px=150400)");
        assert!(of(Some(&[]), BASE).is_empty());
        assert!(!drawn(Some(&[])));
        assert_eq!(extra(Some(&[]), BASE), 0);
        assert_eq!(line(Some(&[]), BASE), "windowd: body ink (n=0 px=0)");
    }

    /// The structure's check keeps the ink inside the base; the scene cuts every piece to the
    /// base it draws again (the surface may be larger than the display, the display may shrink
    /// under a kept structure): no piece ever samples past the surface.
    #[test]
    fn test_reject_a_body_piece_outside_the_surface() {
        let ink = [
            r(1900, 1000, 100, 200),
            r(u16::MAX - 1, 0, u16::MAX, 10),
            r(0, 0, 10, 10),
            r(5, 2000, 10, 10),
            r(0, 50, 0, 10),
        ];
        let got = of(Some(&ink), BASE);
        assert_eq!(got, [(1900, 1000, 20, 80), (0, 0, 10, 10)]);
        for p in &got {
            assert!(p.0 + p.2 <= BASE.0 && p.1 + p.3 <= BASE.1, "{p:?} past the surface");
        }
        assert!(of(None, (0, 1080)).is_empty(), "a base with no columns draws nothing");
        // More rects than the bound are never read.
        let many = [r(0, 0, 1, 1); MAX_INK + 4];
        assert_eq!(of(Some(&many), BASE).len(), MAX_INK);
    }

    /// A piece whose rectangle changes is a structure change (RFC-0101 §8 R4): where the pieces
    /// that went were and where those that came are — the ones kept name nothing; the whole
    /// base stands for the full-screen piece of an ink not declared.
    #[test]
    fn a_changed_piece_is_a_structure_change_where_it_was_and_is() {
        use crate::layer_geometry::SurfaceLayers;
        use nexus_display_proto::surface_layers::{SurfaceTree, INK_WHOLE};
        let tree = |ink: &[Rect16]| {
            let mut t = SurfaceTree::EMPTY;
            t.ink[..ink.len()].copy_from_slice(ink);
            t.ink_count = ink.len() as u8;
            t
        };
        let (bar, dock, card) = (r(0, 0, 1920, 40), r(560, 980, 800, 92), r(100, 200, 300, 160));
        let mut layers = SurfaceLayers::EMPTY;
        layers.stage(tree(&[bar, dock]));
        let first = layers.activate();
        assert!(first.ink, "from the whole base to two pieces");
        assert!(first.rects().contains(&(0, 0, 65535, 65535)));
        layers.stage(tree(&[bar, card]));
        let change = layers.activate();
        assert!(change.ink && !change.whole);
        assert_eq!(change.rects(), [dock.wide(), card.wide()], "the dock went, the card came");
        layers.stage(tree(&[bar, card]));
        assert!(!layers.activate().ink, "the same ink changes nothing");
        let mut whole = tree(&[]);
        whole.ink_count = INK_WHOLE;
        layers.stage(whole);
        let back = layers.activate();
        assert!(back.ink && back.rects().len() == 3, "two pieces went, the whole base came");
    }
}
