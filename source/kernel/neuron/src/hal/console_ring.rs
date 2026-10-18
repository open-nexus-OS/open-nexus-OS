// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The console ring (RFC-0107 Phase 2, TASK-0327B P2): every byte the kernel sends to
//! the console, kept in a fixed ring of whole pages in exactly the order the UART receives it.
//! The kernel's own lines and every service's reach the console through ONE hook,
//! `platform::console_write_byte`, and so does a byte written before a console is known — kept
//! here, dropped on the wire. The layout is `nexus-console-ring`'s. Init receives the ring
//! read-only (`nexus_abi::INIT_CONSOLE_RING_SLOT`) and pins it to the block owner alone, which
//! keeps it in the boot trace on the boot disk. Static and allocation-free (ADR-0040 as amended
//! by RFC-0107): the pages are part of the image's `.bss`, zeroed by the boot; `init` writes
//! the header once before the first byte. Since TASK-0330 S2b the serial line reads the ring
//! (`hal::console_serial`): once the runtime runs, a write records its byte and leaves the
//! sending to the line, which follows the head from a cursor of its own — no writer waits for
//! the transmitter.
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the layout is RFC-0107's contract)
//! TEST_COVERAGE: the format and the reader in nexus-console-ring (host); every QEMU lane's trace
//!   contract compares the ring, as the block owner kept it, with the UART log byte for byte

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, Ordering};

use nexus_console_ring::{header_fields, BYTES, DATA_AT, DATA_BYTES, OFF_HEAD};

use crate::sync::spin_irq::SpinIrqLock;

/// The ring's pages: page-aligned and nothing else in them, since they are exposed as pages.
#[repr(C, align(4096))]
struct Pages(UnsafeCell<[u8; BYTES]>);

// SAFETY: every write happens under `LOCK` (or, when the lock cannot be had within its bound,
// from a path that must print regardless); readers — the serial line among them — only ever
// read, ordered by the head's release store.
unsafe impl Sync for Pages {}

static RING: Pages = Pages(UnsafeCell::new([0; BYTES]));
/// Orders the writers: a byte is recorded — and, before the serial line reads the ring, written
/// to the transmitter — under one hold.
static LOCK: SpinIrqLock<()> = SpinIrqLock::new(());
/// The bound on waiting for the lock: far longer than any hold (one byte on the wire), short
/// enough that a hart stopped while holding it cannot silence the console for good.
const LOCK_TRIES: usize = 1 << 20;

/// The ring's data bytes: the farthest the serial line can fall behind the writers.
pub const CAPACITY: u64 = DATA_BYTES as u64;

fn base() -> *mut u8 {
    RING.0.get().cast::<u8>()
}

fn head() -> &'static AtomicU64 {
    // SAFETY: `OFF_HEAD` is 8-aligned inside a page-aligned static that lives forever; the
    // head is only ever accessed atomically.
    unsafe { &*(base().add(OFF_HEAD) as *const AtomicU64) }
}

/// Writes the header — once, after the boot zeroed `.bss` and before the first console byte.
pub fn init() {
    for (at, field) in header_fields() {
        for (i, b) in field.iter().enumerate() {
            // SAFETY: `at + i` lies inside the header page.
            unsafe { base().add(at + i).write_volatile(*b) };
        }
    }
}

/// Stamps the boot's trace sequence number into the header (RFC-0107 Phase 3): the next loader
/// matches a ring it finds in RAM to the boot it belongs to by this. Once, after the tree is read.
pub fn stamp(seq: u64) {
    // SAFETY: `OFF_SEQ` is 8-aligned inside the header page of a static that lives forever;
    // the field is only ever accessed atomically.
    let field = unsafe { &*(base().add(nexus_console_ring::OFF_SEQ) as *const AtomicU64) };
    field.store(seq, Ordering::Release);
}

/// The ring's lock, waited for up to the bound; `None` when a hart holds it past that.
fn lock_bounded() -> Option<crate::sync::spin_irq::SpinIrqGuard<'static, ()>> {
    for _ in 0..LOCK_TRIES {
        if let Some(held) = LOCK.try_lock() {
            return Some(held);
        }
        core::hint::spin_loop();
    }
    None
}

/// Bytes ever written: the count of the next one.
pub fn written() -> u64 {
    head().load(Ordering::Acquire)
}

/// The byte the ring holds for `count` — the writers' byte while they have not lapped it, which
/// the reader's cursor rule decides (`crate::console_tx`).
pub fn byte_at(count: u64) -> u8 {
    let at = DATA_AT + (count % CAPACITY) as usize;
    // SAFETY: `at` lies inside the data area of the ring's pages; a byte is plain data.
    unsafe { base().add(at).read_volatile() }
}

/// Runs `f` under the ring's lock (bounded as a write is): no byte is recorded meanwhile — the
/// serial line's switch to reading the ring happens between two bytes.
pub fn with_lock<R>(f: impl FnOnce() -> R) -> R {
    let guard = lock_bounded();
    let r = f();
    drop(guard);
    r
}

/// Records `byte` in the ring, then hands it to `emit` under the ring's lock and returns what
/// `emit` says: before the serial line reads the ring (and in a panic) `emit` writes the byte to
/// the transmitter, so the ring keeps the bytes in the order the UART receives them; once the
/// line reads the ring, `emit` only says the line is to be kicked, which the caller does after
/// the lock.
pub fn record_then<R>(byte: u8, emit: impl FnOnce(u8) -> R) -> R {
    let guard = lock_bounded();
    let h = head().load(Ordering::Relaxed);
    let at = DATA_AT + (h % DATA_BYTES as u64) as usize;
    // SAFETY: `at` lies inside the data area of the ring's pages.
    unsafe { base().add(at).write_volatile(byte) };
    head().store(h + 1, Ordering::Release);
    // RFC-0107 on a real cache (TASK-0260B P3): the ring lives in write-back memory and the
    // board's reset drops dirty lines, so the next loader's rescue lost the tail — a kernel
    // read as "stopped" lines before it did. Every byte and the header are written back at
    // once (Zicbom `cbo.flush`, the block size from the tree; nothing without Zicbom, where
    // the ring is what it was): the rescue then ends at the last byte the kernel wrote, a
    // half line included — cheap next to the UART's own time per byte.
    let block = super::platform::cbom_block();
    if block != 0 {
        flush_block(at, block);
        flush_block(OFF_HEAD, block);
    }
    let said = emit(byte);
    drop(guard);
    said
}

/// Writes the cache block holding ring offset `at` back to memory.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn flush_block(at: usize, block: usize) {
    let addr = (base() as usize + at) & !(block - 1);
    // SAFETY: `cbo.flush` on a block of the ring's own pages; the tree named Zicbom and the
    // firmware enabled it for S-mode (`menvcfg.CBCFE`), as the platform's DMA path relies on.
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option arch, +zicbom",
            "cbo.flush ({0})",
            ".option pop",
            in(reg) addr,
            options(nostack)
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn flush_block(_at: usize, _block: usize) {}

/// The ring as init receives it: read-only pages over the image's own frames (the pool never
/// owned them), like the tree's alias (`boot_fdt::init_alias`).
pub fn init_alias() -> Option<crate::cap::Capability> {
    use crate::cap::{Capability, CapabilityKind, Rights};
    let pa = crate::phys::virt_to_phys(base() as usize);
    let id = crate::mm::vmo::adopt_fixed(pa, BYTES).ok()?;
    Some(Capability { kind: CapabilityKind::VmoRo { id, len: BYTES }, rights: Rights::MAP })
}
