// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: size-class free lists over blocks the bump handed out — the half
//! of ADR-0065 that makes DURABLE state reclaimable (TASK-0077C P3).
//!
//! The frame arena made the frame flat. What still advanced the base heap —
//! measured at 432 B per layout on a real boot, 200–256 B per structural
//! dispatch on the host — is the reduce path: a dispatch queue and a
//! changed-field list built fresh per dispatch, and above all a store field
//! overwritten with a new `Value::Str` whose predecessor's bytes stay on a
//! heap that never frees. None of that belongs in a frame arena; durable state
//! must not. So the base heap has to free.
//!
//! WHY SEGREGATED FITS AND NOT A GENERAL ALLOCATOR: fixed classes, LIFO reuse,
//! no splitting, no coalescing, no search. Every operation is O(1) and every
//! outcome is a function of the request sequence alone — the property every OS
//! service relies on, and the reason ADR-0065 rejected a general free-list on
//! every service's floor. Internal waste is bounded by class rounding (< 2×),
//! and a class's memory is reused by that class for the life of the process,
//! so a session's working set is bounded by its peak per class. This is what
//! real small-object allocators do; it is not novel, which is the point.
//!
//! The rule is intrusive — a freed block's first word links to the next — and
//! is proven on the host over a plain byte buffer. The OS glue in `lib.rs`
//! consults it before the bump and after the arena check, in every service
//! (TASK-0333 6b-3: no service's heap is a bump that never frees). A block on a
//! list is marked in a bitmap over the base heap: a second free of it is
//! refused — its link would close the list on itself and hand the block to two
//! owners — and counted.
//!
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below (host)
//! ADR: docs/adr/0065-app-host-frame-phase-allocates-from-a-generation-arena.md

/// Block sizes, ascending. Multiples of [`BLOCK_ALIGN`], so every block is
/// [`BLOCK_ALIGN`]-aligned and any request with a smaller alignment can reuse
/// any block of its class.
pub(crate) const CLASS_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// Alignment every class block has. Requests needing more go to the bump and
/// never enter a list — both on allocation AND on free, by the same rule.
pub(crate) const BLOCK_ALIGN: usize = 16;

/// The class a request belongs to, or `None` for one the lists never serve:
/// larger than the biggest class, or aligned beyond [`BLOCK_ALIGN`].
///
/// ONE function for both paths. `alloc` rounds a request UP to its class and
/// `dealloc` finds the class from the same size, so a block always returns to
/// the class it was carved for — the invariant the intrusive links rest on.
pub(crate) fn class_for(size: usize, align: usize) -> Option<usize> {
    if align > BLOCK_ALIGN {
        return None;
    }
    CLASS_SIZES.iter().position(|&c| size <= c)
}

/// Why a free was not parked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The block is on a list already: a double free.
    Twice,
    /// The block is not one of the base heap's (outside it, or off its grid).
    Foreign,
}

/// Per-class LIFO stacks of freed blocks, linked through the blocks
/// themselves (`0` is the empty list; no block ever sits at address 0), and
/// the base heap's blocks on a list: one bit per [`BLOCK_ALIGN`] bytes, `WORDS`
/// words — the heap's size over 1 KiB.
pub(crate) struct FreeLists<const WORDS: usize> {
    heads: [usize; CLASS_SIZES.len()],
    /// Blocks currently on a list — what the probe reports as reclaimable.
    free_blocks: usize,
    /// The base heap's first byte (`init`); `0` until then — every free refused.
    base: usize,
    on_list: [u64; WORDS],
    /// Frees refused: a double free, or a block that is not the heap's.
    refused: usize,
}

impl<const WORDS: usize> FreeLists<WORDS> {
    pub(crate) const fn empty() -> Self {
        Self {
            heads: [0; CLASS_SIZES.len()],
            free_blocks: 0,
            base: 0,
            on_list: [0; WORDS],
            refused: 0,
        }
    }

    /// The base heap starts at `base`. Its blocks sit on the ABSOLUTE [`BLOCK_ALIGN`] grid the
    /// bump carves on, and the heap's first byte need not: a heap is a byte array, aligned to 1
    /// (QEMU, 6b-3: fourteen services refused every free as "not the heap's"). So the bitmap
    /// starts at the grid line at or below `base` — still `WORDS * 64` units, since the heap's
    /// last block starts below `base + HEAP_SIZE - BLOCK_ALIGN`.
    pub(crate) fn init(&mut self, base: usize) {
        self.base = base & !(BLOCK_ALIGN - 1);
    }

