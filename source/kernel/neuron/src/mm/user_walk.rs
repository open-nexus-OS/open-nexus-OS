// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the walk a user copy translates with (TASK-0330 P2, ADR-0102) — Sv39, from a
//! root's physical address, reading each entry once and taking no lock. It answers where the
//! bytes of a user range are in physical memory, piece by piece, and refuses everything a user
//! access through the MMU would fault on: an address outside the user half, an entry that is
//! not valid, a leaf without the user bit, without the right asked, without its accessed (and
//! for a write: dirty) bit, or malformed. A refusal is a `Miss`: the caller takes the path
//! that holds the address space (a lazy page is filled there, a violation named there).
//! Pure: the reader of an entry is the caller's (the direct map on the target, a map of tables
//! in the tests).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::*`: 4 KiB and 2 MiB leaves, a range across leaves, and
//!   `test_reject_*` for the kernel half, a missing page, a page without the user bit, a
//!   read-only page written, a wrap, a misaligned superpage, a table at the last level)
//! INVARIANTS: an entry is read once per piece; what is returned was readable (writable) by a
//!   user access at that reading; nothing here writes.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

const VALID: usize = 1 << 0;
const READ: usize = 1 << 1;
const WRITE: usize = 1 << 2;
const EXECUTE: usize = 1 << 3;
const USER: usize = 1 << 4;
const ACCESSED: usize = 1 << 6;
const DIRTY: usize = 1 << 7;
const LEAF: usize = READ | WRITE | EXECUTE;
/// The physical page number of an entry: bits 10..54; the bits above are reserved.
const PPN_BITS: u32 = 44;
const RESERVED: usize = !((1usize << (10 + PPN_BITS)) - 1);
/// Entries of a table page.
const ENTRIES: usize = 512;
/// One past the last address of the user half (Sv39: bit 38 clear).
pub const USER_END: usize = 1 << 38;

/// What the copy does with the user range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The kernel reads the user's bytes.
    Read,
    /// The kernel writes the user's bytes.
    Write,
}

/// The walk cannot vouch for the range: the caller takes the path that holds the space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Miss;

/// A stretch of a user range inside one leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// The physical address of the stretch's first byte.
    pub pa: usize,
    /// Its offset in the range.
    pub offset: usize,
    /// Its length.
    pub len: usize,
}

/// Where `va` is, and how many bytes follow it inside its leaf.
fn leaf(
    root_pa: usize,
    va: usize,
    access: Access,
    read: &impl Fn(usize, usize) -> usize,
) -> Result<(usize, usize), Miss> {
    if va >= USER_END {
        return Err(Miss);
    }
    let mut table = root_pa;
    for level in (0..3usize).rev() {
        let entry = read(table, (va >> (12 + 9 * level)) & (ENTRIES - 1));
        if entry & VALID == 0 || entry & RESERVED != 0 {
            return Err(Miss);
        }
        let pa = ((entry >> 10) & ((1 << PPN_BITS) - 1)) << 12;
        if entry & LEAF == 0 {
            table = pa;
            continue;
        }
        let needed = match access {
            Access::Read => USER | READ | ACCESSED,
            Access::Write => USER | READ | WRITE | ACCESSED | DIRTY,
        };
        let size = 1usize << (12 + 9 * level);
        if entry & needed != needed || pa & (size - 1) != 0 {
            return Err(Miss);
        }
        let within = va & (size - 1);
        return Ok((pa | within, size - within));
    }
    // A table where the last level's leaf belongs.
    Err(Miss)
}

