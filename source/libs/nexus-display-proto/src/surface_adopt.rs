// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A restarted compositor adopts the app surfaces gpud kept (RFC-0112 Phase 3,
//! TASK-0348 P9). When windowd dies, gpud keeps every app's object — the pixels the apps drew
//! last — and remembers where the last frame drew each one (its body's place, slot and z).
//! The new windowd asks once at its start (`OP_SURFACE_ADOPT`, after its framebuffer grant and
//! its GPU context) and gets the placed objects back in pages: enough to show every window
//! where it was, with its last pixels, until its app presents again. Objects no frame showed
//! (a minimized window's) are released by gpud instead — a minimized window has no ghost.
//! gpud serves windowd alone (identity = the kernel's sender id); the decode is fail-closed:
//! an entry past its object or of no owner refuses the whole page.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal v1
//! TEST_COVERAGE: the goldens and `test_reject_*` below

use crate::surface_alloc::{OP_SURFACE_ADOPT, SLOTS_MAX};

/// `OP_SURFACE_ADOPT`'s request: the op alone — the page is whatever gpud still holds.
pub const ADOPT_REQUEST_LEN: usize = 1;
/// The answer's first byte (above `0x80`: never a present ack's status, never `ALLOCATED_TAG`).
pub const ADOPTED_TAG: u8 = 0x80 | OP_SURFACE_ADOPT;
/// The answer's header: `[ADOPTED_TAG, restarted u8, count u8, remaining u16]`.
pub const ADOPTED_HEADER_LEN: usize = 5;
/// One entry: `[buffer u32, owner u64, width u16, rows u16, stride u32, slots u8, cache_block
/// u16, dst_x u16, dst_y u16, w u16, h u16, slot u8, z u8, window_key u16]`.
pub const ADOPT_ENTRY_LEN: usize = 35;
/// Entries per page: a page fits windowd's 512-byte reply buffer.
pub const ADOPT_PAGE_MAX: usize = 14;
/// A full page's length.
pub const ADOPTED_MAX_LEN: usize = ADOPTED_HEADER_LEN + ADOPT_PAGE_MAX * ADOPT_ENTRY_LEN;

/// One object the new compositor takes over: the object (as `Allocated` describes one) and
/// where the last frame drew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adopted {
    /// gpud's id of the object (a layer's `source`; never 0).
    pub buffer: u32,
    /// The app's kernel id: who owns the window the ghost stands for (never 0).
    pub owner: u64,
    pub width: u16,
    pub rows: u16,
    pub stride: u32,
    pub slots: u8,
    pub cache_block: u16,
    /// The body's place on the screen in the last frame, and its size there.
    pub dst_x: u16,
    pub dst_y: u16,
    pub w: u16,
    pub h: u16,
    /// The slot that frame read, and the body's place in the frame's draw order (back to front).
    pub slot: u8,
    pub z: u8,
    /// The window's wire key in the old compositor (`0xFFFF`: the desktop surface).
    pub window_key: u16,
}

#[must_use]
pub const fn encode_adopt_request() -> [u8; ADOPT_REQUEST_LEN] {
    [OP_SURFACE_ADOPT]
}

/// Whether `frame` is a well-formed `OP_SURFACE_ADOPT` request (length-exact).
#[must_use]
pub fn is_adopt_request(frame: &[u8]) -> bool {
    frame == [OP_SURFACE_ADOPT]
}

/// Whether an entry describes a body inside its own object.
#[must_use]
pub fn entry_fits(a: &Adopted) -> bool {
    a.buffer != 0
        && a.owner != 0
        && (1..=SLOTS_MAX).contains(&a.slots)
        && a.slot < a.slots
        && a.width > 0
        && a.rows > 0
        && a.stride >= u32::from(a.width) * 4
        && (1..=a.width).contains(&a.w)
        && (1..=a.rows).contains(&a.h)
}

/// Builds one page of the answer, entry by entry.
pub struct AdoptedPage {
    buf: [u8; ADOPTED_MAX_LEN],
    count: usize,
}

impl Default for AdoptedPage {
    fn default() -> Self {
        Self::new()
    }
}

impl AdoptedPage {
    #[must_use]
    pub const fn new() -> Self {
        Self { buf: [0u8; ADOPTED_MAX_LEN], count: 0 }
    }

    /// Entries the page holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether one more entry fits.
    #[must_use]
    pub const fn has_room(&self) -> bool {
        self.count < ADOPT_PAGE_MAX
    }

