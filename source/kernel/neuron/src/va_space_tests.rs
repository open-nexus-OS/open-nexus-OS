// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host proofs for [`crate::va_space`] (RFC-0085) — the tests that would
//! catch a WRONG design, not merely exercise a correct one. Un-gated
//! sibling module (the `image_allocs` family), so `cargo test -p neuron`
//! runs every one of them on the host.

use crate::va_space::{
    superpage_plan, MapChunk, RegionKind, VaError, VaRegion, VaSpace, MAX_REGIONS, SUPERPAGE,
};

const PAGE: usize = 4096;
/// A small test window: policy code takes the window as data, so the tests
/// need no 768 MiB pretend-space. 0x10_0000 base, 64 MiB long.
const BASE: usize = 0x10_0000;
const LEN: usize = 64 * 1024 * 1024;

fn space() -> VaSpace {
    VaSpace::new(BASE, LEN)
}

fn vmo(va: usize, len: usize, pa: usize) -> VaRegion {
    VaRegion { va, len, pa, flags: 0xF, kind: RegionKind::Vmo }
}

/// ① Determinism: a fixed op sequence yields EXACT addresses, byte for byte.
/// This is the normative artifact — if first-fit, sorting, or the window
/// base ever change behavior, this fails before any boot does.
#[test]
fn golden_sequence_yields_exact_addresses() {
    let mut s = space();
    let a = s.find_hole(8 * PAGE, PAGE, 0).unwrap();
    assert_eq!(a, BASE);
    s.insert(vmo(a, 8 * PAGE, 0x8000_0000)).unwrap();

    let b = s.find_hole(4 * PAGE, PAGE, 0).unwrap();
    assert_eq!(b, BASE + 8 * PAGE);
    s.insert(vmo(b, 4 * PAGE, 0x8010_0000)).unwrap();

    let c = s.find_hole(SUPERPAGE, SUPERPAGE, 0).unwrap();
    assert_eq!(c, BASE + SUPERPAGE - (BASE % SUPERPAGE), "first 2M-aligned va past the tail");
    s.insert(vmo(c, SUPERPAGE, 0x8020_0000)).unwrap();
}

/// ② First-fit reuse — the anti-virgl-arena test. gpud's old slot allocator
/// was monotonic and never reused a released VA; the whole point of a real
/// allocator is that unmap gives the address back.
#[test]
fn unmapped_region_is_reused() {
    let mut s = space();
    let a = s.find_hole(16 * PAGE, PAGE, 0).unwrap();
    s.insert(vmo(a, 16 * PAGE, 0x8000_0000)).unwrap();
    let b = s.find_hole(16 * PAGE, PAGE, 0).unwrap();
    s.insert(vmo(b, 16 * PAGE, 0x8100_0000)).unwrap();
    assert_ne!(a, b);

    s.remove_exact(a, 16 * PAGE).unwrap();
    let c = s.find_hole(8 * PAGE, PAGE, 0).unwrap();
    assert_eq!(c, a, "a smaller mapping lands in the freed hole, lowest-first");
}

/// ③a Phase-aligned holes: the chosen va is 2 MiB-congruent with pa, which
/// is the precondition for superpage promotion.
#[test]
fn phase_aligned_hole_matches_pa_phase() {
    let mut s = space();
    // Occupy the window start so the hole search has to skip.
    s.insert(vmo(BASE, 3 * PAGE, 0x9000_0000)).unwrap();
    let pa = 0x8000_0000 + 5 * PAGE; // phase = 5 pages into a 2M frame
    let phase = pa & (SUPERPAGE - 1);
    let va = s.find_hole(4 * SUPERPAGE, SUPERPAGE, phase).unwrap();
    assert_eq!(va & (SUPERPAGE - 1), phase);
    assert!(va >= BASE + 3 * PAGE);
}