    /// The bit of the block at `addr`, when it is one of the base heap's.
    fn unit(&self, addr: usize) -> Option<usize> {
        let offset = addr.checked_sub(self.base).filter(|_| self.base != 0)?;
        let unit = offset / BLOCK_ALIGN;
        (offset % BLOCK_ALIGN == 0 && unit < WORDS * 64).then_some(unit)
    }

    fn mark(&mut self, unit: usize, on: bool) {
        let (word, bit) = (unit / 64, 1u64 << (unit % 64));
        if on {
            self.on_list[word] |= bit;
        } else {
            self.on_list[word] &= !bit;
        }
    }

    /// Takes a block of `class`, if one was freed earlier.
    ///
    /// # Safety
    /// Every address on the list was handed to [`FreeLists::push`] by this
    /// allocator's `dealloc` and is readable for `CLASS_SIZES[class]` bytes.
    pub(crate) unsafe fn pop(&mut self, class: usize) -> Option<usize> {
        let head = self.heads[class];
        if head == 0 {
            return None;
        }
        // SAFETY: `head` is a block this allocator owns; its first word is
        // the link `push` wrote.
        self.heads[class] = unsafe { core::ptr::read(head as *const usize) };
        self.free_blocks -= 1;
        if let Some(unit) = self.unit(head) {
            self.mark(unit, false);
        }
        Some(head)
    }

    /// Returns a block to `class`, or refuses it — already on a list (a double
    /// free), or not the base heap's — leaving every list as it was. With
    /// `poison`, the bytes AFTER the link are filled with `0xDE`, so a
    /// use-after-free of durable state fails as loudly as a use-after-reset in
    /// the arena.
    ///
    /// # Safety
    /// `addr` is a block this allocator carved for `class` and nothing
    /// references it any more — or one already on a list, which is refused
    /// without a byte written.
    pub(crate) unsafe fn push(
        &mut self,
        class: usize,
        addr: usize,
        poison: bool,
    ) -> Result<(), Refused> {
        let Some(unit) = self.unit(addr) else {
            self.refused += 1;
            return Err(Refused::Foreign);
        };
        if self.on_list[unit / 64] & (1 << (unit % 64)) != 0 {
            self.refused += 1;
            return Err(Refused::Twice);
        }
        // SAFETY: the caller guarantees exclusive ownership of the block.
        unsafe {
            if poison {
                let size = CLASS_SIZES[class];
                core::ptr::write_bytes(
                    (addr + core::mem::size_of::<usize>()) as *mut u8,
                    0xDE,
                    size - core::mem::size_of::<usize>(),
                );
            }
            core::ptr::write(addr as *mut usize, self.heads[class]);
        }
        self.mark(unit, true);
        self.heads[class] = addr;
        self.free_blocks += 1;
        Ok(())
    }

    /// Blocks currently parked on the lists.
    pub(crate) fn free_blocks(&self) -> usize {
        self.free_blocks
    }