/// Hands `piece` every stretch of the user range `va..va + len` of the space rooted at
/// `root_pa`, in order, each translated right before it is handed out. Stops at the first
/// stretch the walk or `piece` refuses; stretches handed out before stay as they are.
pub fn pieces(
    root_pa: usize,
    va: usize,
    len: usize,
    access: Access,
    read: impl Fn(usize, usize) -> usize,
    mut piece: impl FnMut(Piece) -> Result<(), Miss>,
) -> Result<(), Miss> {
    let end = va.checked_add(len).ok_or(Miss)?;
    if end > USER_END {
        return Err(Miss);
    }
    let mut offset = 0;
    while offset < len {
        let (pa, left) = leaf(root_pa, va + offset, access, &read)?;
        let n = left.min(len - offset);
        piece(Piece { pa, offset, len: n })?;
        offset += n;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;

    const PAGE: usize = 4096;
    const RW: usize = VALID | READ | WRITE | USER | ACCESSED | DIRTY;
    const RO: usize = VALID | READ | USER | ACCESSED;

    /// Physical memory holding page tables: (table pa, index) → entry.
    #[derive(Default)]
    struct Tables {
        entries: BTreeMap<(usize, usize), usize>,
        next: usize,
    }

    impl Tables {
        const ROOT: usize = 0x8000_0000;

        fn new() -> Self {
            Self { entries: BTreeMap::new(), next: Self::ROOT + PAGE }
        }

        fn table(&mut self, at: usize, index: usize) -> usize {
            if let Some(entry) = self.entries.get(&(at, index)) {
                return (entry >> 10) << 12;
            }
            let pa = self.next;
            self.next += PAGE;
            self.entries.insert((at, index), (pa >> 12) << 10 | VALID);
            pa
        }

        fn map(&mut self, va: usize, pa: usize, flags: usize) {
            let l1 = self.table(Self::ROOT, (va >> 30) & 511);
            let l0 = self.table(l1, (va >> 21) & 511);
            self.entries.insert((l0, (va >> 12) & 511), (pa >> 12) << 10 | flags);
        }

        fn map_2m(&mut self, va: usize, pa: usize, flags: usize) {
            let l1 = self.table(Self::ROOT, (va >> 30) & 511);
            self.entries.insert((l1, (va >> 21) & 511), (pa >> 12) << 10 | flags);
        }

        fn walk(&self, va: usize, len: usize, access: Access) -> Result<Vec<Piece>, Miss> {
            let mut out = Vec::new();
            pieces(
                Self::ROOT,
                va,
                len,
                access,
                |table, index| self.entries.get(&(table, index)).copied().unwrap_or(0),
                |piece| {
                    out.push(piece);
                    Ok(())
                },
            )?;
            Ok(out)
        }
    }

    #[test]
    fn a_range_inside_a_page_is_one_piece() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_0000, RW);
        assert_eq!(
            t.walk(0x1000_0010, 100, Access::Write),
            Ok(alloc::vec![Piece { pa: 0x9000_0010, offset: 0, len: 100 }])
        );
        assert_eq!(t.walk(0x1000_0010, 0, Access::Read), Ok(Vec::new()));
    }

    #[test]
    fn a_range_across_pages_follows_each_page_to_its_own_frame() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_5000, RW);
        t.map(0x1000_1000, 0x9000_2000, RW);
        assert_eq!(
            t.walk(0x1000_0ff0, 0x30, Access::Read),
            Ok(alloc::vec![
                Piece { pa: 0x9000_5ff0, offset: 0, len: 0x10 },
                Piece { pa: 0x9000_2000, offset: 0x10, len: 0x20 },
            ])
        );
    }

    #[test]
    fn a_superpage_is_one_piece_up_to_its_end() {
        let mut t = Tables::new();
        t.map_2m(0x4000_0000, 0xa000_0000, RW);
        t.map(0x4020_0000, 0x9000_0000, RW);
        let got = t.walk(0x4000_0000 + 0x1f_f000, 0x2000, Access::Write).expect("mapped");
        assert_eq!(
            got,
            [
                Piece { pa: 0xa000_0000 + 0x1f_f000, offset: 0, len: 0x1000 },
                Piece { pa: 0x9000_0000, offset: 0x1000, len: 0x1000 },
            ]
        );
    }

    #[test]
    fn test_reject_the_kernel_half_and_a_wrap() {
        let mut t = Tables::new();
        t.map(USER_END - PAGE, 0x9000_0000, RW);
        assert!(t.walk(USER_END - 8, 8, Access::Read).is_ok());
        assert_eq!(t.walk(USER_END - 8, 9, Access::Read), Err(Miss));
        assert_eq!(t.walk(0xffff_ffc0_0000_0000, 8, Access::Read), Err(Miss));
        assert_eq!(t.walk(usize::MAX - 3, 8, Access::Read), Err(Miss));
    }

    #[test]
    fn test_reject_a_missing_page_also_in_the_middle() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_0000, RW);
        assert_eq!(t.walk(0x2000_0000, 8, Access::Read), Err(Miss));
        // The first page is there, the second is not: the walk stops between them.
        let mut seen = 0;
        let r = pieces(
            Tables::ROOT,
            0x1000_0ff8,
            16,
            Access::Read,
            |table, index| t.entries.get(&(table, index)).copied().unwrap_or(0),
            |_| {
                seen += 1;
                Ok(())
            },
        );
        assert_eq!((r, seen), (Err(Miss), 1));
    }

    #[test]
    fn test_reject_a_page_without_the_user_bit() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_0000, RW & !USER);
        assert_eq!(t.walk(0x1000_0000, 8, Access::Read), Err(Miss));
        assert_eq!(t.walk(0x1000_0000, 8, Access::Write), Err(Miss));
    }

    #[test]
    fn test_reject_a_write_to_a_read_only_page() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_0000, RO);
        assert!(t.walk(0x1000_0000, 8, Access::Read).is_ok());
        assert_eq!(t.walk(0x1000_0000, 8, Access::Write), Err(Miss));
        // Writable but not yet dirty: the MMU would fault or update the entry — not ours.
        t.map(0x1000_1000, 0x9000_1000, RW & !DIRTY);
        assert_eq!(t.walk(0x1000_1000, 8, Access::Write), Err(Miss));
        // An execute-only page is not readable.
        t.map(0x1000_2000, 0x9000_2000, VALID | EXECUTE | USER | ACCESSED);
        assert_eq!(t.walk(0x1000_2000, 8, Access::Read), Err(Miss));
    }

    #[test]
    fn test_reject_a_malformed_entry() {
        let mut t = Tables::new();
        // A superpage whose frame is not on its own grid.
        t.map_2m(0x4000_0000, 0xa000_1000, RW);
        assert_eq!(t.walk(0x4000_0000, 8, Access::Read), Err(Miss));
        // A table where the last level's leaf belongs.
        t.map(0x1000_0000, 0x9000_0000, VALID);
        assert_eq!(t.walk(0x1000_0000, 8, Access::Read), Err(Miss));
        // Reserved bits set.
        t.map(0x1000_1000, 0x9000_0000, RW | 1 << 60);
        assert_eq!(t.walk(0x1000_1000, 8, Access::Read), Err(Miss));
    }

    #[test]
    fn a_refusal_of_the_piece_ends_the_walk() {
        let mut t = Tables::new();
        t.map(0x1000_0000, 0x9000_0000, RW);
        t.map(0x1000_1000, 0x9000_1000, RW);
        let mut seen = 0;
        let r = pieces(
            Tables::ROOT,
            0x1000_0000,
            2 * PAGE,
            Access::Read,
            |table, index| t.entries.get(&(table, index)).copied().unwrap_or(0),
            |_| {
                seen += 1;
                Err(Miss)
            },
        );
        assert_eq!((r, seen), (Err(Miss), 1));
    }
}
