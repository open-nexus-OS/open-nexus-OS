// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the IPC payload and its two tiers (TASK-0054C P3b, RFC-0096).
//! A control message travels INLINE in the message itself and costs the kernel
//! no heap; anything larger is a heap payload; anything above
//! [`IPC_PAYLOAD_MAX`] is refused with `E2BIG` so bulk cannot drift inline
//! unnoticed (bulk belongs on a VMO — RFC-0096 §"Copies").
//!
//! WHY 32 AND NOT 64: P3a measured the distribution instead of guessing it
//! (`KSELFTEST: ipc payload hist`, 13 windows, two workload families). In the
//! standard boot every bucket above 32 B is constant — the fixed boot work —
//! while everything that SCALES with the window is ≤ 32 B (72.3 % to 93.1 % of
//! all messages). Raising the tier to RFC-0096's original 64 would not buy a
//! percentage, it would buy a fixed ~160 messages per boot for twice the inline
//! footprint in a value that is moved on every send, push, pop and error
//! return. The numbers are in RFC-0096 §Amendment 2026-09-16.
//!
//! NOT TARGET-GATED, on purpose: `mod ipc` is `cfg(target_os = "none")`, so
//! anything living under it can never be tested on the host. The tier choice
//! and the bounds are pure logic, so they are declared at the crate root
//! (`crate::ipc_payload`, like `ipc_stats` and `ipc_eof`) and tested by
//! `just test-kernel`.
//!
//! This module touches NO global counters: `ipc_stats`' own test asserts exact
//! values after a reset, and cargo runs tests in parallel. The syscall layer
//! counts, using [`Payload::is_heap`].
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal (the CONSTANTS are ABI — mirrored in `nexus-abi`
//!   and kept honest by `scripts/check-ipc-bounds.sh`)
//! TEST_COVERAGE: this file's `tests` (via `just test-kernel`, in `test-all`)
//! RFC: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md

use alloc::vec::Vec;

/// Payloads up to this many bytes travel inline, costing zero kernel heap.
///
/// ABI: mirrored as `nexus_abi::IPC_SHORT_MAX`. The kernel does not depend on
/// `nexus-abi` (it is the defining side of the ABI), so the two are kept in
/// step by `scripts/check-ipc-bounds.sh` rather than by hope.
pub const IPC_SHORT_MAX: usize = 32;

/// Hard transport cap. A larger payload is refused with `E2BIG`
/// ([`crate::ipc::IpcError::TooBig`]) at every send-side trap — never
/// truncated, never silently split. Bulk travels as a VMO moved with the
/// message (RFC-0096).
///
/// ABI: mirrored as `nexus_abi::IPC_PAYLOAD_MAX` (same gate as above).
pub const IPC_PAYLOAD_MAX: usize = 8 * 1024;

/// A message payload in whichever tier fits it.
///
/// `Inline` keeps the bytes in the message, so a control message costs no
/// allocation and no free on either side of the queue. `Heap` is the same
/// `Vec<u8>` the whole path used before this tier existed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Up to [`IPC_SHORT_MAX`] bytes, stored in the message itself.
    Inline {
        /// Number of valid bytes in `bytes` (`<= IPC_SHORT_MAX`).
        len: u8,
        /// The payload, zero-padded past `len`.
        bytes: [u8; IPC_SHORT_MAX],
    },
    /// More than [`IPC_SHORT_MAX`] bytes, on the kernel heap.
    Heap(Vec<u8>),
}

impl Default for Payload {
    fn default() -> Self {
        Self::empty()
    }
}

impl Payload {
    /// An empty payload. Inline, so it allocates nothing.
    #[inline]
    pub const fn empty() -> Self {
        Self::Inline { len: 0, bytes: [0u8; IPC_SHORT_MAX] }
    }

    /// Copies `src` into whichever tier fits it.
    pub fn from_slice(src: &[u8]) -> Self {
        if src.len() <= IPC_SHORT_MAX {
            let mut bytes = [0u8; IPC_SHORT_MAX];
            bytes[..src.len()].copy_from_slice(src);
            Self::Inline { len: src.len() as u8, bytes }
        } else {
            Self::Heap(src.to_vec())
        }
    }

