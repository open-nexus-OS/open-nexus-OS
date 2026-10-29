// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the swap stage at the ABI (TASK-0333 6f-2, RFC-0100 §5, ADR-0088) — the four calls
//! (70: the RNG service seeds the boot's key once; 71–73: the pager — the pressure sink's holder,
//! memd — exports sealed records, commits them to its slots, pages them back), the export
//! buffer's decoder, and the pager's two notices on the pressure sink: `OP_PAGE_IN {token, slot}`
//! (a thread waits for that page) and `OP_SWAP_DROP {slots}` (an object died with them). The
//! pager only ever holds ciphertext: a record opens in the kernel alone, as the page and the
//! export it was sealed for. The decoders are pure and host-tested; the calls are the OS's.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable (versioned buffer: `SWAP_EXPORT_VERSION`)
//! TEST_COVERAGE: tests below (the buffer's entries, the notices, the refusals of a short, a
//!   foreign or an overrunning frame); QEMU `SELFTEST: swap out ok (…)` / `swap in ok (…)`

#[cfg(nexus_env = "os")]
use super::*;

/// Bytes of the boot's swap key (the RNG service's seed).
pub const SWAP_KEY_LEN: usize = 32;
/// The export buffer's layout version.
pub const SWAP_EXPORT_VERSION: u32 = 1;
/// The buffer's header: `version: u32`, `count: u32`.
pub const SWAP_EXPORT_HEADER: usize = 8;
/// An entry's head: `token: u64`, `ctr: u64`, `len: u16`, `pad: u16`.
pub const SWAP_ENTRY_HEAD: usize = 20;
/// Records one export seals at most.
pub const SWAP_EXPORT_BATCH: usize = 16;
/// The largest sealed record: the largest record the kernel's pool keeps (7/8 of a page) and the
/// seal's 16-byte tag. One pager slot (4 KiB) holds one.
pub const SWAP_MAX_SEALED: usize = 4096 / 8 * 7 + 16;
/// The buffer one export fills at most (a longer one is used up to here).
pub const SWAP_EXPORT_MAX_BUF: usize =
    SWAP_EXPORT_HEADER + SWAP_EXPORT_BATCH * swap_entry_len(SWAP_MAX_SEALED);
/// The smallest buffer an export takes.
pub const SWAP_EXPORT_MIN_BUF: usize = SWAP_EXPORT_HEADER + swap_entry_len(SWAP_MAX_SEALED);

/// The op byte of a page-in request on the pressure sink: `[op, token: u64le, slot: u32le]`.
pub const OP_PAGE_IN: u8 = 0x72;
/// The op byte of a drop on the pressure sink: `[op, count: u8, slot: u32le × count]`.
pub const OP_SWAP_DROP: u8 = 0x73;
/// Slots one drop frame carries at most.
pub const SWAP_DROP_BATCH: usize = 32;

/// An entry's bytes for a sealed record of `sealed` bytes (padded to 4).
#[must_use]
pub const fn swap_entry_len(sealed: usize) -> usize {
    SWAP_ENTRY_HEAD + sealed.next_multiple_of(4)
}

/// `(object, page)` of a swap token — for a log line; the pager never needs to know which.
#[must_use]
pub const fn swap_token_parts(token: u64) -> (u32, u32) {
    ((token >> 32) as u32, token as u32)
}

/// One sealed record of an export buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapEntry<'a> {
    /// The page: commit and page-in name it.
    pub token: u64,
    /// The export it was sealed for: the commit names it back.
    pub ctr: u64,
    /// The sealed bytes (the pager writes them to one slot, unchanged).
    pub sealed: &'a [u8],
}

/// The entries of an export buffer, validated as a whole first: `None` unless the header is
/// this layout and every entry lies inside the buffer and within the seal's bound.
#[derive(Clone, Debug)]
pub struct SwapEntries<'a> {
    buf: &'a [u8],
    at: usize,
    left: u32,
}

impl<'a> SwapEntries<'a> {
    /// The buffer an export wrote. Pure.
    #[must_use]
    pub fn decode(buf: &'a [u8]) -> Option<Self> {
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
        };
        if word(0)? != SWAP_EXPORT_VERSION {
            return None;
        }
        let count = word(4)?;
        if count as usize > SWAP_EXPORT_BATCH {
            return None;
        }
        let mut at = SWAP_EXPORT_HEADER;
        for _ in 0..count {
            let (_, next) = entry_at(buf, at)?;
            at = next;
        }
        Some(Self { buf, at: SWAP_EXPORT_HEADER, left: count })
    }

    /// Entries left.
    #[must_use]
    pub fn len(&self) -> usize {
        self.left as usize
    }

    /// No entry left.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.left == 0
    }
}

