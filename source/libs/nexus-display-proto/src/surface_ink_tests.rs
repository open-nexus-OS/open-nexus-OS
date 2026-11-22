// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: unit tests for the RFC-0121 ink of a surface's structure (`surface_ink.rs`): the
//! round trip, a frame without the tail decoding as the whole base, and a `test_reject_*` for
//! every bound a receiver checks (the count, the reserved byte, the length, a rect past the base,
//! an empty rect, rects sharing a pixel).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_SCOPE: surface tree ink codec and validation

use crate::surface_layers::*;

const W: u16 = 1920;
const H: u16 = 1080;

fn r(x: u16, y: u16, w: u16, h: u16) -> Rect16 {
    Rect16 { x, y, w, h }
}

/// The desktop's base: a top bar, a dock, a card.
fn inked(rects: &[Rect16]) -> SurfaceTree {
    let mut t = SurfaceTree::EMPTY;
    t.ink[..rects.len()].copy_from_slice(rects);
    t.ink_count = rects.len() as u8;
    t
}

fn shell() -> SurfaceTree {
    inked(&[r(0, 0, 1920, 40), r(560, 980, 800, 92), r(100, 200, 300, 160)])
}

fn round(t: &SurfaceTree) -> (usize, Option<SurfaceTree>) {
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(5, t, &mut buf);
    let mut out = SurfaceTree::EMPTY;
    (len, decode_surface_tree(&buf[..len], &mut out).map(|_| out))
}

fn frame(t: &SurfaceTree) -> ([u8; SURFACE_TREE_MAX_LEN], usize) {
    let mut buf = [0u8; SURFACE_TREE_MAX_LEN];
    let len = encode_surface_tree(5, t, &mut buf);
    (buf, len)
}

#[test]
fn the_ink_round_trips_and_validates() {
    let t = shell();
    let (_, out) = round(&t);
    let out = out.expect("decodes");
    assert_eq!(out, t);
    assert_eq!(out.ink(), Some(&t.ink[..3]));
    assert_eq!(validate_tree(&out, W, H, H, 0), Ok(()));
    // No pixel: declared, and empty — no body.
    let (len, none) = round(&inked(&[]));
    assert_eq!(none.expect("decodes").ink(), Some(&[][..]));
    assert_eq!(len, round(&SurfaceTree::EMPTY).0 + 2, "the tail's head alone");
    // The bound itself fits, and the longest structure still fits windowd's receive buffer.
    let full: [Rect16; MAX_INK] = core::array::from_fn(|i| r(i as u16 * 100, 0, 90, 90));
    let (_, out) = round(&inked(&full));
    assert_eq!(out.expect("decodes").ink().map(<[Rect16]>::len), Some(MAX_INK));
    const _: () = assert!(SURFACE_TREE_MAX_LEN <= 512);
}

/// A window's structure, and every structure of before the field, is the whole base: the
/// encoder writes no tail for it, and a frame without one decodes as `INK_WHOLE`.
#[test]
fn a_structure_without_the_ink_is_the_whole_base() {
    let (len, out) = round(&SurfaceTree::EMPTY);
    assert_eq!(len, 12, "the frame of before the field: header, id, counts");
    let out = out.expect("decodes");
    assert_eq!(out.ink_count, INK_WHOLE);
    assert_eq!(out.ink(), None);
    // `INK_WHOLE` written explicitly with no rect is the same thing.
    let (mut buf, len) = frame(&SurfaceTree::EMPTY);
    buf[len] = INK_WHOLE;
    buf[len + 1] = 0;
    let mut out = SurfaceTree::EMPTY;
    assert_eq!(decode_surface_tree(&buf[..len + 2], &mut out), Some(5));
    assert_eq!(out.ink(), None);
}

#[test]
fn test_reject_an_ink_count_past_the_bound() {
    let (buf, len) = frame(&shell());
    let at = len - 2 - 3 * 8;
    let mut out = SurfaceTree::EMPTY;
    for count in [MAX_INK as u8 + 1, 0x80, INK_WHOLE - 1] {
        let mut over = buf;
        over[at] = count;
        assert_eq!(decode_surface_tree(&over[..len], &mut out), None, "count {count}");
    }
    // A count the length does not carry, either way.
    let mut more = buf;
    more[at] = 4;
    assert_eq!(decode_surface_tree(&more[..len], &mut out), None, "a rect missing");
    let mut less = buf;
    less[at] = 2;
    assert_eq!(decode_surface_tree(&less[..len], &mut out), None, "a rect too many");
    // `INK_WHOLE` carries no rect.
    let mut whole = buf;
    whole[at] = INK_WHOLE;
    assert_eq!(decode_surface_tree(&whole[..len], &mut out), None, "rects after INK_WHOLE");
    // The reserved byte, and a tail too short for its head.
    let mut reserved = buf;
    reserved[at + 1] = 1;
    assert_eq!(decode_surface_tree(&reserved[..len], &mut out), None, "reserved byte");
    assert_eq!(decode_surface_tree(&buf[..at + 1], &mut out), None, "a lone count byte");
    assert_eq!(out, SurfaceTree::EMPTY, "a refused frame leaves the output alone");
}

#[test]
fn test_reject_an_ink_rect_past_the_base() {
    let check = |rects: &[Rect16]| validate_tree(&inked(rects), W, H, H + 400, W);
    assert_eq!(check(&[r(0, 0, W, H)]), Ok(()), "the whole base");
    assert_eq!(check(&[r(1, 0, W, 10)]), Err(TreeError::InkOutside), "a column past");
    assert_eq!(check(&[r(0, 1000, 10, 81)]), Err(TreeError::InkOutside), "into the store");
    assert_eq!(check(&[r(u16::MAX, 0, 1, 1)]), Err(TreeError::InkOutside), "far past");
    assert_eq!(
        check(&[r(0, 0, 10, 10), r(u16::MAX, u16::MAX, u16::MAX, u16::MAX)]),
        Err(TreeError::InkOutside),
        "a later rect"
    );
    assert_eq!(check(&[r(5, 5, 0, 10)]), Err(TreeError::InkOutside), "empty");
    assert_eq!(check(&[r(5, 5, 10, 0)]), Err(TreeError::InkOutside), "empty");
    // Two pieces sharing a pixel would draw it twice.
    assert_eq!(check(&[r(0, 0, 10, 10), r(9, 9, 10, 10)]), Err(TreeError::Overlap));
    assert_eq!(check(&[r(0, 0, 10, 10), r(10, 0, 10, 10)]), Ok(()), "touching is not sharing");
    // A window's structure (`INK_WHOLE`) is checked as before.
    assert_eq!(validate_tree(&SurfaceTree::EMPTY, W, H, H, 0), Ok(()));
}
