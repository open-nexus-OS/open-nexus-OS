// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memory accounting (RFC-0098 C4, TASK-0286 P5). The frame pool, the
//! page tables, the VMO table and each address space's region table already hold
//! the truth; `MmStats` is a read of them, never a second set of counters. This
//! module is the record's wire form (`mm_stats`, syscall 61), the check a caller's
//! buffer must pass, and the bounded logging of exhaustion events (RFC-0087 §1) —
//! pure, so all of it runs on host. The collection itself is `mm::usage` (riscv).
//! NOT target-gated; `mod mm` is riscv/none-only, hence the `#[path]` in `lib.rs`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable (versioned record: `MM_STATS_VERSION`)
//! TEST_COVERAGE: `tests` below; `KSELFTEST: mm frames (…)` and
//!   `metricsd: mm snapshot ok (…)` on every QEMU boot
//! INVARIANTS: the record never names another task; a buffer shorter than the
//!   record is refused, never half-written; an exhaustion is always counted and
//!   logged on the 1st, 2nd, 4th, 8th … occurrence.

/// The record's layout version (bumped on any field change).
pub const MM_STATS_VERSION: u32 = 1;
/// 64-bit fields after the 8-byte header.
pub const MM_STATS_FIELDS: usize = 14;
/// Bytes of the record: `version: u32`, `fields: u32`, then the fields.
pub const MM_STATS_BYTES: usize = 8 + MM_STATS_FIELDS * 8;

/// What `mm_stats` answers: the pool, the objects, and the caller's own space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MmStats {
    /// Memory banks the tree named.
    pub banks: u64,
    /// Frames handed to the pool.
    pub total: u64,
    /// Frames free now.
    pub free: u64,
    /// Frames the tree reserved.
    pub reserved: u64,
    /// Frames the kernel excluded (its image, the tree).
    pub excluded: u64,
    /// Blocks allocated since boot.
    pub allocs: u64,
    /// Blocks returned since boot.
    pub frees: u64,
    /// Requests the pool could not satisfy at all.
    pub exhausted: u64,
    /// Frames holding page tables.
    pub pt_frames: u64,
    /// Live VMOs.
    pub vmos: u64,
    /// Bytes they hold.
    pub vmo_bytes: u64,
    /// Bytes held by physically contiguous (DMA) VMOs.
    pub dma_bytes: u64,
    /// The caller's resident bytes: VMO and kernel-placed regions of its space.
    pub own_rss_bytes: u64,
    /// The part of those that is contiguous-DMA memory.
    pub own_dma_bytes: u64,
}

/// A buffer shorter than the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooSmall;

impl MmStats {
    /// The fields in wire order.
    pub fn fields(&self) -> [u64; MM_STATS_FIELDS] {
        [
            self.banks,
            self.total,
            self.free,
            self.reserved,
            self.excluded,
            self.allocs,
            self.frees,
            self.exhausted,
            self.pt_frames,
            self.vmos,
            self.vmo_bytes,
            self.dma_bytes,
            self.own_rss_bytes,
            self.own_dma_bytes,
        ]
    }

    /// Write the record into `out`; returns the bytes written.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, TooSmall> {
        check_out_len(out.len())?;
        out[0..4].copy_from_slice(&MM_STATS_VERSION.to_le_bytes());
        out[4..8].copy_from_slice(&(MM_STATS_FIELDS as u32).to_le_bytes());
        for (i, value) in self.fields().iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        Ok(MM_STATS_BYTES)
    }
}

/// A caller's buffer must hold the whole record.
pub fn check_out_len(len: usize) -> Result<(), TooSmall> {
    if len < MM_STATS_BYTES {
        Err(TooSmall)
    } else {
        Ok(())
    }
}

/// Log the `count`-th exhaustion? The 1st, 2nd, 4th, 8th …: a storm stays
/// bounded on the console, the counter holds every one.
pub fn log_exhaustion(count: u64) -> bool {
    count.is_power_of_two()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_is_a_versioned_header_and_the_fields_in_order() {
        let stats = MmStats {
            banks: 2,
            total: 77280,
            free: 1,
            own_dma_bytes: 0x1234,
            ..Default::default()
        };
        let mut out = [0xffu8; MM_STATS_BYTES + 3];
        assert_eq!(stats.encode(&mut out), Ok(MM_STATS_BYTES));
        assert_eq!(u32::from_le_bytes(out[0..4].try_into().unwrap()), MM_STATS_VERSION);
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()), MM_STATS_FIELDS as u32);
        let field = |i: usize| u64::from_le_bytes(out[8 + i * 8..16 + i * 8].try_into().unwrap());
        assert_eq!((field(0), field(1), field(2)), (2, 77280, 1));
        assert_eq!(field(MM_STATS_FIELDS - 1), 0x1234);
        assert_eq!(out[MM_STATS_BYTES..], [0xff; 3], "nothing past the record");
    }

    #[test]
    fn test_reject_mm_stats_into_a_short_buffer() {
        let mut out = [0x5au8; MM_STATS_BYTES - 1];
        assert_eq!(MmStats::default().encode(&mut out), Err(TooSmall));
        assert!(out.iter().all(|b| *b == 0x5a), "refused, never half-written");
        assert_eq!(check_out_len(0), Err(TooSmall));
        assert_eq!(check_out_len(MM_STATS_BYTES), Ok(()));
    }

    #[test]
    fn exhaustion_is_logged_on_powers_of_two_and_never_on_zero() {
        let logged: Vec<u64> = (0..=17).filter(|c| log_exhaustion(*c)).collect();
        assert_eq!(logged, [1, 2, 4, 8, 16]);
    }
}