/// ③b The superpage plan tiles exactly: head + maximal 2M interior + tail,
/// no gap, no overlap — exhaustively over every page phase in a 2M frame.
#[test]
fn superpage_plan_tiles_exactly_for_every_phase() {
    let len = 8 * SUPERPAGE + 12 * PAGE;
    for phase_pages in 0..(SUPERPAGE / PAGE) {
        let off = phase_pages * PAGE;
        let va = 0x5000_0000 + off;
        let pa = 0x8000_0000 + off; // congruent by construction
        let chunks: [Option<MapChunk>; 3] = superpage_plan(va, pa, len);
        let mut cursor_va = va;
        let mut cursor_pa = pa;
        let mut super_bytes = 0;
        for chunk in chunks.into_iter().flatten() {
            assert_eq!(chunk.va, cursor_va, "no gap/overlap at phase {phase_pages}");
            assert_eq!(chunk.pa, cursor_pa);
            if chunk.superpage {
                assert_eq!(chunk.va % SUPERPAGE, 0);
                assert_eq!(chunk.pa % SUPERPAGE, 0);
                assert_eq!(chunk.len % SUPERPAGE, 0);
                super_bytes += chunk.len;
            }
            cursor_va += chunk.len;
            cursor_pa += chunk.len;
        }
        assert_eq!(cursor_va, va + len, "full coverage at phase {phase_pages}");
        // Maximality: interior superpages must cover every fully-contained
        // 2M frame.
        let head = if va % SUPERPAGE == 0 { 0 } else { SUPERPAGE - (va % SUPERPAGE) };
        let expected = ((len - head) / SUPERPAGE) * SUPERPAGE;
        assert_eq!(super_bytes, expected, "maximal promotion at phase {phase_pages}");
    }
}

/// ③c Incongruent va/pa never promote — a superpage over mismatched frames
/// would translate to the wrong physical pages.
#[test]
fn superpage_plan_refuses_incongruent_ranges() {
    let chunks = superpage_plan(0x5000_0000, 0x8000_0000 + PAGE, 8 * SUPERPAGE);
    let promoted = chunks.into_iter().flatten().any(|c| c.superpage);
    assert!(!promoted);
}

/// ④ Exhaustion leaves the allocator bit-identical: a refused map must not
/// perturb later decisions (determinism across failures).
#[test]
fn window_exhaustion_leaves_state_unchanged() {
    let mut s = space();
    let a = s.find_hole(LEN, PAGE, 0).unwrap();
    s.insert(vmo(a, LEN, 0x8000_0000)).unwrap();
    let before_count = s.region_count();

    assert!(s.find_hole(PAGE, PAGE, 0).is_none(), "window is full");
    assert_eq!(s.region_count(), before_count);
    // The allocator still answers the observability query correctly.
    assert_eq!(s.largest_free_hole(), 0);
}

/// ⑤ The 65th region is refused with NO phantom entry.
#[test]
fn table_full_refuses_without_phantom() {
    let mut s = space();
    for i in 0..MAX_REGIONS {
        let va = BASE + i * 2 * PAGE; // gaps, so no coalescing possibility
        s.insert(vmo(va, PAGE, 0x8000_0000 + i * PAGE)).unwrap();
    }
    let overflow_va = BASE + MAX_REGIONS * 2 * PAGE;
    assert_eq!(s.insert(vmo(overflow_va, PAGE, 0x9000_0000)), Err(VaError::TableFull));
    assert_eq!(s.region_count(), MAX_REGIONS);
    assert_eq!(s.peak_regions(), MAX_REGIONS);
}

/// ⑥ Bounded deterministic fuzz: after every op the invariant holds —
/// regions pairwise disjoint, sorted, kernel-chosen kinds in-window.
#[test]
fn fuzz_invariant_disjoint_sorted_in_window() {
    let mut s = space();
    let mut lcg: u64 = 0x2545_F491_4F6C_DD1D;
    let mut live: [Option<(usize, usize)>; 32] = [None; 32];
    for step in 0..2000 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let r = (lcg >> 33) as usize;
        let slot = r % live.len();
        match live[slot] {
            None => {
                let pages = 1 + (r >> 8) % 64;
                if let Some(va) = s.find_hole(pages * PAGE, PAGE, 0) {
                    s.insert(vmo(va, pages * PAGE, 0x8000_0000 + step * PAGE)).unwrap();
                    live[slot] = Some((va, pages * PAGE));
                }
            }
            Some((va, len)) => {
                s.remove_exact(va, len).unwrap();
                live[slot] = None;
            }
        }
        // Invariant walk after EVERY op.
        let mut prev_end = 0usize;
        for i in 0..s.region_count() {
            let (va, len) = s.region_at(i);
            assert!(va >= prev_end, "sorted + disjoint violated at step {step}");
            assert!(va >= BASE && va + len <= BASE + LEN, "in-window violated at step {step}");
            prev_end = va + len;
        }
    }
}