    /// Takes ownership of an existing `Vec`.
    ///
    /// A short `Vec` is NOT folded into the inline tier: it is already
    /// allocated, and copying it down would add a copy and a free to save
    /// nothing. Callers that can avoid the `Vec` use [`Payload::from_slice`] or
    /// [`Payload::copy_in_from_user`] instead.
    #[inline]
    pub fn from_vec(v: Vec<u8>) -> Self {
        Self::Heap(v)
    }

    /// Copies `len` bytes from a caller-validated user pointer into whichever
    /// tier fits — the send path's entry point, and the reason the inline tier
    /// saves an allocation rather than moving one around.
    ///
    /// # Safety
    /// `src` must point to at least `len` readable bytes in the CURRENT address
    /// space (the syscall layer proves this with `ensure_user_slice` before
    /// calling), and `len` must not exceed [`IPC_PAYLOAD_MAX`].
    pub unsafe fn copy_in_from_user(src: *const u8, len: usize) -> Self {
        if len <= IPC_SHORT_MAX {
            let mut bytes = [0u8; IPC_SHORT_MAX];
            if len != 0 {
                // SAFETY: caller guarantees `len` readable bytes at `src`; the
                // destination is a local array of IPC_SHORT_MAX >= len bytes.
                unsafe { core::ptr::copy_nonoverlapping(src, bytes.as_mut_ptr(), len) };
            }
            Self::Inline { len: len as u8, bytes }
        } else {
            let mut v = alloc::vec![0u8; len];
            // SAFETY: as above; `v` was just sized to exactly `len` bytes.
            unsafe { core::ptr::copy_nonoverlapping(src, v.as_mut_ptr(), len) };
            Self::Heap(v)
        }
    }

    /// True when this payload cost a kernel heap allocation. The syscall layer
    /// counts allocations with this, so `heap_allocs` in `KSELFTEST: ipc stats`
    /// means what it says.
    #[inline]
    pub fn is_heap(&self) -> bool {
        matches!(self, Self::Heap(_))
    }

