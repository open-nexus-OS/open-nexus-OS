// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the one door a physical address leaves the kernel by (RFC-0098 C4,
//! TASK-0286 P4a). `vmo_runs` (syscall 60) hands a driver the `(pa, len)` runs
//! behind a byte range of a VMO so it can program a bus master — a virtio queue's
//! one base, a scatter-gather resource backing. The authority rule and the
//! clipping live here, pure, so the reject matrix runs on host; the syscall
//! (`syscall/api/vmo_runs.rs`) only resolves the capability and copies the
//! answer out. `cap_query` names no VMO base.
//! NOT target-gated (pure arithmetic); `mod mm` is riscv/none-only, hence the
//! `#[path]` in `lib.rs`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (authority + clipping reject matrix); the wiring
//!   by `KSELFTEST: vmo runs ok (…)` and every DMA driver on every QEMU boot
//! INVARIANTS: no run is reported to a task without a device capability; a
//!   read-only alias never yields a run; the runs lie inside the object and
//!   cover exactly the asked range; the answer is complete or refused, never
//!   truncated.

/// Runs one call may return (16 bytes each: a 4 KiB answer at most).
pub const MAX_RUNS: usize = 256;

/// Bytes of one run in the user buffer: `pa: u64` then `len: u64`, little endian.
pub const RUN_BYTES: usize = 16;

/// What the capability in the slot is, as far as this door cares.
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
    /// The caller holds no device capability: it has nothing to program.
    NoDevice,
    /// Empty or overflowing range, past the object's end, the object shorter
    /// than its capability, or an answer buffer of 0 or more than `MAX_RUNS`.
    Range,
    /// The range needs more runs than the answer buffer holds.
    TooMany,
}

/// The authority rule: a writable VMO, and a caller that holds a device.
pub fn authorize(holds_device: bool, holding: Holding) -> Result<(), RunsError> {
    match holding {
        Holding::Other => Err(RunsError::NotVmo),
        Holding::ReadOnlyAlias => Err(RunsError::ReadOnly),
        Holding::Vmo if !holds_device => Err(RunsError::NoDevice),
        Holding::Vmo => Ok(()),
    }
}

/// Clip the object's runs (`(pa, len)` in object order) to the byte range
/// `offset..offset + len` of an object of `object_len` bytes into `out`,
/// merging physically adjacent pieces. Returns how many runs were written.
pub fn clip<I>(
    runs: I,
    object_len: usize,
    offset: usize,
    len: usize,
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
            let piece_pa = pa.checked_add(lo - at).ok_or(RunsError::Range)? as u64;
            let piece_len = (hi - lo) as u64;
            match written.checked_sub(1).map(|i| &mut out[i]) {
                Some(last) if last.0 + last.1 == piece_pa => last.1 += piece_len,
                _ => {
                    let slot = out.get_mut(written).ok_or(RunsError::TooMany)?;
                    *slot = (piece_pa, piece_len);
                    written += 1;
                }
            }
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

    const PAGE: usize = 4096;
    const MIB: usize = 1 << 20;

    #[test]
    fn a_driver_with_a_writable_vmo_is_let_through() {
        assert_eq!(authorize(true, Holding::Vmo), Ok(()));
    }

    #[test]
    fn test_reject_vmo_runs_without_a_device_capability() {
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
        assert_eq!(clip(runs, 4 * PAGE, 0, 4 * PAGE, &mut out), Ok(1));
        assert_eq!(out[0], (0x8040_0000, 4 * PAGE as u64));
        assert_eq!(clip(runs, 4 * PAGE, PAGE + 17, 100, &mut out), Ok(1));
        assert_eq!(out[0], (0x8040_0000 + PAGE as u64 + 17, 100));
    }

    #[test]
    fn a_range_across_blocks_yields_the_pieces_in_object_order() {
        // Greedy anon blocks: 2 MiB, then 4 KiB far away, then 8 KiB.
        let runs = [(0x9000_0000usize, 2 * MIB), (0x8100_0000, PAGE), (0x8200_0000, 2 * PAGE)];
        let total = 2 * MIB + 3 * PAGE;
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(clip(runs, total, 0, total, &mut out), Ok(3));
        assert_eq!(
            out[..3],
            [(0x9000_0000, 2 * MIB as u64), (0x8100_0000, 4096), (0x8200_0000, 8192)]
        );
        // The tail half of the first block plus half of the last one.
        assert_eq!(clip(runs, total, MIB, MIB + PAGE + PAGE, &mut out), Ok(3));
        assert_eq!(out[0], (0x9000_0000 + MIB as u64, MIB as u64));
        assert_eq!(out[2], (0x8200_0000, 4096));
    }

    #[test]
    fn physically_adjacent_blocks_merge_into_one_run() {
        let runs = [(0x8000_0000usize, 2 * MIB), (0x8020_0000, MIB), (0x8030_0000, PAGE)];
        let total = 3 * MIB + PAGE;
        let mut out = [(0u64, 0u64); 1];
        assert_eq!(clip(runs, total, 0, total, &mut out), Ok(1));
        assert_eq!(out[0], (0x8000_0000, total as u64));
    }

    #[test]
    fn a_run_at_physical_zero_is_an_address_like_any_other() {
        // The board's first bank starts at 0: 0 is not "no address".
        let runs = [(0usize, 2 * PAGE)];
        let mut out = [(u64::MAX, 0u64); 1];
        assert_eq!(clip(runs, 2 * PAGE, 0, 2 * PAGE, &mut out), Ok(1));
        assert_eq!(out[0], (0, 2 * PAGE as u64));
    }

    #[test]
    fn test_reject_vmo_runs_past_the_end() {
        let runs = [(0x8040_0000usize, 4 * PAGE)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(clip(runs, 4 * PAGE, 3 * PAGE, 2 * PAGE, &mut out), Err(RunsError::Range));
        assert_eq!(clip(runs, 4 * PAGE, 4 * PAGE, 1, &mut out), Err(RunsError::Range));
    }

    #[test]
    fn test_reject_vmo_runs_empty_or_overflowing_range() {
        let runs = [(0x8040_0000usize, 4 * PAGE)];
        let mut out = [(0u64, 0u64); 4];
        assert_eq!(clip(runs, 4 * PAGE, 0, 0, &mut out), Err(RunsError::Range));
        assert_eq!(clip(runs, 4 * PAGE, usize::MAX, 2, &mut out), Err(RunsError::Range));
    }

    #[test]
    fn test_reject_vmo_runs_bad_answer_buffer() {
        let runs = [(0x8040_0000usize, PAGE)];
        assert_eq!(clip(runs, PAGE, 0, PAGE, &mut []), Err(RunsError::Range));
        let mut huge = [(0u64, 0u64); MAX_RUNS + 1];
        assert_eq!(clip(runs, PAGE, 0, PAGE, &mut huge), Err(RunsError::Range));
    }

    #[test]
    fn test_reject_vmo_runs_that_do_not_fit_instead_of_truncating() {
        let runs = [(0x8040_0000usize, PAGE), (0x8060_0000, PAGE)];
        let mut out = [(7u64, 7u64); 1];
        assert_eq!(clip(runs, 2 * PAGE, 0, 2 * PAGE, &mut out), Err(RunsError::TooMany));
    }

    #[test]
    fn test_reject_vmo_runs_of_an_object_shorter_than_its_capability() {
        let runs = [(0x8040_0000usize, PAGE)];
        let mut out = [(0u64, 0u64); 2];
        assert_eq!(clip(runs, 2 * PAGE, 0, 2 * PAGE, &mut out), Err(RunsError::Range));
    }
}