    /// Appends `a`; `false` when the page is full or `a` describes no body inside its object
    /// (nothing appended).
    pub fn push(&mut self, a: &Adopted) -> bool {
        if !self.has_room() || !entry_fits(a) {
            return false;
        }
        let at = ADOPTED_HEADER_LEN + self.count * ADOPT_ENTRY_LEN;
        let e = &mut self.buf[at..at + ADOPT_ENTRY_LEN];
        e[0..4].copy_from_slice(&a.buffer.to_le_bytes());
        e[4..12].copy_from_slice(&a.owner.to_le_bytes());
        e[12..14].copy_from_slice(&a.width.to_le_bytes());
        e[14..16].copy_from_slice(&a.rows.to_le_bytes());
        e[16..20].copy_from_slice(&a.stride.to_le_bytes());
        e[20] = a.slots;
        e[21..23].copy_from_slice(&a.cache_block.to_le_bytes());
        e[23..25].copy_from_slice(&a.dst_x.to_le_bytes());
        e[25..27].copy_from_slice(&a.dst_y.to_le_bytes());
        e[27..29].copy_from_slice(&a.w.to_le_bytes());
        e[29..31].copy_from_slice(&a.h.to_le_bytes());
        e[31] = a.slot;
        e[32] = a.z;
        e[33..35].copy_from_slice(&a.window_key.to_le_bytes());
        self.count += 1;
        true
    }

    /// The page's bytes: `restarted` says a compositor ran before this one, `remaining` how
    /// many placed objects gpud still holds after this page (0: this is the last page).
    pub fn finish(&mut self, restarted: bool, remaining: u16) -> &[u8] {
        self.buf[0] = ADOPTED_TAG;
        self.buf[1] = u8::from(restarted);
        self.buf[2] = self.count as u8;
        self.buf[3..5].copy_from_slice(&remaining.to_le_bytes());
        &self.buf[..ADOPTED_HEADER_LEN + self.count * ADOPT_ENTRY_LEN]
    }
}

/// Whether `frame` starts like an adopt answer (its tag): what a matched wait on gpud's reply
/// route accepts before the full decode.
#[must_use]
pub fn is_adopted(frame: &[u8]) -> bool {
    frame.first() == Some(&ADOPTED_TAG)
}

