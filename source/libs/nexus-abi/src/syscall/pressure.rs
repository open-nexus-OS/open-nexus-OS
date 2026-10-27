// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Memory pressure at the ABI (TASK-0333 6e, RFC-0100 §3/§4, ADR-0085) — the levels,
//! the frame the kernel writes into the one bound endpoint and its decoder (pure, host-tested),
//! and the purgeable lock/unlock of a lazy VMO (syscall 68). Split out of `memory.rs` by the
//! structure gate; `memory` re-exports every item, so `nexus_abi::syscall::memory::…` and the
//! crate-root names keep resolving unchanged.

#[cfg(nexus_env = "os")]
use super::*;

/// The pressure levels of `MmStats::pressure_level` and the `OP_MEMORY_PRESSURE` frame
/// (ADR-0085): above every watermark …
pub const MEM_PRESSURE_NORMAL: u8 = 0;
/// … at or below `warn`: purgeable pages may go, caches should shrink …
pub const MEM_PRESSURE_WARN: u8 = 1;
/// … at or below `critical`: the hidden bands are killed, lowest first …
pub const MEM_PRESSURE_CRITICAL: u8 = 2;
/// … at or below `emergency`: whatever is not foreground goes.
pub const MEM_PRESSURE_EMERGENCY: u8 = 3;

/// The op byte of the frame the kernel writes into the endpoint bound with
/// [`mem_pressure_bind`](crate::syscall::caps::mem_pressure_bind) on every level change, and
/// once at the bind. Payload: `op: u8, level: u8, free: u64le, total: u64le` (frames).
pub const OP_MEMORY_PRESSURE: u8 = 0x71;

/// Decodes one pressure frame: `(level, free frames, total frames)`. `None` for anything else
/// on the endpoint or a level this ABI does not speak. Pure, host-tested.
pub fn decode_memory_pressure(frame: &[u8]) -> Option<(u8, u64, u64)> {
    if frame.len() != 18 || frame[0] != OP_MEMORY_PRESSURE || frame[1] > MEM_PRESSURE_EMERGENCY {
        return None;
    }
    let word = |at: usize| -> Option<u64> {
        let bytes: [u8; 8] = frame[at..at + 8].try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    };
    Some((frame[1], word(2)?, word(10)?))
}

/// The level's name on the console (`"?"` for one this ABI does not speak).
pub fn mem_pressure_name(level: u8) -> &'static str {
    match level {
        MEM_PRESSURE_NORMAL => "normal",
        MEM_PRESSURE_WARN => "warn",
        MEM_PRESSURE_CRITICAL => "critical",
        MEM_PRESSURE_EMERGENCY => "emergency",
        _ => "?",
    }
}

/// RFC-0100 §4 (TASK-0333 6e-3): unlocks the lazy VMO in `slot` for purging (syscall 68) — from
/// here on the kernel may drop its pages when the pool reaches `warn`, and the object is
/// untouchable (no map, write, read or `vmo_runs`) until [`vmo_lock_purgeable`]. Refused for an
/// eager object, one a device was told the runs of (a GPU binding is that fact), or one any
/// space maps: a cache is unmapped before it is unlocked.
#[cfg(nexus_env = "os")]
pub fn vmo_unlock_purgeable(slot: Handle) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_PURGEABLE: usize = 68;
        let raw = unsafe { ecall2(SYSCALL_VMO_PURGEABLE, slot as usize, 1) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = slot;
        Err(AbiError::Unsupported)
    }
}

/// RFC-0100 §4: locks the VMO in `slot` again (syscall 68) — the owner's from here on. `true`
/// when a purge took the content since the unlock: every page is zero on its next touch, the
/// cache is rebuilt. A locked object answers `false`.
#[cfg(nexus_env = "os")]
pub fn vmo_lock_purgeable(slot: Handle) -> SysResult<bool> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_PURGEABLE: usize = 68;
        let raw = unsafe { ecall2(SYSCALL_VMO_PURGEABLE, slot as usize, 0) };
        decode_syscall(raw).map(|purged| purged != 0)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = slot;
        Err(AbiError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(level: u8, free: u64, total: u64) -> [u8; 18] {
        let mut f = [0u8; 18];
        f[0] = OP_MEMORY_PRESSURE;
        f[1] = level;
        f[2..10].copy_from_slice(&free.to_le_bytes());
        f[10..18].copy_from_slice(&total.to_le_bytes());
        f
    }

    #[test]
    fn a_pressure_frame_decodes_to_its_level_and_counts() {
        assert_eq!(
            decode_memory_pressure(&frame(MEM_PRESSURE_CRITICAL, 4_338, 77_261)),
            Some((MEM_PRESSURE_CRITICAL, 4_338, 77_261))
        );
        assert_eq!(mem_pressure_name(MEM_PRESSURE_WARN), "warn");
    }

    #[test]
    fn test_reject_a_short_foreign_or_unknown_pressure_frame() {
        let f = frame(MEM_PRESSURE_WARN, 1, 2);
        assert_eq!(decode_memory_pressure(&f[..17]), None);
        let mut foreign = f;
        foreign[0] = 0x70;
        assert_eq!(decode_memory_pressure(&foreign), None);
        let mut unknown = f;
        unknown[1] = 4;
        assert_eq!(decode_memory_pressure(&unknown), None);
        assert_eq!(mem_pressure_name(9), "?");
    }
}
