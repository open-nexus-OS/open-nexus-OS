// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the rules of memory committed on first touch (RFC-0100 §2, ADR-0077) — what a page
//! fault or a kernel access to user memory may do, and the frame table of a lazy object — as pure
//! logic, so the decisions that would let a process map what it has no right to are proven on the
//! host. The kernel's fault path and its checked user range (`mm/populate.rs`) feed these
//! functions facts from the page table and the region record and execute their verdicts.
//!
//! NOT target-gated (pure index logic, like `va_space` and `vmo_lifetime`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6b-1)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — the verdict table (every right, a valid PTE, an eager
//!   region's hole, no region) and the frame table (a page committed once, counts, bounds)
//! INVARIANTS: a page is committed only inside a region that grants the access; a lazy page is
//!   never executable; a page has at most one frame; a frame number fits the table's word.

use alloc::vec::Vec;

/// One page.
pub const PAGE: usize = 4096;

/// What a fault or a kernel range asks of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Execute,
}

/// The rights a region grants or a PTE carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Rights {
    /// Whether these rights allow `access`.
    #[must_use]
    pub fn allow(self, access: Access) -> bool {
        match access {
            Access::Read => self.read,
            Access::Write => self.write,
            Access::Execute => self.execute,
        }
    }
}

/// The region an address lies in, as a fault sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionView {
    pub rights: Rights,
    /// The region maps a lazy object: its pages are committed on touch.
    pub lazy: bool,
}

/// Why a fault is not ours to fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// No region covers the address.
    NoRegion,
    /// The region does not grant the access.
    NoRight(Access),
    /// An eager region has every page mapped: a fault inside it is not a missing page.
    EagerRegion,
    /// A lazy page is data, never code.
    ExecuteLazy,
}

/// What to do with a fault (or a page of a kernel range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Commit the page if it is not committed yet, map it, fence, resume.
    Populate,
    /// The PTE already grants the access: fence and resume.
    Spurious,
    /// Not a missing page: the fault kills, the range is `EFAULT`.
    Violation(Violation),
}

/// The verdict for `access` at an address inside `region` (if any) whose PTE carries `pte`
/// (`None` when no valid leaf maps it).
#[must_use]
pub fn classify(region: Option<RegionView>, pte: Option<Rights>, access: Access) -> Verdict {
    if pte.is_some_and(|rights| rights.allow(access)) {
        return Verdict::Spurious;
    }
    let Some(region) = region else { return Verdict::Violation(Violation::NoRegion) };
    if !region.rights.allow(access) {
        return Verdict::Violation(Violation::NoRight(access));
    }
    if !region.lazy {
        return Verdict::Violation(Violation::EagerRegion);
    }
    if access == Access::Execute {
        return Verdict::Violation(Violation::ExecuteLazy);
    }
    Verdict::Populate
}

/// The object page a fault at `va` names, in a region that maps the object from byte `offset`
/// at `region_va`.
#[must_use]
pub fn page_index(region_va: usize, offset: usize, va: usize) -> usize {
    (offset + (va & !(PAGE - 1)) - region_va) / PAGE
}

/// A page's frame number in the table: `NONE` = not committed.
const NONE: u32 = u32::MAX;

/// Why a commit was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    /// The page already has this frame: the caller frees the one it brought and uses it.
    Committed(u64),
    /// The page lies past the object's end, or the frame does not fit the table's word.
    OutOfRange,
}

/// The frames of a lazy object: one entry per 4 KiB page, the frame number or none.
pub struct LazyPages {
    frames: Vec<u32>,
    committed: usize,
}

impl LazyPages {
    /// `pages` pages, none committed.
    #[must_use]
    pub fn new(pages: usize) -> Self {
        Self { frames: alloc::vec![NONE; pages], committed: 0 }
    }