/// Decodes one page into `out`: `(restarted, remaining, count)`. Length-exact for its count;
/// refuses a count past the page, a tag of another answer, and any entry whose body lies past
/// its object or that names no owner — the whole page then, never a part of it.
#[must_use]
pub fn decode_adopted(
    frame: &[u8],
    out: &mut [Adopted; ADOPT_PAGE_MAX],
) -> Option<(bool, u16, usize)> {
    if frame.len() < ADOPTED_HEADER_LEN || frame[0] != ADOPTED_TAG || frame[1] > 1 {
        return None;
    }
    let count = usize::from(frame[2]);
    if count > ADOPT_PAGE_MAX || frame.len() != ADOPTED_HEADER_LEN + count * ADOPT_ENTRY_LEN {
        return None;
    }
    let remaining = u16::from_le_bytes([frame[3], frame[4]]);
    for (i, slot) in out.iter_mut().take(count).enumerate() {
        let at = ADOPTED_HEADER_LEN + i * ADOPT_ENTRY_LEN;
        let e = &frame[at..at + ADOPT_ENTRY_LEN];
        let u16_at = |p: usize| u16::from_le_bytes([e[p], e[p + 1]]);
        let u32_at = |p: usize| u32::from_le_bytes([e[p], e[p + 1], e[p + 2], e[p + 3]]);
        let mut owner = [0u8; 8];
        owner.copy_from_slice(&e[4..12]);
        let a = Adopted {
            buffer: u32_at(0),
            owner: u64::from_le_bytes(owner),
            width: u16_at(12),
            rows: u16_at(14),
            stride: u32_at(16),
            slots: e[20],
            cache_block: u16_at(21),
            dst_x: u16_at(23),
            dst_y: u16_at(25),
            w: u16_at(27),
            h: u16_at(29),
            slot: e[31],
            z: e[32],
            window_key: u16_at(33),
        };
        if !entry_fits(&a) {
            return None;
        }
        *slot = a;
    }
    Some((frame[1] == 1, remaining, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Adopted = Adopted {
        buffer: 0,
        owner: 0,
        width: 0,
        rows: 0,
        stride: 0,
        slots: 0,
        cache_block: 0,
        dst_x: 0,
        dst_y: 0,
        w: 0,
        h: 0,
        slot: 0,
        z: 0,
        window_key: 0,
    };

    fn window() -> Adopted {
        Adopted {
            buffer: 0x1_0007,
            owner: 0xA11CE,
            width: 1472,
            rows: 2432,
            stride: 5888,
            slots: 2,
            cache_block: 64,
            dst_x: 120,
            dst_y: 48,
            w: 1440,
            h: 900,
            slot: 1,
            z: 3,
            window_key: 0x2A,
        }
    }

    fn desktop() -> Adopted {
        Adopted {
            buffer: 9,
            owner: 0xDE5C,
            width: 1920,
            rows: 1088,
            stride: 7680,
            slots: 2,
            cache_block: 0,
            dst_x: 0,
            dst_y: 0,
            w: 1920,
            h: 1080,
            slot: 0,
            z: 0,
            window_key: 0xFFFF,
        }
    }

    #[test]
    fn a_page_round_trips_with_its_flags_and_its_count() {
        assert!(is_adopt_request(&encode_adopt_request()));
        let mut page = AdoptedPage::new();
        assert!(page.push(&desktop()) && page.push(&window()));
        let bytes = page.finish(true, 5).to_vec();
        assert_eq!(bytes.len(), ADOPTED_HEADER_LEN + 2 * ADOPT_ENTRY_LEN);
        assert!(is_adopted(&bytes));
        let mut out = [NONE; ADOPT_PAGE_MAX];
        assert_eq!(decode_adopted(&bytes, &mut out), Some((true, 5, 2)));
        assert_eq!((out[0], out[1]), (desktop(), window()));
        let mut empty = AdoptedPage::new();
        let bytes = empty.finish(false, 0).to_vec();
        assert_eq!(decode_adopted(&bytes, &mut out), Some((false, 0, 0)), "an empty last page");
        assert!(ADOPTED_MAX_LEN <= 512, "a full page fits the reply buffer");
    }

    #[test]
    fn a_full_page_takes_no_more() {
        let mut page = AdoptedPage::new();
        for _ in 0..ADOPT_PAGE_MAX {
            assert!(page.push(&window()));
        }
        assert!(!page.has_room() && !page.push(&window()));
        assert_eq!(page.len(), ADOPT_PAGE_MAX);
    }

    #[test]
    fn test_reject_a_page_whose_entry_lies_past_its_object_or_names_no_owner() {
        let bad = [
            Adopted { buffer: 0, ..window() },
            Adopted { owner: 0, ..window() },
            Adopted { slot: 2, ..window() },
            Adopted { slots: SLOTS_MAX + 1, ..window() },
            Adopted { w: 1473, ..window() },
            Adopted { h: 2433, ..window() },
            Adopted { w: 0, ..window() },
            Adopted { stride: 5887, ..window() },
        ];
        for a in &bad {
            assert!(!entry_fits(a), "{a:?}");
            let mut page = AdoptedPage::new();
            assert!(!page.push(a), "{a:?}");
            // Forged on the wire: the whole page is refused.
            let mut forged = AdoptedPage::new();
            assert!(forged.push(&desktop()));
            let mut bytes = forged.finish(false, 0).to_vec();
            let mut e = AdoptedPage::new();
            let _ = e.push(&window());
            let good = e.finish(false, 0).to_vec();
            bytes.extend_from_slice(&good[ADOPTED_HEADER_LEN..]);
            bytes[2] = 2;
            let at = ADOPTED_HEADER_LEN + ADOPT_ENTRY_LEN;
            bytes[at..at + 4].copy_from_slice(&a.buffer.to_le_bytes());
            bytes[at + 4..at + 12].copy_from_slice(&a.owner.to_le_bytes());
            bytes[at + 16..at + 20].copy_from_slice(&a.stride.to_le_bytes());
            bytes[at + 20] = a.slots;
            bytes[at + 27..at + 29].copy_from_slice(&a.w.to_le_bytes());
            bytes[at + 29..at + 31].copy_from_slice(&a.h.to_le_bytes());
            bytes[at + 31] = a.slot;
            let mut out = [NONE; ADOPT_PAGE_MAX];
            assert_eq!(decode_adopted(&bytes, &mut out), None, "{a:?}");
        }
    }

    #[test]
    fn test_reject_a_short_a_long_a_foreign_or_an_overfull_page() {
        let mut page = AdoptedPage::new();
        assert!(page.push(&window()));
        let bytes = page.finish(false, 0).to_vec();
        let mut out = [NONE; ADOPT_PAGE_MAX];
        assert!(decode_adopted(&bytes[..bytes.len() - 1], &mut out).is_none(), "short");
        let mut long = bytes.clone();
        long.push(0);
        assert!(decode_adopted(&long, &mut out).is_none(), "long");
        let mut foreign = bytes.clone();
        foreign[0] = crate::surface_alloc::ALLOCATED_TAG;
        assert!(decode_adopted(&foreign, &mut out).is_none(), "another answer's tag");
        let mut overfull = bytes.clone();
        overfull[2] = ADOPT_PAGE_MAX as u8 + 1;
        assert!(decode_adopted(&overfull, &mut out).is_none(), "count past the page");
        let mut flag = bytes;
        flag[1] = 2;
        assert!(decode_adopted(&flag, &mut out).is_none(), "a flag that is no bool");
        assert!(!is_adopt_request(&[OP_SURFACE_ADOPT, 0]), "a long request");
        assert!(decode_adopted(&[], &mut out).is_none(), "empty");
    }
}
