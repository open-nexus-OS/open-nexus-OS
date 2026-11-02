// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The surface's buffer as app-host draws it — the pure part (RFC-0114 §1, §4, §5):
//! where a row of a slot lies, which slot the next frame is drawn into, what of the last present
//! is copied forward into it (a slot holds the frame before last: two slots alternate), and the
//! byte spans a writer cleans for a GPU that does not snoop the caches. The mapping itself is
//! `probe/surface_mem.rs`. Same one-cfg shape as `band_math`: `probe/` is RISC-V-only, the
//! arithmetic lives here where host tests reach it.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below incl. `test_reject_*`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

/// A rectangle of the surface: `(x, y, w, h)` in surface pixels (base rows, then the store's).
pub(crate) type Rect = (u32, u32, u32, u32);

/// The most byte spans one frame cleans: its own rects and those it copied forward.
pub(crate) const MAX_SPANS: usize = 8;

/// The buffer's geometry (`OP_SURFACE_BUFFERS`): `slots` slots of `rows` rows at `stride`
/// bytes, each row `width` pixels wide at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlotLayout {
    pub(crate) stride: usize,
    pub(crate) rows: u32,
    pub(crate) width: u32,
    pub(crate) slots: u8,
}

impl SlotLayout {
    /// Byte offset of row `y` of slot `slot`; `None` past the slot's rows or the slots.
    pub(crate) fn row_at(&self, slot: u8, y: u32) -> Option<usize> {
        (slot < self.slots && y < self.rows)
            .then(|| (usize::from(slot) * self.rows as usize + y as usize) * self.stride)
    }

    /// The slot drawn after `slot`.
    pub(crate) fn next(&self, slot: u8) -> u8 {
        slot.wrapping_add(1) % self.slots.max(1)
    }

    /// Row `y` of slot `slot` as `[start, end)` bytes: `width × 4` bytes from its start.
    fn row_bytes(&self, slot: u8, y: u32) -> Option<(usize, usize)> {
        let at = self.row_at(slot, y)?;
        Some((at, at + self.width as usize * 4))
    }
}

/// Copies `rects` of slot `from` into slot `to` (the newer slot's pixels where the last present
/// changed them: `to` then holds the newest frame everywhere). A rect is cut to the slot.
pub(crate) fn copy_forward(
    bytes: &mut [u8],
    layout: &SlotLayout,
    (from, to): (u8, u8),
    rects: &[Rect],
) {
    if from == to {
        return;
    }
    for &(x, y, w, h) in rects {
        let x0 = x.min(layout.width) as usize * 4;
        let x1 = x.saturating_add(w).min(layout.width) as usize * 4;
        if x1 <= x0 {
            continue;
        }
        for row in y..y.saturating_add(h).min(layout.rows) {
            let (Some((src, _)), Some((dst, _))) =
                (layout.row_bytes(from, row), layout.row_bytes(to, row))
            else {
                continue;
            };
            if dst + x1 <= bytes.len() && src + x1 <= bytes.len() {
                bytes.copy_within(src + x0..src + x1, dst + x0);
            }
        }
    }
}

/// The byte spans of slot `slot` the `rects` cover — whole rows, overlapping and touching rows
/// merged, each one `vmo_clean` — into `out`. Returns how many.
pub(crate) fn clean_spans(
    layout: &SlotLayout,
    slot: u8,
    rects: &[Rect],
    out: &mut [(usize, usize); MAX_SPANS],
) -> usize {
    let mut rows = [(0u32, 0u32); MAX_SPANS];
    let mut n = 0;
    for &(_, y, _, h) in rects.iter().take(MAX_SPANS) {
        let (y0, y1) = (y.min(layout.rows), y.saturating_add(h).min(layout.rows));
        if y1 > y0 {
            rows[n] = (y0, y1);
            n += 1;
        }
    }
    rows[..n].sort_unstable();
    let mut m = 0;
    for i in 0..n {
        let (a, b) = rows[i];
        if m > 0 && a <= rows[m - 1].1 {
            rows[m - 1].1 = rows[m - 1].1.max(b);
        } else {
            rows[m] = (a, b);
            m += 1;
        }
    }
    let mut k = 0;
    for &(a, b) in &rows[..m] {
        if let Some(at) = layout.row_at(slot, a) {
            out[k] = (at, (b - a) as usize * layout.stride);
            k += 1;
        }
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: SlotLayout = SlotLayout { stride: 64, rows: 10, width: 12, slots: 2 };

    #[test]
    fn a_row_lies_in_its_slot_and_the_slots_alternate() {
        assert_eq!(L.row_at(0, 0), Some(0));
        assert_eq!(L.row_at(0, 9), Some(9 * 64));
        assert_eq!(L.row_at(1, 0), Some(10 * 64), "slot 1 starts after slot 0's rows");
        assert_eq!((L.next(0), L.next(1)), (1, 0));
    }

    #[test]
    fn test_reject_a_row_past_the_slot_or_the_slots() {
        assert_eq!(L.row_at(0, 10), None);
        assert_eq!(L.row_at(2, 0), None);
    }

    /// The last present's rects come forward: the older slot then holds the newest frame there,
    /// and nothing outside the rects changes.
    #[test]
    fn copy_forward_brings_the_changed_rects_into_the_older_slot() {
        let mut bytes = vec![0u8; 2 * 10 * 64];
        for row in 0..10 {
            let at = L.row_at(1, row).expect("row");
            bytes[at..at + 48].fill(row as u8 + 1);
        }
        copy_forward(&mut bytes, &L, (1, 0), &[(2, 3, 4, 2)]);
        let row3 = L.row_at(0, 3).expect("row");
        assert_eq!(&bytes[row3 + 8..row3 + 24], &[4u8; 16], "x 2..6 of row 3");
        assert_eq!(bytes[row3 + 4], 0, "x 1: outside the rect");
        assert_eq!(bytes[row3 + 24], 0, "x 6: outside the rect");
        let row5 = L.row_at(0, 5).expect("row");
        assert_eq!(bytes[row5 + 8], 0, "row 5: outside the rect");
        // A rect past the slot is cut to it; one slot onto itself is nothing.
        copy_forward(&mut bytes, &L, (1, 0), &[(10, 9, 100, 100)]);
        let row9 = L.row_at(0, 9).expect("row");
        assert_eq!(&bytes[row9 + 40..row9 + 48], &[10u8; 8]);
        copy_forward(&mut bytes, &L, (0, 0), &[(0, 0, 12, 10)]);
    }

    /// One clean per run of rows, whole rows of the stride, inside the slot.
    #[test]
    fn clean_spans_merge_rows_and_stay_in_the_slot() {
        let mut out = [(0, 0); MAX_SPANS];
        let n = clean_spans(&L, 1, &[(0, 2, 4, 2), (5, 3, 1, 2), (0, 8, 12, 9)], &mut out);
        assert_eq!(&out[..n], &[((10 + 2) * 64, 3 * 64), ((10 + 8) * 64, 2 * 64)]);
        let n = clean_spans(&L, 0, &[(0, 4, 2, 0), (0, 12, 2, 2)], &mut out);
        assert_eq!(n, 0, "empty and past-the-slot rects clean nothing");
    }
}
