// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one door a physical address leaves the kernel by (RFC-0098 C4,
//! TASK-0286 P4a, device-scoped by TASK-0246 P1). `vmo_runs` (syscall 60) hands a
//! driver the runs behind a byte range of a VMO so it can program a bus master — a
//! virtio queue's one base, a scatter-gather resource backing — in THAT device's bus
//! addresses: the caller names the device, holds its capability, and every byte must
//! lie in the device's reach (`crate::dma_reach`). The authority rule and the
//! clip-then-translate live here, pure, so the reject matrix runs on host; the syscall
//! (`syscall/api/vmo_runs.rs`) resolves the capabilities and copies the answer out.
//! `cap_query` names no VMO base.
//! NOT target-gated (pure arithmetic); `mod mm` is riscv/none-only, hence the
//! `#[path]` in `lib.rs`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (authority, clipping and reach reject matrix); the
//!   wiring by `KSELFTEST: vmo runs ok (…)` and every DMA driver on every QEMU boot
//! INVARIANTS: no run is answered for a slot that is not a device the caller holds;
//!   a read-only alias never yields a run; the runs lie inside the object, cover
//!   exactly the asked range and lie in the device's reach; the answer is complete
//!   or refused, never truncated.

use crate::dma_reach::{DmaReach, ReachError};

/// Runs one call may return (16 bytes each: a 4 KiB answer at most).
pub const MAX_RUNS: usize = 256;

/// Bytes of one run in the user buffer: `bus: u64` then `len: u64`, little endian.
pub const RUN_BYTES: usize = 16;

/// What the capability in the VMO slot is, as far as this door cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holding {
    /// A writable VMO held with MAP.
    Vmo,
    /// A read-only alias (`vmo_share_ro`).
    ReadOnlyAlias,
    /// Anything else.
    Other,
}

/// Why the door stayed shut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunsError {
    /// Not a VMO.
    NotVmo,
    /// A read-only alias: a device could write through the address.
    ReadOnly,
    /// The device slot holds no device capability of the caller's.
    NoDevice,
    /// Empty or overflowing range, past the object's end, the object shorter
    /// than its capability, or an answer buffer of 0 or more than `MAX_RUNS`.
    Range,
    /// The range needs more runs than the answer buffer holds.
    TooMany,
    /// Some byte of the range lies outside the device's reach.
    OutOfReach,
}

/// The authority rule: a writable VMO, and a device the caller holds.
pub fn authorize(device: bool, holding: Holding) -> Result<(), RunsError> {
    match holding {
        Holding::Other => Err(RunsError::NotVmo),
        Holding::ReadOnlyAlias => Err(RunsError::ReadOnly),
        Holding::Vmo if !device => Err(RunsError::NoDevice),
        Holding::Vmo => Ok(()),
    }
}

