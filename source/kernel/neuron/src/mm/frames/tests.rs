// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host proof of the frame allocator over the two golden trees
//! (`nexus-fdt/tests/goldens`: QEMU virt with one bank, the board with two
//! banks around a 2 GiB hole and an OpenSBI reservation) plus synthetic banks
//! for the edges the goldens do not exercise.

use super::{
    Block, FrameAllocator, FrameError, Range, FRAME_SIZE, MAX_BANKS, MAX_HOLES, MAX_ORDER,
    SUPERPAGE_ORDER,
};
use proptest::prelude::*;

const VIRT: &[u8] = include_bytes!("../../../../../libs/nexus-fdt/tests/goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("../../../../../libs/nexus-fdt/tests/goldens/bpi-f3.dtb");

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
const SUPER: u64 = FRAME_SIZE << SUPERPAGE_ORDER;

fn from_tree(bytes: &[u8], excluded: &[Range]) -> FrameAllocator {
    let fdt = nexus_fdt::Fdt::new(bytes).expect("golden parses");
    FrameAllocator::init_from_fdt(&fdt, excluded).expect("golden inits")
}

fn frames(bytes: u64) -> usize {
    (bytes / FRAME_SIZE) as usize
}

#[test]
fn virt_golden_is_one_bank_with_nothing_reserved() {
    let mut fa = from_tree(VIRT, &[]);
    let s = fa.stats();
    assert_eq!(s.banks, 1);
    assert_eq!(s.total, frames(320 * MIB));
    assert_eq!(s.free, s.total);
    assert_eq!((s.reserved, s.excluded), (0, 0));
    // The first superpage is the bank base itself: lowest-first.
    let b = fa.alloc(MAX_ORDER).unwrap();
    assert_eq!(b, Block { base: 0x8000_0000, order: MAX_ORDER });
    assert_eq!(fa.stats().free, s.total - b.frames());
}

#[test]
fn virt_golden_excludes_what_the_kernel_names() {
    // The image and the tree, as P2 will pass them: both leave the bank total
    // and neither is ever handed out.
    let image = Range { base: 0x8040_0000, size: 24 * MIB };
    let tree = Range { base: 0x8020_0000 + 0x100, size: 0x3000 };
    let mut fa = from_tree(VIRT, &[image, tree]);
    let s = fa.stats();
    assert_eq!(s.excluded, frames(24 * MIB) + 4);
    assert_eq!(s.total, frames(320 * MIB) - s.excluded);
    let mut seen = 0;
    while let Ok(b) = fa.alloc(0) {
        assert!(!(b.base >= image.base && b.base < image.base + image.size), "image frame {b:?}");
        assert!(!(b.base >= 0x8020_0000 && b.base < 0x8020_4000), "tree frame {b:?}");
        seen += 1;
    }
    assert_eq!(seen, s.total);
}

#[test]
fn board_golden_has_two_banks_around_the_hole() {
    let mut fa = from_tree(BOARD, &[]);
    let s = fa.stats();
    assert_eq!(s.banks, 2);
    assert_eq!(s.reserved, frames(0x8_0000), "OpenSBI's 512 KiB at 0");
    assert_eq!(s.total, frames(4 * GIB) - s.reserved);
    assert_eq!(fa.banks()[0].base(), 0);
    assert_eq!(fa.banks()[1].base(), 4 * GIB);
    // Bank 0's first superpage sits above the reservation, still 2 MiB-aligned.
    let b = fa.alloc(SUPERPAGE_ORDER).unwrap();
    assert_eq!(b, Block { base: SUPER, order: SUPERPAGE_ORDER });
    // The 512 KiB..2 MiB tail of the first superpage is free in smaller blocks.
    let tail = fa.alloc(7).unwrap();
    assert_eq!(tail, Block { base: 0x8_0000, order: 7 });
    // Nothing between the banks is ever handed out; bank 1 follows bank 0.
    let mut n = 0usize;
    let mut last = 0u64;
    while let Ok(b) = fa.alloc(SUPERPAGE_ORDER) {
        assert!(!(b.base >= 2 * GIB && b.base < 4 * GIB), "the hole {b:?}");
        assert!(b.base > last, "ascending");
        last = b.base;
        n += 1;
    }
    // Bank 0 minus the OpenSBI superpage and the one taken above, then all of bank 1.
    assert_eq!(n, (2 * GIB / SUPER) as usize - 2 + (2 * GIB / SUPER) as usize);
    assert_eq!(fa.stats().exhausted, 1);
}

#[test]
fn alloc_below_never_crosses_the_limit() {
    let mut fa = from_tree(BOARD, &[]);
    let mut n = 0usize;
    while let Ok(b) = fa.alloc_below(SUPERPAGE_ORDER, 4 * GIB) {
        assert!(b.end() <= 4 * GIB, "{b:?}");
        n += 1;
    }
    assert_eq!(n, (2 * GIB / SUPER) as usize - 1);
    // Bank 1 is untouched: its whole extent is still free.
    assert_eq!(fa.banks()[1].free_frames(), frames(2 * GIB));
    assert!(fa.alloc(MAX_ORDER).unwrap().base >= 4 * GIB);
}

#[test]
fn same_tree_same_calls_same_frames() {
    let script = [9u8, 0, 3, 9, 1, 0, 5, 2, 9, 0];
    let run = |excluded: &[Range]| {
        let mut fa = from_tree(BOARD, excluded);
        let mut out = alloc::vec::Vec::new();
        for (i, &order) in script.iter().enumerate() {
            let b = fa.alloc(order).unwrap();
            out.push(b);
            if i % 3 == 2 {
                fa.free(out[i - 1]).unwrap();
            }
        }
        out
    };
    let ex = [Range { base: 0x60_0000, size: 8 * MIB }];
    assert_eq!(run(&ex), run(&ex));
    assert_ne!(run(&ex), run(&[]), "a different exclusion is a different map");
}

#[test]
fn free_merges_back_to_the_initial_map() {
    let mut fa = from_tree(VIRT, &[]);
    let before = fa.stats();
    let first = fa.alloc(MAX_ORDER).unwrap();
    fa.free(first).unwrap();
    let mut live = alloc::vec::Vec::new();
    for order in [0u8, 1, 9, 4, 0, 8, 3, 9, 2, 6, 0, 0, 5] {
        live.push(fa.alloc(order).unwrap());
    }
    for b in live.iter().rev() {
        fa.free(*b).unwrap();
    }
    let after = fa.stats();
    assert_eq!(after.free, before.total);
    assert_eq!((after.allocs, after.frees), (14, 14));
    assert_eq!(fa.alloc(MAX_ORDER).unwrap(), first, "the map merged back to the same superpages");
}

#[test]
fn rejects_double_free_and_foreign_blocks() {
    let mut fa = from_tree(BOARD, &[]);
    let b = fa.alloc(3).unwrap();
    fa.free(b).unwrap();
    assert_eq!(fa.free(b), Err(FrameError::NotAllocated));
    // Merged into a bigger free block: still a double free.
    let big = fa.alloc(MAX_ORDER).unwrap();
    let inner = Block { base: big.base + 4 * FRAME_SIZE, order: 2 };
    fa.free(big).unwrap();
    assert_eq!(fa.free(inner), Err(FrameError::NotAllocated));
    // Outside every bank (the 2 GiB hole), misaligned, a hole, bad order.
    assert_eq!(fa.free(Block { base: 3 * GIB, order: 0 }), Err(FrameError::NotOwned));
    assert_eq!(fa.free(Block { base: SUPER + FRAME_SIZE, order: 1 }), Err(FrameError::NotOwned));
    assert_eq!(fa.free(Block { base: 0, order: 0 }), Err(FrameError::NotOwned), "OpenSBI");
    assert_eq!(fa.free(Block { base: SUPER, order: MAX_ORDER + 1 }), Err(FrameError::BadOrder));
    assert_eq!(fa.alloc(MAX_ORDER + 1), Err(FrameError::BadOrder));
    assert_eq!(fa.stats().frees, 2);
}

#[test]
fn exhaustion_is_an_error_and_a_counter() {
    let bank = Range { base: 0x8000_0000, size: 64 * FRAME_SIZE };
    let mut fa = FrameAllocator::init(&[bank], &[], &[]).unwrap();
    assert_eq!(fa.alloc(MAX_ORDER), Err(FrameError::Exhausted { order: MAX_ORDER, free: 64 }));
    let mut n = 0;
    while fa.alloc(0).is_ok() {
        n += 1;
    }
    assert_eq!(n, 64);
    assert_eq!(fa.alloc(0), Err(FrameError::Exhausted { order: 0, free: 0 }));
    // The superpage ask, the loop's last try, the explicit one.
    assert_eq!(fa.stats().exhausted, 3);
}

#[test]
fn a_fallback_to_smaller_blocks_is_served_not_exhausted() {
    // Two frames free, no order-1 block left: the order-1 ask is answered with
    // an order-0 block and neither the counter nor the caller sees exhaustion.
    let bank = Range { base: 0x8000_0000, size: 4 * FRAME_SIZE };
    let mut fa = FrameAllocator::init(&[bank], &[], &[]).unwrap();
    let a = fa.alloc(0).unwrap();
    let _b = fa.alloc(0).unwrap();
    let c = fa.alloc(0).unwrap();
    let _d = fa.alloc(0).unwrap();
    fa.free(a).unwrap();
    fa.free(c).unwrap(); // frames 0 and 2 free, their buddies taken: no order-1 block
    let got = fa.alloc_at_most(1).unwrap();
    assert_eq!(got, Block { base: 0x8000_0000, order: 0 }, "the largest that fits, lowest first");
    assert_eq!(fa.stats().exhausted, 0);
    // A whole block of the asked order is taken when one exists.
    let mut fresh = FrameAllocator::init(&[bank], &[], &[]).unwrap();
    assert_eq!(fresh.alloc_at_most(1).unwrap().order, 1);
}

#[test]
fn test_reject_alloc_at_most_counts_exhaustion_only_when_nothing_is_left() {
    // Three frames: an order-1 block and an order-0 block.
    let bank = Range { base: 0x8000_0000, size: 3 * FRAME_SIZE };
    let mut fa = FrameAllocator::init(&[bank], &[], &[]).unwrap();
    assert_eq!(fa.alloc_at_most(MAX_ORDER).map(|b| b.order), Ok(1));
    assert_eq!(fa.alloc_at_most(MAX_ORDER).map(|b| b.order), Ok(0));
    assert_eq!(fa.stats().exhausted, 0);
    assert_eq!(
        fa.alloc_at_most(MAX_ORDER),
        Err(FrameError::Exhausted { order: MAX_ORDER, free: 0 })
    );
    assert_eq!(fa.stats().exhausted, 1);
    assert_eq!(fa.alloc_at_most(MAX_ORDER + 1), Err(FrameError::BadOrder));
}

#[test]
fn holes_poison_whole_frames_and_are_never_handed_out() {
    let bank = Range { base: 0x8000_0000, size: 4 * MIB };
    let reserved = [Range { base: 0x8000_0000 + 0x1800, size: 0x100 }];
    let excluded = [Range { base: 0x8020_0000 - 0x10, size: 0x20 }];
    let mut fa = FrameAllocator::init(&[bank], &reserved, &excluded).unwrap();
    let s = fa.stats();
    assert_eq!((s.reserved, s.excluded), (1, 2));
    assert_eq!(s.total, frames(4 * MIB) - 3);
    let mut n = 0;
    while let Ok(b) = fa.alloc(0) {
        assert_ne!(b.base, 0x8000_1000);
        assert_ne!(b.base, 0x801f_f000);
        assert_ne!(b.base, 0x8020_0000);
        n += 1;
    }
    assert_eq!(n, s.total);
}

#[test]
fn an_unaligned_bank_base_keeps_superpages_aligned() {
    let bank = Range { base: 0x8010_0800, size: 6 * MIB };
    let mut fa = FrameAllocator::init(&[bank], &[], &[]).unwrap();
    let b = fa.alloc(SUPERPAGE_ORDER).unwrap();
    assert_eq!(b.base % SUPER, 0);
    assert!(b.base >= bank.base);
    assert_eq!(b.base, 0x8020_0000);
    // Half a frame at each end is not a frame: one frame of the six MiB is lost.
    assert_eq!(fa.stats().total, frames(6 * MIB) - 1);
}

#[test]
fn refuses_more_banks_or_holes_than_the_tables_hold() {
    let bank = Range { base: 0, size: MIB };
    let banks = [bank; MAX_BANKS + 1];
    assert_eq!(FrameAllocator::init(&banks, &[], &[]).err(), Some(FrameError::TooManyBanks));
    let holes = [Range { base: 0, size: FRAME_SIZE }; MAX_HOLES + 1];
    assert_eq!(FrameAllocator::init(&[bank], &holes, &[]).err(), Some(FrameError::TooManyHoles));
    assert_eq!(
        FrameAllocator::init(&[Range { base: u64::MAX - 8, size: 16 }], &[], &[]).err(),
        Some(FrameError::TooLarge)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any alloc/free script over a synthetic bank: live blocks never overlap,
    /// free never exceeds total, and freeing everything restores the map.
    #[test]
    fn live_blocks_never_overlap_and_everything_comes_back(
        script in prop::collection::vec((0u8..=MAX_ORDER, any::<bool>()), 1..200)
    ) {
        let bank = Range { base: 0x8000_0000, size: 16 * MIB };
        let hole = Range { base: 0x8040_0000, size: 3 * FRAME_SIZE };
        let mut fa = FrameAllocator::init(&[bank], &[hole], &[]).unwrap();
        let total = fa.stats().total;
        let mut live: alloc::vec::Vec<Block> = alloc::vec::Vec::new();
        for (order, do_free) in script {
            if do_free && !live.is_empty() {
                let b = live.swap_remove(order as usize % live.len());
                fa.free(b).unwrap();
            } else if let Ok(b) = fa.alloc(order) {
                prop_assert!(b.base % b.size() == 0);
                prop_assert!(b.base >= hole.base + hole.size || b.end() <= hole.base);
                for other in &live {
                    prop_assert!(b.end() <= other.base || other.end() <= b.base, "{b:?} vs {other:?}");
                }
                live.push(b);
            }
            let used: usize = live.iter().map(|b| b.frames()).sum();
            prop_assert_eq!(fa.stats().free + used, total);
        }
        for b in live.drain(..) {
            fa.free(b).unwrap();
        }
        prop_assert_eq!(fa.stats().free, total);
    }
}

#[test]
fn virt_boot_shape_takes_the_smallest_sufficient_order_then_the_lowest_address() {
    // The exclusions the kernel passes on QEMU virt (TASK-0286 P2b): the
    // firmware reservation, the loader's tree copy, the image, the two fixed
    // windows, the stack pool and the bootstrap identity window. Free: the
    // rest of the loader's window (from 0x8021_c000, whose first block is
    // order 2), the tail behind the image (from 0x815b_b000: one order-0
    // block first, then bigger ones) and the top of the bank.
    let reserved = [Range { base: 0x8000_0000, size: 0x6_0000 }];
    let excluded = [
        Range { base: 0x8020_0000, size: 0x1_c000 },
        Range { base: 0x8040_0000, size: 0x11b_ab80 },
        Range { base: 0x8200_0000, size: 24 * MIB },
        Range { base: 0x8380_0000, size: 224 * MIB },
        Range { base: 0x8010_0000, size: MIB },
        Range { base: 0x8000_0000, size: MIB },
    ];
    let bank = Range { base: 0x8000_0000, size: 320 * MIB };
    let mut fa = FrameAllocator::init(&[bank], &reserved, &excluded).unwrap();
    // Classic buddy policy: an exact-size free block anywhere beats splitting a
    // bigger one lower down — the image tail's lone order-0 block goes first,
    // then the loader window's order-2 block is split from its bottom.
    assert_eq!(fa.alloc(0).unwrap().base, 0x815b_b000);
    assert_eq!(fa.alloc(0).unwrap().base, 0x8021_c000);
    assert_eq!(fa.alloc(0).unwrap().base, 0x8021_d000);
    // Same shape, same calls, same frames; nothing from an excluded range.
    let mut again = FrameAllocator::init(&[bank], &reserved, &excluded).unwrap();
    for _ in 0..3 {
        again.alloc(0).unwrap();
    }
    let mut n = 0;
    while let (Ok(a), Ok(b)) = (fa.alloc(0), again.alloc(0)) {
        assert_eq!(a, b);
        assert!(!(0x8040_0000..0x815b_b000).contains(&a.base), "image frame {a:?}");
        assert!(!(0x8200_0000..0x9180_0000).contains(&a.base), "window frame {a:?}");
        n += 1;
    }
    assert_eq!(n + 3, fa.stats().total);
}
