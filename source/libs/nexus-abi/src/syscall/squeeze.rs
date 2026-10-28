// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the squeeze at the ABI (TASK-0333 6f-1, RFC-0100 §5, ADR-0087) — `task_squeeze`
//! (syscall 69): the PARENT orders one bounded batch of a parked child's private lazy pages
//! compressed (pages of zeroes dropped, the rest recorded in their objects' stores; a touch
//! brings each back). One call is one batch; the caller passes the address the last call
//! answered until it answers `0`, and adds the records up. The record's decode is pure and
//! host-tested; the call is the OS's.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable (versioned record: `SQUEEZE_BATCH_VERSION`)
//! TEST_COVERAGE: tests below (the record's layout, the refusals of a short or foreign one);
//!   QEMU `SELFTEST: exec child squeeze ok (…)`

#[cfg(nexus_env = "os")]
use super::*;

/// The batch record's layout version.
pub const SQUEEZE_BATCH_VERSION: u32 = 1;
/// The record's 64-bit fields after its 8-byte header.
pub const SQUEEZE_BATCH_FIELDS: u32 = 5;
/// The record's bytes.
pub const SQUEEZE_BATCH_BYTES: usize = 8 + SQUEEZE_BATCH_FIELDS as usize * 8;

/// What one batch did (or, added up, a whole squeeze).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SqueezeBatch {
    /// Pages recorded compressed.
    pub squeezed: u64,
    /// Pages of zeroes dropped (their next touch commits a zeroed frame).
    pub zero: u64,
    /// Committed pages kept as they are (they would not compress, or the store had no frame).
    pub kept: u64,
    /// Frames the stores took.
    pub taken: u64,
    /// Bytes of the records written.
    pub stored: u64,
}

impl SqueezeBatch {
    /// The record the kernel wrote, `None` unless it is this layout. Pure.
    #[must_use]
    pub fn decode(record: &[u8]) -> Option<Self> {
        if record.len() < SQUEEZE_BATCH_BYTES {
            return None;
        }
        let half = |at: usize| {
            u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]])
        };
        if half(0) != SQUEEZE_BATCH_VERSION || half(4) != SQUEEZE_BATCH_FIELDS {
            return None;
        }
        let word = |i: usize| {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&record[8 + i * 8..16 + i * 8]);
            u64::from_le_bytes(bytes)
        };
        Some(Self {
            squeezed: word(0),
            zero: word(1),
            kept: word(2),
            taken: word(3),
            stored: word(4),
        })
    }

    /// `self` plus `other`, field by field (a squeeze's total over its batches).
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            squeezed: self.squeezed.saturating_add(other.squeezed),
            zero: self.zero.saturating_add(other.zero),
            kept: self.kept.saturating_add(other.kept),
            taken: self.taken.saturating_add(other.taken),
            stored: self.stored.saturating_add(other.stored),
        }
    }

    /// Frames the squeeze gave back net: every squeezed and every zero page freed one, the
    /// stores took `taken`.
    #[must_use]
    pub fn net_frames(&self) -> i64 {
        (self.squeezed + self.zero) as i64 - self.taken as i64
    }
}

/// Squeezes one batch of the parked CHILD `pid` from `from_va` (`0` to start): answers the
/// address to pass next (`0`: the walk reached the end) and what the batch did.
/// `Err(NoSuchPid)` for a task that is not the caller's live child; the kernel's `EBUSY` while a
/// task of the child still runs (freeze it first — a freeze on its way to another hart is not
/// parked yet).
#[cfg(nexus_env = "os")]
pub fn task_squeeze(pid: Pid, from_va: usize) -> SysResult<(usize, SqueezeBatch)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_SQUEEZE: usize = 69;
        let mut record = [0u8; SQUEEZE_BATCH_BYTES];
        // SAFETY: `record` is a live, writable buffer of `SQUEEZE_BATCH_BYTES` bytes.
        let raw = unsafe {
            ecall4(
                SYSCALL_TASK_SQUEEZE,
                pid as usize,
                from_va,
                record.as_mut_ptr() as usize,
                record.len(),
            )
        };
        let next = decode_syscall(raw)?;
        let batch = SqueezeBatch::decode(&record).ok_or(AbiError::Unsupported)?;
        Ok((next, batch))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (pid, from_va);
        Err(AbiError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: [u64; 5]) -> [u8; SQUEEZE_BATCH_BYTES] {
        let mut out = [0u8; SQUEEZE_BATCH_BYTES];
        out[0..4].copy_from_slice(&SQUEEZE_BATCH_VERSION.to_le_bytes());
        out[4..8].copy_from_slice(&SQUEEZE_BATCH_FIELDS.to_le_bytes());
        for (i, value) in fields.iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_batch_record_decodes_in_field_order_and_adds_up() {
        let a = SqueezeBatch::decode(&record([4, 1, 1, 1, 520])).unwrap();
        assert_eq!(a, SqueezeBatch { squeezed: 4, zero: 1, kept: 1, taken: 1, stored: 520 });
        let b = SqueezeBatch::decode(&record([10, 0, 2, 3, 9000])).unwrap();
        let total = a.plus(b);
        assert_eq!((total.squeezed, total.kept, total.stored), (14, 3, 9520));
        assert_eq!(total.net_frames(), 14 + 1 - 4);
    }

    #[test]
    fn test_reject_a_short_or_foreign_batch_record() {
        let good = record([1, 2, 3, 4, 5]);
        assert!(SqueezeBatch::decode(&good[..SQUEEZE_BATCH_BYTES - 1]).is_none(), "short");
        let mut version = good;
        version[0] = 2;
        assert!(SqueezeBatch::decode(&version).is_none(), "another layout");
        let mut fields = good;
        fields[4] = 4;
        assert!(SqueezeBatch::decode(&fields).is_none(), "another field count");
    }
}