    /// Number of valid payload bytes.
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len as usize,
            Self::Heap(v) => v.len(),
        }
    }

    /// True when the payload carries no bytes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The payload bytes.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline { len, bytes } => &bytes[..*len as usize],
            Self::Heap(v) => v.as_slice(),
        }
    }

    /// Start of the payload bytes, for the copy-out path.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.as_slice().as_ptr()
    }

    /// Shortens the payload to `n` bytes, keeping the tier it is already in.
    /// Growing is not possible — `n` beyond the current length does nothing.
    pub fn truncate(&mut self, n: usize) {
        match self {
            Self::Inline { len, bytes } => {
                if n < *len as usize {
                    bytes[n..*len as usize].fill(0);
                    *len = n as u8;
                }
            }
            Self::Heap(v) => v.truncate(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_payloads_are_inline_and_long_ones_are_not() {
        for len in [0usize, 1, 31, IPC_SHORT_MAX] {
            let p = Payload::from_slice(&vec![7u8; len]);
            assert!(!p.is_heap(), "{len} bytes must not allocate");
            assert_eq!(p.len(), len);
        }
        for len in [IPC_SHORT_MAX + 1, 512, IPC_PAYLOAD_MAX] {
            let p = Payload::from_slice(&vec![7u8; len]);
            assert!(p.is_heap(), "{len} bytes must be a heap payload");
            assert_eq!(p.len(), len);
        }
    }

    #[test]
    fn the_tier_never_changes_the_bytes() {
        for len in [0usize, 1, 32, 33, 1000] {
            let src: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let inline_or_heap = Payload::from_slice(&src);
            assert_eq!(inline_or_heap.as_slice(), &src[..]);
            // The copy-in path must agree byte for byte with `from_slice`.
            // SAFETY: `src` is a live slice of exactly `len` bytes.
            let copied = unsafe { Payload::copy_in_from_user(src.as_ptr(), len) };
            assert_eq!(copied, inline_or_heap);
        }
    }

    #[test]
    fn truncate_shortens_without_changing_tier_or_leaking_bytes() {
        let mut p = Payload::from_slice(&[9u8; 20]);
        p.truncate(4);
        assert_eq!(p.as_slice(), &[9u8; 4]);
        assert!(!p.is_heap());
        // The bytes past the new length are cleared, so a later widening bug
        // cannot expose a previous message's tail.
        match &p {
            Payload::Inline { bytes, .. } => assert!(bytes[4..].iter().all(|b| *b == 0)),
            Payload::Heap(_) => panic!("tier changed"),
        }
        p.truncate(99);
        assert_eq!(p.len(), 4, "truncate must never grow a payload");

        let mut h = Payload::from_slice(&[1u8; 100]);
        h.truncate(40);
        assert_eq!(h.len(), 40);
        assert!(h.is_heap(), "truncation must not silently re-tier");
    }

    #[test]
    fn an_owned_vec_keeps_its_allocation() {
        // from_vec must not copy a short Vec down into the inline tier: the
        // allocation already happened, so folding it would add a copy and a
        // free to save nothing.
        let p = Payload::from_vec(vec![1, 2, 3]);
        assert!(p.is_heap());
        assert_eq!(p.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn a_short_payload_costs_zero_allocations_and_a_long_one_costs_exactly_one() {
        // The tier's entire claim, counted rather than read (TASK-0054C P3b).
        // `copy_in_from_user` is the send path's entry point, so this is the
        // hot path itself, not a proxy for it.
        let short = [7u8; IPC_SHORT_MAX];
        let before = crate::test_alloc::thread_allocs();
        // SAFETY: `short` is a live local of exactly IPC_SHORT_MAX bytes.
        let p = unsafe { Payload::copy_in_from_user(short.as_ptr(), short.len()) };
        let spent = crate::test_alloc::thread_allocs() - before;
        assert!(!p.is_heap());
        assert_eq!(spent, 0, "a payload of IPC_SHORT_MAX bytes must not allocate");

        let long = [7u8; IPC_SHORT_MAX + 1];
        let before = crate::test_alloc::thread_allocs();
        // SAFETY: `long` is a live local of exactly IPC_SHORT_MAX + 1 bytes.
        let q = unsafe { Payload::copy_in_from_user(long.as_ptr(), long.len()) };
        let spent = crate::test_alloc::thread_allocs() - before;
        assert!(q.is_heap());
        assert_eq!(spent, 1, "one byte over the tier must cost exactly ONE allocation");

        // An empty message — the most common shape of all — allocates nothing
        // either, which is what makes `heap_allocs` in the boot marker readable.
        let before = crate::test_alloc::thread_allocs();
        let e = Payload::empty();
        let spent = crate::test_alloc::thread_allocs() - before;
        assert!(e.is_empty() && spent == 0);
    }

    #[test]
    fn the_inline_tier_costs_what_the_ledger_says_it_costs() {
        // `Message` carries this by value and is moved on every send, push, pop
        // and error return, so the footprint is a number someone has to agree
        // to, not an accident of a later edit. Measured price of the tier on
        // the running system: +10 592 bytes of steady kernel heap (+0.54 %),
        // for 3996 allocate/free pairs removed in the same window.
        assert_eq!(core::mem::size_of::<Payload>(), IPC_SHORT_MAX + 8);
        assert!(
            core::mem::size_of::<Payload>() < IPC_SHORT_MAX + core::mem::size_of::<Vec<u8>>(),
            "the enum must not pay for both tiers at once"
        );
    }

    #[test]
    fn the_bounds_are_the_ones_the_contract_names() {
        // RFC-0096 §Amendment 2026-09-16 (measured in TASK-0054C P3a) and the
        // mirror in `nexus-abi`, which `scripts/check-ipc-bounds.sh` compares.
        assert_eq!(IPC_SHORT_MAX, 32);
        assert_eq!(IPC_PAYLOAD_MAX, 8 * 1024);
        assert!(IPC_SHORT_MAX < IPC_PAYLOAD_MAX);
        assert!(IPC_SHORT_MAX <= u8::MAX as usize, "Inline::len is a u8");
    }
}