/// ⑦ Unmap discrimination: unknown va vs wrong length vs double-unmap.
#[test]
fn unmap_discriminates_not_found_and_len_mismatch() {
    let mut s = space();
    s.insert(vmo(BASE, 4 * PAGE, 0x8000_0000)).unwrap();

    assert_eq!(s.remove_exact(BASE + 16 * PAGE, PAGE), Err(VaError::NotFound));
    assert_eq!(s.remove_exact(BASE, 2 * PAGE), Err(VaError::LenMismatch));
    assert!(s.remove_exact(BASE, 4 * PAGE).is_ok());
    assert_eq!(s.remove_exact(BASE, 4 * PAGE), Err(VaError::NotFound), "double unmap");
}

/// ⑧ The invariant's negative test: a caller-chosen (fixed) VA inside the
/// managed window is refused — that is what keeps the hole-finder sound
/// without ever consulting the page table.
#[test]
fn fixed_map_into_managed_window_is_refused() {
    let s = space();
    assert_eq!(s.admits_fixed(BASE + PAGE, PAGE), Err(VaError::Occupied));
    // Straddling the window boundary is refused too.
    assert_eq!(s.admits_fixed(BASE - PAGE, 4 * PAGE), Err(VaError::Occupied));
    // Clear of the window: admitted.
    assert!(s.admits_fixed(0x2000_0000, 4 * PAGE).is_ok());
}

/// ⑨ Fixed-record coalescing: a page-by-page legacy loop collapses into one
/// record; differing flags break the chain; overflow counts.
#[test]
fn fixed_records_coalesce_and_overflow_counts() {
    let mut s = space();
    for page in 0..1000 {
        s.record_fixed(0x2000_0000 + page * PAGE, PAGE, 0x9000_0000 + page * PAGE, 0x7);
    }
    assert_eq!(s.region_count(), 1, "1000-page loop = ONE record");

    // A flag change starts a new record.
    s.record_fixed(0x2000_0000 + 1000 * PAGE, PAGE, 0x9000_0000 + 1000 * PAGE, 0x3);
    assert_eq!(s.region_count(), 2);

    // Fill the table with scattered fixed records, then overflow: counted.
    let mut i = 0;
    while s.region_count() < MAX_REGIONS {
        s.record_fixed(0x3000_0000 + i * 4 * PAGE, PAGE, 0xA000_0000 + i * PAGE, 0x7);
        i += 1;
    }
    let before = s.untracked_fixed();
    s.record_fixed(0x4000_0000, PAGE, 0xB000_0000, 0x7);
    assert_eq!(s.untracked_fixed(), before + 1, "overflow is counted, never forgotten");
}

/// ⑩ The vmo_destroy guard: live while mapped, clear after unmap; Fixed
/// regions never block a destroy (they are not VMO-backed userspace maps).
#[test]
fn destroy_guard_tracks_live_backings() {
    let mut s = space();
    s.insert(vmo(BASE, 8 * PAGE, 0x8000_0000)).unwrap();
    assert!(s.any_backed_by(0x8000_0000 + 4 * PAGE, PAGE));
    assert!(!s.any_backed_by(0x8000_0000 + 8 * PAGE, PAGE), "past the mapping");

    s.remove_exact(BASE, 8 * PAGE).unwrap();
    assert!(!s.any_backed_by(0x8000_0000, 8 * PAGE));

    s.record_fixed(0x2000_0000, PAGE, 0x8000_0000, 0x7);
    assert!(!s.any_backed_by(0x8000_0000, PAGE), "Fixed records do not block destroy");
}

/// ⑪ Edge guards: zero length, unaligned inputs, address-space overflow.
#[test]
fn edge_inputs_are_refused() {
    let mut s = space();
    assert!(s.find_hole(0, PAGE, 0).is_none());
    assert!(s.find_hole(PAGE, 3, 0).is_none(), "non-power-of-two align");
    assert!(s.find_hole(PAGE, PAGE, PAGE).is_none(), "phase >= align");
    assert_eq!(s.insert(vmo(BASE + 1, PAGE, 0)), Err(VaError::BadInput));
    assert_eq!(s.insert(vmo(BASE, PAGE + 1, 0)), Err(VaError::BadInput));
    assert_eq!(s.insert(vmo(usize::MAX & !0xFFF, 2 * PAGE, 0)), Err(VaError::BadInput));
    assert_eq!(s.remove_exact(BASE, 0), Err(VaError::BadInput));
}
