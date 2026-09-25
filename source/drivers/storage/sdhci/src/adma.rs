// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADMA2 descriptors with 32-bit addresses (SDHCI 3.00 §1.13): eight bytes each —
//! attribute [15:0], length [31:16] (0 means 65 536), address [63:32], little endian — one
//! per piece of a run, built from the `DmaRun`s the kernel answered for THIS device (its bus
//! addresses, RFC-0098 C4). The last transfer descriptor carries END: no trailing NOP (some
//! controllers — the K1 among them — mishandle an END on a NOP; every controller accepts it
//! on the last transfer). Every piece is 4-byte aligned in address and length (the K1
//! counts ADMA lengths in words) and lies below 4 GiB (32-bit descriptors; 64-bit DMA is
//! broken on the K1, and its storage bus reaches only 2 GiB anyway). The descriptors cover
//! exactly the transfer: a controller whose block count reaches zero before END, or END
//! before the count, reports a length mismatch.
//! OWNERS: @runtime @drivers

use nexus_abi::DmaRun;

/// Bytes of one descriptor.
pub const DESC_BYTES: usize = 8;
/// The most bytes one descriptor moves.
pub const MAX_PIECE: u64 = 65_536;
/// Attribute: the descriptor is valid.
pub const ATTR_VALID: u16 = 1 << 0;
/// Attribute: the last descriptor.
pub const ATTR_END: u16 = 1 << 1;
/// Attribute: transfer data.
pub const ATTR_TRAN: u16 = 2 << 4;

/// Why no descriptor table was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmaError {
    /// Nothing to transfer.
    Empty,
    /// The runs cover fewer bytes than the transfer.
    Short,
    /// A piece is not 4-byte aligned in address or length.
    Unaligned,
    /// A byte lies at or above 4 GiB.
    Above4G,
    /// More descriptors than the table holds.
    TableFull,
    /// The table itself is not one run (the controller is given one address).
    TableSplit,
}

/// Write the descriptors for the first `len` bytes of `runs` into `table`; returns how many
/// were written. Nothing past them is touched; on an error the table's content is
/// unspecified and must not be handed to the controller.
pub fn build(runs: &[DmaRun], len: usize, table: &mut [u8]) -> Result<usize, AdmaError> {
    if len == 0 {
        return Err(AdmaError::Empty);
    }
    let capacity = table.len() / DESC_BYTES;
    let mut left = len as u64;
    let mut n = 0usize;
    for run in runs {
        if left == 0 {
            break;
        }
        let mut addr = run.bus;
        let mut take = run.len.min(left);
        while take > 0 {
            let piece = take.min(MAX_PIECE);
            if addr % 4 != 0 || piece % 4 != 0 {
                return Err(AdmaError::Unaligned);
            }
            if addr.checked_add(piece).is_none_or(|end| end > 1 << 32) {
                return Err(AdmaError::Above4G);
            }
            if n == capacity {
                return Err(AdmaError::TableFull);
            }
            let length = if piece == MAX_PIECE { 0 } else { piece as u16 };
            let at = n * DESC_BYTES;
            table[at..at + 2].copy_from_slice(&(ATTR_VALID | ATTR_TRAN).to_le_bytes());
            table[at + 2..at + 4].copy_from_slice(&length.to_le_bytes());
            table[at + 4..at + 8].copy_from_slice(&(addr as u32).to_le_bytes());
            n += 1;
            addr += piece;
            take -= piece;
            left -= piece;
        }
    }
    if left > 0 {
        return Err(AdmaError::Short);
    }
    let last = (n - 1) * DESC_BYTES;
    table[last] |= ATTR_END as u8;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bus: u64, len: u64) -> DmaRun {
        DmaRun { bus, len }
    }

    fn desc(table: &[u8], i: usize) -> (u16, u16, u32) {
        let at = i * DESC_BYTES;
        let attr = u16::from_le_bytes([table[at], table[at + 1]]);
        let len = u16::from_le_bytes([table[at + 2], table[at + 3]]);
        let addr = u32::from_le_bytes([table[at + 4], table[at + 5], table[at + 6], table[at + 7]]);
        (attr, len, addr)
    }

    #[test]
    fn one_run_is_one_descriptor_with_end_and_the_exact_bytes() {
        let mut table = [0xAAu8; 32];
        assert_eq!(build(&[run(0x4010_0000, 4096)], 4096, &mut table), Ok(1));
        assert_eq!(&table[..8], &[0x23, 0x00, 0x00, 0x10, 0x00, 0x00, 0x10, 0x40]);
        assert_eq!(table[8], 0xAA, "nothing past the descriptors is touched");
    }

    #[test]
    fn a_run_longer_than_64_kib_splits_and_a_full_piece_encodes_as_zero() {
        let mut table = [0u8; 64];
        assert_eq!(build(&[run(0x2000_0000, 0x2_1000)], 0x2_1000, &mut table), Ok(3));
        assert_eq!(desc(&table, 0), (ATTR_VALID | ATTR_TRAN, 0, 0x2000_0000));
        assert_eq!(desc(&table, 1), (ATTR_VALID | ATTR_TRAN, 0, 0x2001_0000));
        assert_eq!(desc(&table, 2), (ATTR_VALID | ATTR_TRAN | ATTR_END, 0x1000, 0x2002_0000));
    }

    #[test]
    fn scattered_runs_are_followed_in_order_and_only_as_far_as_the_transfer() {
        let runs = [run(0x7000_0000, 4096), run(0x3000_0000, 8192), run(0x5000_0000, 4096)];
        let mut table = [0u8; 64];
        assert_eq!(build(&runs, 6144, &mut table), Ok(2));
        assert_eq!(desc(&table, 0), (ATTR_VALID | ATTR_TRAN, 4096, 0x7000_0000));
        assert_eq!(desc(&table, 1), (ATTR_VALID | ATTR_TRAN | ATTR_END, 2048, 0x3000_0000));
    }

    #[test]
    fn test_reject_descriptors_the_controller_cannot_follow() {
        let mut table = [0u8; 16];
        assert_eq!(build(&[run(0x1000, 4096)], 0, &mut table), Err(AdmaError::Empty));
        assert_eq!(build(&[run(0x1000, 4096)], 8192, &mut table), Err(AdmaError::Short));
        assert_eq!(build(&[run(0x1002, 512)], 512, &mut table), Err(AdmaError::Unaligned));
        assert_eq!(build(&[run(0x1000, 4096)], 510, &mut table), Err(AdmaError::Unaligned));
        assert_eq!(build(&[run(0xFFFF_F000, 8192)], 8192, &mut table), Err(AdmaError::Above4G));
        assert_eq!(build(&[run(0x1_0000_0000, 512)], 512, &mut table), Err(AdmaError::Above4G));
        let runs = [run(0x1000, 512), run(0x3000, 512), run(0x5000, 512)];
        assert_eq!(build(&runs, 1536, &mut table), Err(AdmaError::TableFull));
    }

    #[test]
    fn the_last_byte_below_4_gib_is_reachable() {
        let mut table = [0u8; 8];
        assert_eq!(build(&[run(0xFFFF_F000, 4096)], 4096, &mut table), Ok(1));
    }
}