impl<'a> Iterator for SwapEntries<'a> {
    type Item = SwapEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return None;
        }
        let (entry, next) = entry_at(self.buf, self.at)?;
        self.at = next;
        self.left -= 1;
        Some(entry)
    }
}

/// The entry at `at` and where the next one starts.
fn entry_at(buf: &[u8], at: usize) -> Option<(SwapEntry<'_>, usize)> {
    let long = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(buf.get(at..at + 8)?.try_into().ok()?))
    };
    let len = usize::from(u16::from_le_bytes(buf.get(at + 16..at + 18)?.try_into().ok()?));
    if len > SWAP_MAX_SEALED {
        return None;
    }
    let body = at.checked_add(SWAP_ENTRY_HEAD)?;
    let sealed = buf.get(body..body.checked_add(len)?)?;
    let next = at.checked_add(swap_entry_len(len))?;
    Some((SwapEntry { token: long(at)?, ctr: long(at + 8)?, sealed }, next))
}

/// `(token, slot)` of a page-in request; `None` for anything else on the sink. Pure.
#[must_use]
pub fn decode_page_in(frame: &[u8]) -> Option<(u64, u32)> {
    if frame.len() != 13 || frame[0] != OP_PAGE_IN {
        return None;
    }
    let token = u64::from_le_bytes(frame[1..9].try_into().ok()?);
    Some((token, u32::from_le_bytes(frame[9..13].try_into().ok()?)))
}

/// The slots of a drop frame into `out`; answers how many. `None` for anything else, or a frame
/// whose count disagrees with its length. Pure.
pub fn decode_swap_drops(frame: &[u8], out: &mut [u32; SWAP_DROP_BATCH]) -> Option<usize> {
    let (&op, rest) = frame.split_first()?;
    let (&count, slots) = rest.split_first()?;
    let n = usize::from(count);
    if op != OP_SWAP_DROP || n > SWAP_DROP_BATCH || slots.len() != n * 4 {
        return None;
    }
    for (i, bytes) in slots.chunks_exact(4).enumerate() {
        out[i] = u32::from_le_bytes(bytes.try_into().ok()?);
    }
    Some(n)
}

/// Seeds the boot's swap key (syscall 70) — the RNG service alone, once per boot (`Busy` for a
/// second key, `CapabilityDenied` for anyone else). The caller wipes its copy afterwards.
#[cfg(nexus_env = "os")]
pub fn swap_key(key: &[u8; SWAP_KEY_LEN]) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SWAP_KEY: usize = 70;
        // SAFETY: `key` is a live, readable buffer of `SWAP_KEY_LEN` bytes.
        let raw = unsafe { ecall2(SYSCALL_SWAP_KEY, key.as_ptr() as usize, key.len()) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = key;
        Err(AbiError::Unsupported)
    }
}

/// Exports one batch of process `pid`'s records from `from_va` (`0` to start) into `buf` (at
/// least [`SWAP_EXPORT_MIN_BUF`] bytes; read it with [`SwapEntries::decode`]): answers the
/// address to pass next (`0`: done). The pager alone (`CapabilityDenied` otherwise);
/// `InvalidArgument` without a key (swap is off), `NoSuchPid` for no such process.
#[cfg(nexus_env = "os")]
pub fn swap_export(pid: Pid, from_va: usize, buf: &mut [u8]) -> SysResult<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SWAP_EXPORT: usize = 71;
        // SAFETY: `buf` is a live, writable buffer of `buf.len()` bytes.
        let raw = unsafe {
            ecall4(SYSCALL_SWAP_EXPORT, pid as usize, from_va, buf.as_mut_ptr() as usize, buf.len())
        };
        decode_syscall(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (pid, from_va, buf);
        Err(AbiError::Unsupported)
    }
}

/// The record of `token` sealed under `ctr` is in the pager's `slot` (syscall 72): the page is
/// swapped, its record released. `NoSuchPid` (ESRCH): the export is no longer the page's — a
/// touch brought it back, or a later export superseded it; the slot is the pager's to free.
#[cfg(nexus_env = "os")]
pub fn swap_commit(token: u64, ctr: u64, slot: u32) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SWAP_COMMIT: usize = 72;
        let raw =
            unsafe { ecall3(SYSCALL_SWAP_COMMIT, token as usize, ctr as usize, slot as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (token, ctr, slot);
        Err(AbiError::Unsupported)
    }
}