    /// Pages the object spans.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.frames.len()
    }

    /// Pages committed.
    #[must_use]
    pub fn committed(&self) -> usize {
        self.committed
    }

    /// The physical address of page `index`'s frame, when committed.
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<u64> {
        let pfn = *self.frames.get(index)?;
        (pfn != NONE).then(|| u64::from(pfn) * PAGE as u64)
    }

    /// Record `pa` as page `index`'s frame. A page is committed once: a second commit is refused
    /// with the frame the page already has.
    pub fn commit(&mut self, index: usize, pa: u64) -> Result<(), CommitError> {
        let pfn = u32::try_from(pa / PAGE as u64).map_err(|_| CommitError::OutOfRange)?;
        if pfn == NONE || pa % PAGE as u64 != 0 {
            return Err(CommitError::OutOfRange);
        }
        let entry = self.frames.get_mut(index).ok_or(CommitError::OutOfRange)?;
        if *entry != NONE {
            return Err(CommitError::Committed(u64::from(*entry) * PAGE as u64));
        }
        *entry = pfn;
        self.committed += 1;
        Ok(())
    }

    /// Pages committed in `[first, first + count)`.
    #[must_use]
    pub fn committed_in(&self, first: usize, count: usize) -> usize {
        let end = first.saturating_add(count).min(self.frames.len());
        self.frames.get(first..end).map_or(0, |s| s.iter().filter(|&&p| p != NONE).count())
    }

    /// Every committed frame, in page order (the object's death frees them).
    pub fn frames(&self) -> impl Iterator<Item = u64> + '_ {
        self.frames.iter().filter(|&&p| p != NONE).map(|&p| u64::from(p) * PAGE as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RW: Rights = Rights { read: true, write: true, execute: false };
    const RO: Rights = Rights { read: true, write: false, execute: false };
    const RX: Rights = Rights { read: true, write: false, execute: true };

    fn lazy(rights: Rights) -> Option<RegionView> {
        Some(RegionView { rights, lazy: true })
    }

    #[test]
    fn a_touch_inside_a_lazy_region_with_its_right_populates() {
        assert_eq!(classify(lazy(RW), None, Access::Read), Verdict::Populate);
        assert_eq!(classify(lazy(RW), None, Access::Write), Verdict::Populate);
        assert_eq!(classify(lazy(RO), None, Access::Read), Verdict::Populate);
    }

    /// A valid PTE that already grants the access: another hart committed the page, or this
    /// hart cached the miss — fence and resume, never a second frame.
    #[test]
    fn a_valid_pte_with_the_right_is_spurious() {
        assert_eq!(classify(lazy(RW), Some(RW), Access::Write), Verdict::Spurious);
        assert_eq!(classify(lazy(RO), Some(RO), Access::Read), Verdict::Spurious);
        // Even outside any region a PTE that grants the access has nothing to fix.
        assert_eq!(classify(None, Some(RO), Access::Read), Verdict::Spurious);
    }

    #[test]
    fn test_reject_populate_outside_a_region() {
        assert_eq!(classify(None, None, Access::Read), Verdict::Violation(Violation::NoRegion));
        assert_eq!(classify(None, None, Access::Write), Verdict::Violation(Violation::NoRegion));
    }

    #[test]
    fn test_reject_write_fault_on_a_read_only_region() {
        assert_eq!(
            classify(lazy(RO), None, Access::Write),
            Verdict::Violation(Violation::NoRight(Access::Write))
        );
        // A present read-only PTE does not let a store through either.
        assert_eq!(
            classify(lazy(RO), Some(RO), Access::Write),
            Verdict::Violation(Violation::NoRight(Access::Write))
        );
    }

    #[test]
    fn test_reject_exec_fault() {
        assert_eq!(
            classify(lazy(RW), None, Access::Execute),
            Verdict::Violation(Violation::NoRight(Access::Execute))
        );
        // Even a region that would grant execute never runs a lazy page.
        assert_eq!(
            classify(lazy(RX), None, Access::Execute),
            Verdict::Violation(Violation::ExecuteLazy)
        );
    }

    /// An eager region is fully mapped from the start: a fault inside it is a real fault.
    #[test]
    fn test_reject_populating_an_eager_region() {
        let eager = Some(RegionView { rights: RW, lazy: false });
        assert_eq!(
            classify(eager, None, Access::Write),
            Verdict::Violation(Violation::EagerRegion)
        );
    }

    #[test]
    fn a_fault_names_the_object_page_of_its_region() {
        let (region_va, offset) = (0x5000_0000, 3 * PAGE);
        assert_eq!(page_index(region_va, offset, region_va), 3);
        assert_eq!(page_index(region_va, offset, region_va + PAGE + 17), 4);
        assert_eq!(page_index(region_va, 0, region_va + 10 * PAGE + PAGE - 1), 10);
    }

    #[test]
    fn a_page_is_committed_once_and_keeps_its_frame() {
        let mut pages = LazyPages::new(8);
        assert_eq!(pages.committed(), 0);
        assert_eq!(pages.frame(2), None);
        pages.commit(2, 0x8000_3000).unwrap();
        assert_eq!(pages.frame(2), Some(0x8000_3000));
        assert_eq!(pages.commit(2, 0x8000_9000), Err(CommitError::Committed(0x8000_3000)));
        assert_eq!(pages.committed(), 1, "a refused commit counts nothing");
        pages.commit(7, 0x1_2000_0000).unwrap();
        assert_eq!(pages.committed_in(0, 8), 2);
        assert_eq!(pages.committed_in(3, 4), 0);
        assert_eq!(pages.committed_in(6, 100), 1, "a count past the end is clipped");
        assert_eq!(pages.frames().collect::<Vec<_>>(), [0x8000_3000, 0x1_2000_0000]);
    }

    #[test]
    fn test_reject_a_commit_past_the_end_or_of_a_frame_the_table_cannot_hold() {
        let mut pages = LazyPages::new(4);
        assert_eq!(pages.commit(4, 0x8000_0000), Err(CommitError::OutOfRange));
        assert_eq!(pages.commit(0, 0x8000_0010), Err(CommitError::OutOfRange), "unaligned");
        assert_eq!(pages.commit(0, 1 << 46), Err(CommitError::OutOfRange), "beyond 16 TiB");
        assert_eq!(pages.committed(), 0);
    }
}
