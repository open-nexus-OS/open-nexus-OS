// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel console ring a service receives from init (RFC-0107 Phase 2): a `VmoRo`
//! of the kernel's ring pages (`INIT_CONSOLE_RING_SLOT` in init), pinned into the block owner's
//! declared `ConsoleRing` slot — the one reader. This maps it once, checks its header, and
//! reads it as a `nexus_console_ring::Source`: the head atomically, the data byte by byte
//! (volatile). The reader logic — what is new, what the kernel overwrote — is the crate's.
//! OWNERS: @runtime
//! STATUS: Functional
//! PUBLIC API: ConsoleRing::map(), Source for ConsoleRing; re-exports Reader, Batch, Source
//! TEST_COVERAGE: the reader in nexus-console-ring (host); every QEMU lane's trace contract

pub use nexus_console_ring::{Batch, Reader, Source};

/// The kernel's console ring, mapped read-only.
pub struct ConsoleRing {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    base: usize,
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
impl ConsoleRing {
    /// Maps the ring behind `slot` read-only. `None` = no ring in the slot, or pages that are not
    /// this ring (their size or their header). Each call maps anew; callers keep the value.
    pub fn map(slot: u32) -> Option<Self> {
        use nexus_console_ring::{header_ok, BYTES, PAGE};
        let mut info = crate::CapQuery::default();
        crate::cap_query(slot, &mut info).ok()?;
        if usize::try_from(info.len).ok()? != BYTES {
            return None;
        }
        let flags = crate::page_flags::VALID | crate::page_flags::READ | crate::page_flags::USER;
        let va = crate::vm_map(slot, 0, BYTES, flags).ok()?;
        // SAFETY: `BYTES` bytes are mapped read-only at `va` for the rest of the program (never
        // unmapped); the header is the first page of them.
        let header = unsafe { core::slice::from_raw_parts(va as *const u8, PAGE) };
        header_ok(header).then_some(Self { base: va })
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
impl ConsoleRing {
    /// Host builds carry no ring.
    pub fn map(_slot: u32) -> Option<Self> {
        None
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
impl Source for ConsoleRing {
    fn head(&self) -> u64 {
        use core::sync::atomic::{AtomicU64, Ordering};
        // SAFETY: the head lies 8-aligned inside the mapped header page; the kernel only ever
        // writes it atomically, and a load never writes the read-only page.
        let head = unsafe { &*((self.base + nexus_console_ring::OFF_HEAD) as *const AtomicU64) };
        head.load(Ordering::Acquire)
    }

    fn copy(&self, at: usize, out: &mut [u8]) {
        use nexus_console_ring::{DATA_AT, DATA_BYTES};
        let at = at.min(DATA_BYTES);
        let n = out.len().min(DATA_BYTES - at);
        for (i, byte) in out[..n].iter_mut().enumerate() {
            // SAFETY: `at + i < DATA_BYTES`, inside the mapped data pages.
            *byte = unsafe { ((self.base + DATA_AT + at + i) as *const u8).read_volatile() };
        }
    }
}