/// The device-visible runs of `offset..offset + len` of an object of `object_len`
/// bytes whose physical runs are `runs` (object order): each piece is clipped to
/// the range, translated through `reach` and merged with the previous one when
/// they are contiguous in bus space. Returns how many runs were written.
pub fn device_runs<I>(
    runs: I,
    object_len: usize,
    offset: usize,
    len: usize,
    reach: &DmaReach,
    out: &mut [(u64, u64)],
) -> Result<usize, RunsError>
where
    I: IntoIterator<Item = (usize, usize)>,
{
    if out.is_empty() || out.len() > MAX_RUNS || len == 0 {
        return Err(RunsError::Range);
    }
    let end = offset.checked_add(len).ok_or(RunsError::Range)?;
    if end > object_len {
        return Err(RunsError::Range);
    }
    let mut written = 0usize;
    let mut covered = 0usize;
    let mut at = 0usize;
    for (pa, run_len) in runs {
        let run_end = at.checked_add(run_len).ok_or(RunsError::Range)?;
        let (lo, hi) = (offset.max(at), end.min(run_end));
        if lo < hi {
            let piece_pa = pa.checked_add(lo - at).ok_or(RunsError::Range)?;
            reach.to_bus(piece_pa as u64, (hi - lo) as u64, out, &mut written).map_err(
                |e| match e {
                    ReachError::OutOfReach => RunsError::OutOfReach,
                    ReachError::TooMany => RunsError::TooMany,
                },
            )?;
            covered += hi - lo;
        }
        at = run_end;
        if at >= end {
            break;
        }
    }
    // An object shorter than its capability claims is a kernel bug, and the
    // answer would be short: refuse rather than hand out a partial list.
    if covered != len {
        return Err(RunsError::Range);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dma_reach::DmaWindow;

    const PAGE: usize = 4096;
    const MIB: usize = 1 << 20;
    const GIB: u64 = 1 << 30;
    const ALL: DmaReach = DmaReach::ALL;

    fn storage() -> DmaReach {
        DmaReach::new(&[DmaWindow { bus: 0, cpu: 0, size: 2 * GIB }]).unwrap()
    }

    #[test]
    fn a_driver_with_its_device_and_a_writable_vmo_is_let_through() {
        assert_eq!(authorize(true, Holding::Vmo), Ok(()));
    }

    #[test]
    fn test_reject_vmo_runs_without_the_device_capability() {
        assert_eq!(authorize(false, Holding::Vmo), Err(RunsError::NoDevice));
    }

    #[test]
    fn test_reject_vmo_runs_of_a_read_only_alias() {
        assert_eq!(authorize(true, Holding::ReadOnlyAlias), Err(RunsError::ReadOnly));
        assert_eq!(authorize(false, Holding::ReadOnlyAlias), Err(RunsError::ReadOnly));
    }

    #[test]
    fn test_reject_vmo_runs_of_something_else() {
        assert_eq!(authorize(true, Holding::Other), Err(RunsError::NotVmo));
    }

    #[test]
    fn one_block_is_one_run_and_a_sub_range_is_offset_into_it() {
        let runs = [(0x8040_0000usize, 4 * PAGE)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(device_runs(runs, 4 * PAGE, 0, 4 * PAGE, &ALL, &mut out), Ok(1));
        assert_eq!(out[0], (0x8040_0000, 4 * PAGE as u64));
        assert_eq!(device_runs(runs, 4 * PAGE, PAGE + 17, 100, &ALL, &mut out), Ok(1));
        assert_eq!(out[0], (0x8040_0000 + PAGE as u64 + 17, 100));
    }

    #[test]
    fn a_range_across_blocks_yields_the_pieces_in_object_order() {
        // Greedy anon blocks: 2 MiB, then 4 KiB far away, then 8 KiB.
        let runs = [(0x9000_0000usize, 2 * MIB), (0x8100_0000, PAGE), (0x8200_0000, 2 * PAGE)];
        let total = 2 * MIB + 3 * PAGE;
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(device_runs(runs, total, 0, total, &ALL, &mut out), Ok(3));
        assert_eq!(
            out[..3],
            [(0x9000_0000, 2 * MIB as u64), (0x8100_0000, 4096), (0x8200_0000, 8192)]
        );
        // The tail half of the first block plus half of the last one.
        assert_eq!(device_runs(runs, total, MIB, MIB + PAGE + PAGE, &ALL, &mut out), Ok(3));
        assert_eq!(out[0], (0x9000_0000 + MIB as u64, MIB as u64));
        assert_eq!(out[2], (0x8200_0000, 4096));
    }

    #[test]
    fn physically_adjacent_blocks_merge_into_one_run() {
        let runs = [(0x8000_0000usize, 2 * MIB), (0x8020_0000, MIB), (0x8030_0000, PAGE)];
        let total = 3 * MIB + PAGE;
        let mut out = [(0u64, 0u64); 1];
        assert_eq!(device_runs(runs, total, 0, total, &ALL, &mut out), Ok(1));
        assert_eq!(out[0], (0x8000_0000, total as u64));
    }

    #[test]
    fn a_run_at_physical_zero_is_an_address_like_any_other() {
        // The board's first bank starts at 0: 0 is not "no address".
        let runs = [(0usize, 2 * PAGE)];
        let mut out = [(u64::MAX, 0u64); 1];
        assert_eq!(device_runs(runs, 2 * PAGE, 0, 2 * PAGE, &storage(), &mut out), Ok(1));
        assert_eq!(out[0], (0, 2 * PAGE as u64));
    }

    #[test]
    fn a_translated_bus_answers_bus_addresses() {
        // The multimedia bus: CPU 4 GiB.. is bus 2 GiB.. for the device.
        let mm = DmaReach::new(&[
            DmaWindow { bus: 0, cpu: 0, size: 2 * GIB },
            DmaWindow { bus: 2 * GIB, cpu: 4 * GIB, size: 14 * GIB },
        ])
        .unwrap();
        let runs = [((4 * GIB) as usize + 0x20_0000, 2 * MIB), (0x4000_0000, MIB)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(device_runs(runs, 3 * MIB, 0, 3 * MIB, &mm, &mut out), Ok(2));
        assert_eq!(out[..2], [(2 * GIB + 0x20_0000, 2 * MIB as u64), (0x4000_0000, MIB as u64)]);
    }

    #[test]
    fn test_reject_vmo_runs_outside_the_device_reach() {
        // A storage master cannot reach the board's upper bank — refused, not truncated.
        let runs = [(0x4040_0000usize, PAGE), ((4 * GIB) as usize, PAGE)];
        let mut out = [(7u64, 7u64); 4];
        assert_eq!(
            device_runs(runs, 2 * PAGE, 0, 2 * PAGE, &storage(), &mut out),
            Err(RunsError::OutOfReach)
        );
        // Asking only for the part in reach is answered.
        assert_eq!(device_runs(runs, 2 * PAGE, 0, PAGE, &storage(), &mut out), Ok(1));
    }

    #[test]
    fn test_reject_vmo_runs_past_the_end() {
        let runs = [(0x8040_0000usize, 4 * PAGE)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(
            device_runs(runs, 4 * PAGE, 3 * PAGE, 2 * PAGE, &ALL, &mut out),
            Err(RunsError::Range)
        );
        assert_eq!(device_runs(runs, 4 * PAGE, 4 * PAGE, 1, &ALL, &mut out), Err(RunsError::Range));
    }

    #[test]
    fn test_reject_vmo_runs_empty_or_overflowing_range() {
        let runs = [(0x8040_0000usize, 4 * PAGE)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(device_runs(runs, 4 * PAGE, 0, 0, &ALL, &mut out), Err(RunsError::Range));
        assert_eq!(
            device_runs(runs, 4 * PAGE, usize::MAX, 2, &ALL, &mut out),
            Err(RunsError::Range)
        );
    }

    #[test]
    fn test_reject_vmo_runs_bad_answer_buffer() {
        let runs = [(0x8040_0000usize, PAGE)];
        assert_eq!(device_runs(runs, PAGE, 0, PAGE, &ALL, &mut []), Err(RunsError::Range));
        let mut huge = [(0u64, 0u64); MAX_RUNS + 1];
        assert_eq!(device_runs(runs, PAGE, 0, PAGE, &ALL, &mut huge), Err(RunsError::Range));
    }

    #[test]
    fn test_reject_vmo_runs_that_do_not_fit_instead_of_truncating() {
        let runs = [(0x8040_0000usize, PAGE), (0x8060_0000, PAGE)];
        let mut out = [(7u64, 7u64); 1];
        assert_eq!(
            device_runs(runs, 2 * PAGE, 0, 2 * PAGE, &ALL, &mut out),
            Err(RunsError::TooMany)
        );
    }

    #[test]
    fn test_reject_vmo_runs_of_an_object_shorter_than_its_capability() {
        let runs = [(0x8040_0000usize, PAGE)];
        let mut out = [(0u64, 0u64); 2];
        assert_eq!(device_runs(runs, 2 * PAGE, 0, 2 * PAGE, &ALL, &mut out), Err(RunsError::Range));
    }
}
