// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0121 — where a surface's base holds pixels (its INK): at most [`MAX_INK`]
//! disjoint rectangles of the base, carried as an optional tail of the structure frame
//! (`OP_SURFACE_TREE`, `surface_layers`). A structure without the tail is [`INK_WHOLE`] — the
//! whole base, today's behaviour and every window's — and the encoder writes no tail for it, so
//! such a frame is byte-identical to one of before the field. The compositor draws the base only
//! inside the ink; the producer never under-declares (over-declaring costs fill, under-declaring
//! costs pixels).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: `surface_ink_tests.rs` (round trips, old frames, `test_reject_*` for the count,
//!   the reserved byte, a rect past the base, overlapping rects)
//! INVARIANTS: the count is checked before any rect is read; a decoded ink names at most
//!   `MAX_INK` rects; a validated ink lies inside the base and its rects share no pixel.

use crate::surface_layers::{get16, put16, TreeError};

/// Ink rectangles a surface may declare (RFC-0121).
pub const MAX_INK: usize = 8;
/// `ink_count` of a base that declares no ink: the whole base.
pub const INK_WHOLE: u8 = 0xFF;
// The count byte tells a declared count from `INK_WHOLE`.
const _: () = assert!(MAX_INK < INK_WHOLE as usize);

/// The tail's head: the count, one reserved byte (zero).
pub(crate) const INK_HEAD: usize = 2;
/// One ink rectangle on the wire.
pub(crate) const INK_BYTES: usize = 8;
/// The longest tail.
pub(crate) const INK_MAX_LEN: usize = INK_HEAD + MAX_INK * INK_BYTES;

/// A rectangle of the base in surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect16 {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect16 {
    pub const ZERO: Self = Self { x: 0, y: 0, w: 0, h: 0 };

    /// `(x, y, w, h)` widened.
    #[must_use]
    pub fn wide(&self) -> (u32, u32, u32, u32) {
        (u32::from(self.x), u32::from(self.y), u32::from(self.w), u32::from(self.h))
    }

    /// Pixels it holds.
    #[must_use]
    pub fn area(&self) -> u32 {
        u32::from(self.w) * u32::from(self.h)
    }

    /// Whether it shares a pixel with `o`.
    #[must_use]
    pub fn meets(&self, o: &Self) -> bool {
        let (a, b) = (self.wide(), o.wide());
        a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
    }
}

/// The tail of a structure whose ink is `rects[..count]` (`count` past `MAX_INK`, `INK_WHOLE`
/// included: no tail), written at `out[at..]`. Returns the tail's length.
pub(crate) fn put(out: &mut [u8], at: usize, rects: &[Rect16; MAX_INK], count: u8) -> usize {
    let n = usize::from(count);
    if n > MAX_INK {
        return 0;
    }
    out[at] = count;
    out[at + 1] = 0;
    let mut pos = at + INK_HEAD;
    for r in &rects[..n] {
        put16(out, pos, r.x);
        put16(out, pos + 2, r.y);
        put16(out, pos + 4, r.w);
        put16(out, pos + 6, r.h);
        pos += INK_BYTES;
    }
    pos - at
}

/// The ink of a structure whose fixed part ends at `tail` (`frame[tail..]` the tail, maybe
/// empty): `(rects, count)`. An empty tail is `INK_WHOLE`; otherwise the count — read first,
/// bounded before any rect — is at most `MAX_INK` (or `INK_WHOLE` with no rect), the reserved
/// byte zero and the frame exactly as long as the count says. `None`: malformed.
pub(crate) fn get(frame: &[u8], tail: usize) -> Option<([Rect16; MAX_INK], u8)> {
    let mut rects = [Rect16::ZERO; MAX_INK];
    let rest = frame.len().checked_sub(tail)?;
    if rest == 0 {
        return Some((rects, INK_WHOLE));
    }
    if rest < INK_HEAD || frame[tail + 1] != 0 {
        return None;
    }
    let count = frame[tail];
    let n = match count {
        INK_WHOLE => 0,
        c if usize::from(c) <= MAX_INK => usize::from(c),
        _ => return None,
    };
    if rest != INK_HEAD + n * INK_BYTES {
        return None;
    }
    let mut pos = tail + INK_HEAD;
    for slot in rects.iter_mut().take(n) {
        *slot = Rect16 {
            x: get16(frame, pos),
            y: get16(frame, pos + 2),
            w: get16(frame, pos + 4),
            h: get16(frame, pos + 6),
        };
        pos += INK_BYTES;
    }
    Some((rects, count))
}

/// The ink's check against the base (`w × h`): every rect has area and lies inside it, and no
/// two share a pixel (the compositor draws each over the wallpaper: a pixel in two pieces would
/// be blended twice).
pub(crate) fn check(rects: &[Rect16], w: u32, h: u32) -> Result<(), TreeError> {
    for (i, r) in rects.iter().enumerate() {
        let (x, y, rw, rh) = r.wide();
        if rw == 0 || rh == 0 || x + rw > w || y + rh > h {
            return Err(TreeError::InkOutside);
        }
        if rects[i + 1..].iter().any(|o| r.meets(o)) {
            return Err(TreeError::Overlap);
        }
    }
    Ok(())
}