/// The pager's answer for `token` (syscall 73): `sealed`, the record it wrote to `slot`. The
/// page is back and every thread parked on it woken. `NoSuchPid`: stale (the page went away or
/// moved on — the slot is the pager's to free); `InvalidArgument`: the record did not open (the
/// threads parked on it were killed); `OutOfMemory`: no frame now, the request stands.
#[cfg(nexus_env = "os")]
pub fn swap_page_in(token: u64, slot: u32, sealed: &[u8]) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SWAP_PAGE_IN: usize = 73;
        // SAFETY: `sealed` is a live, readable buffer of `sealed.len()` bytes.
        let raw = unsafe {
            ecall4(
                SYSCALL_SWAP_PAGE_IN,
                token as usize,
                slot as usize,
                sealed.as_ptr() as usize,
                sealed.len(),
            )
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (token, slot, sealed);
        Err(AbiError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A buffer as the kernel writes it: the header, then `(token, ctr, sealed)` entries.
    fn buffer(entries: &[(u64, u64, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&SWAP_EXPORT_VERSION.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (token, ctr, sealed) in entries {
            out.extend_from_slice(&token.to_le_bytes());
            out.extend_from_slice(&ctr.to_le_bytes());
            out.extend_from_slice(&(sealed.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(sealed);
            out.resize(out.len() + (4 - sealed.len() % 4) % 4, 0);
        }
        out
    }

    #[test]
    fn an_export_buffer_yields_its_entries_in_order() {
        let buf = buffer(&[((7 << 32) | 3, 41, &[1, 2, 3]), ((7 << 32) | 4, 42, &[9; 40])]);
        let entries = SwapEntries::decode(&buf).unwrap();
        assert_eq!(entries.len(), 2);
        let all: Vec<SwapEntry<'_>> = entries.collect();
        assert_eq!(all[0], SwapEntry { token: (7 << 32) | 3, ctr: 41, sealed: &[1, 2, 3] });
        assert_eq!((all[1].ctr, all[1].sealed.len()), (42, 40));
        assert_eq!(swap_token_parts(all[1].token), (7, 4));
        assert!(SwapEntries::decode(&buffer(&[])).unwrap().is_empty());
        assert_eq!(SWAP_EXPORT_MIN_BUF, 8 + 20 + 3600);
        assert_eq!(SWAP_EXPORT_MAX_BUF, 8 + 16 * 3620);
    }

    #[test]
    fn test_reject_a_foreign_or_overrunning_export_buffer() {
        let good = buffer(&[(1, 1, &[5; 10])]);
        assert!(SwapEntries::decode(&good[..good.len() - 4]).is_none(), "an entry cut short");
        let mut version = good.clone();
        version[0] = 2;
        assert!(SwapEntries::decode(&version).is_none(), "another layout");
        let mut count = good.clone();
        count[4] = 2;
        assert!(SwapEntries::decode(&count).is_none(), "more entries than bytes");
        let mut many = good.clone();
        many[4] = 17;
        assert!(SwapEntries::decode(&many).is_none(), "over a batch");
        let mut long = good;
        long[24..26].copy_from_slice(&(SWAP_MAX_SEALED as u16 + 1).to_le_bytes());
        assert!(SwapEntries::decode(&long).is_none(), "over the seal's bound");
        assert!(SwapEntries::decode(&[1, 0, 0]).is_none(), "no header");
    }

    #[test]
    fn the_notices_decode_and_test_reject_malformed_ones() {
        let mut request = [0u8; 13];
        request[0] = OP_PAGE_IN;
        request[1..9].copy_from_slice(&((3u64 << 32) | 9).to_le_bytes());
        request[9..13].copy_from_slice(&700u32.to_le_bytes());
        assert_eq!(decode_page_in(&request), Some(((3 << 32) | 9, 700)));
        assert_eq!(decode_page_in(&request[..12]), None, "short");
        let mut drop = vec![OP_SWAP_DROP, 2];
        drop.extend_from_slice(&5u32.to_le_bytes());
        drop.extend_from_slice(&6u32.to_le_bytes());
        let mut slots = [0u32; SWAP_DROP_BATCH];
        assert_eq!(decode_swap_drops(&drop, &mut slots), Some(2));
        assert_eq!(slots[..2], [5, 6]);
        assert_eq!(decode_swap_drops(&drop[..9], &mut slots), None, "a slot cut short");
        assert_eq!(decode_swap_drops(&request, &mut slots), None, "a request is no drop");
        assert_eq!(decode_page_in(&drop), None, "a drop is no request");
        let mut over = vec![OP_SWAP_DROP, 33];
        over.resize(2 + 33 * 4, 0);
        assert_eq!(decode_swap_drops(&over, &mut slots), None, "more than a batch");
    }
}