    /// Frees refused since boot.
    pub(crate) fn refused(&self) -> usize {
        self.refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bitmap words for the 8 KiB scratch heap: one bit per 16 bytes.
    const WORDS: usize = 8192 / BLOCK_ALIGN / 64;

    /// A 16-aligned scratch region standing in for the bump's memory.
    fn region() -> (Vec<u8>, usize) {
        let buf = vec![0u8; 8192 + BLOCK_ALIGN];
        let base = (buf.as_ptr() as usize + BLOCK_ALIGN - 1) & !(BLOCK_ALIGN - 1);
        (buf, base)
    }

    /// Lists over the scratch heap at `base`.
    fn lists(base: usize) -> FreeLists<WORDS> {
        let mut lists = FreeLists::empty();
        lists.init(base);
        lists
    }

    /// TASK-0333 6b-3: a block already on a list is refused a second time — its link would close
    /// the list on itself and two later requests would get the same bytes. The refusal writes
    /// nothing, counts, and the block is taken once; freed again after that, it is parked.
    #[test]
    fn test_reject_a_double_free_of_a_class_block() {
        let (_buf, base) = region();
        let mut lists = lists(base);
        unsafe {
            lists.push(1, base + 32, false).expect("the first free");
            assert_eq!(lists.push(1, base + 32, false), Err(Refused::Twice));
            assert_eq!((lists.free_blocks(), lists.refused()), (1, 1));
            assert_eq!(lists.pop(1), Some(base + 32));
            assert_eq!(lists.pop(1), None, "the list did not close on itself");
            lists.push(1, base + 32, false).expect("freed again once taken");
        }
    }

    /// A block that is not the base heap's — past it, before it, off its 16-byte grid, or freed
    /// before the heap has a base — never enters a list.
    #[test]
    fn test_reject_freeing_a_block_that_is_not_the_heaps() {
        let (_buf, base) = region();
        let mut lists = lists(base);
        unsafe {
            for addr in [base + 8192, base - BLOCK_ALIGN, base + 8] {
                assert_eq!(lists.push(0, addr, false), Err(Refused::Foreign), "{addr:#x}");
            }
            let mut unset: FreeLists<WORDS> = FreeLists::empty();
            assert_eq!(unset.push(0, base, false), Err(Refused::Foreign), "no base yet");
        }
        assert_eq!((lists.free_blocks(), lists.refused()), (0, 3));
    }

    /// A heap is a byte array: it may start off the 16-byte grid its blocks are carved on (QEMU,
    /// 6b-3 — fourteen services refused every free). Its first and last blocks are parked, a
    /// second free is still refused, and an address off the grid is still not the heap's.
    #[test]
    fn a_heap_that_starts_off_the_grid_parks_its_blocks() {
        let (_buf, base) = region();
        let mut lists = lists(base + 8);
        unsafe {
            lists.push(0, base + 16, false).expect("the heap's first block on the grid");
            assert_eq!(lists.push(0, base + 16, false), Err(Refused::Twice));
            lists.push(0, base + 8192 - BLOCK_ALIGN, false).expect("the heap's last block");
            assert_eq!(lists.push(0, base + 24, false), Err(Refused::Foreign), "off the grid");
        }
    }

    #[test]
    fn requests_round_up_to_their_class_and_large_or_overaligned_ones_have_none() {
        assert_eq!(class_for(1, 1), Some(0));
        assert_eq!(class_for(16, 8), Some(0));
        assert_eq!(class_for(17, 8), Some(1));
        assert_eq!(class_for(2048, 16), Some(7));
        assert_eq!(class_for(2049, 8), None, "beyond the largest class is the bump's");
        assert_eq!(class_for(64, 32), None, "over-aligned requests never enter a list");
    }

    /// THE property: a freed block comes back for the next request of its
    /// class, at the SAME address. That is what turns "durable state churns"
    /// into "durable state has a bounded working set".
    #[test]
    fn a_freed_block_is_reused_by_its_class_at_the_same_address() {
        let (_buf, base) = region();
        let mut lists = lists(base);
        let class = class_for(40, 8).expect("class");
        unsafe {
            assert_eq!(lists.pop(class), None, "nothing freed yet");
            lists.push(class, base, false).expect("parked");
            assert_eq!(lists.free_blocks(), 1);
            assert_eq!(lists.pop(class), Some(base), "the same bytes come back");
            assert_eq!(lists.pop(class), None, "and only once");
        }
    }

    #[test]
    fn classes_never_share_blocks() {
        let (_buf, base) = region();
        let mut lists = lists(base);
        unsafe {
            lists.push(2, base, false).expect("parked");
            assert_eq!(lists.pop(3), None, "a bigger class must not steal a smaller block");
            assert_eq!(lists.pop(1), None, "nor a smaller class a bigger one");
            assert_eq!(lists.pop(2), Some(base));
        }
    }

    #[test]
    fn reuse_is_lifo_and_the_links_survive_poisoning() {
        let (_buf, base) = region();
        let mut lists = lists(base);
        let (a, b, c) = (base, base + 64, base + 128);
        unsafe {
            lists.push(2, a, true).expect("parked");
            lists.push(2, b, true).expect("parked");
            lists.push(2, c, true).expect("parked");
            assert_eq!(lists.free_blocks(), 3);
            assert_eq!(lists.pop(2), Some(c));
            assert_eq!(lists.pop(2), Some(b));
            assert_eq!(lists.pop(2), Some(a));
            assert_eq!(lists.pop(2), None);
        }
        // Poison filled everything after the link.
        let bytes = unsafe { core::slice::from_raw_parts((a + 8) as *const u8, 56) };
        assert!(bytes.iter().all(|&x| x == 0xDE), "freed durable bytes must be poison");
    }
}
