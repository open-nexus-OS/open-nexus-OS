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
//! the header once before the first byte.
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
// from a path that must print regardless); readers only ever see the pages read-only, ordered
// by the head's release store.
unsafe impl Sync for Pages {}

static RING: Pages = Pages(UnsafeCell::new([0; BYTES]));
/// Orders the ring with the UART: the byte is recorded and emitted under one hold.
static LOCK: SpinIrqLock<()> = SpinIrqLock::new(());
/// The bound on waiting for the lock: far longer than any hold (one byte on the wire), short
/// enough that a hart stopped while holding it cannot silence the console for good.
const LOCK_TRIES: usize = 1 << 20;

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

/// Records `byte` in the ring, then hands it to `emit` (the UART write), both under the ring's
/// lock: the ring keeps the bytes in the order the console receives them.
pub fn record_then(byte: u8, emit: impl FnOnce(u8)) {
    let mut guard = None;
    for _ in 0..LOCK_TRIES {
        if let Some(held) = LOCK.try_lock() {
            guard = Some(held);
            break;
        }
        core::hint::spin_loop();
    }
    let h = head().load(Ordering::Relaxed);
    let at = DATA_AT + (h % DATA_BYTES as u64) as usize;
    // SAFETY: `at` lies inside the data area of the ring's pages.
    unsafe { base().add(at).write_volatile(byte) };
    head().store(h + 1, Ordering::Release);
    emit(byte);
    drop(guard);
}

/// The ring as init receives it: read-only pages over the image's own frames (the pool never
/// owned them), like the tree's alias (`boot_fdt::init_alias`).
pub fn init_alias() -> Option<crate::cap::Capability> {
    use crate::cap::{Capability, CapabilityKind, Rights};
    let pa = crate::phys::virt_to_phys(base() as usize);
    let id = crate::mm::vmo::adopt_fixed(pa, BYTES).ok()?;
    Some(Capability { kind: CapabilityKind::VmoRo { id, len: BYTES }, rights: Rights::MAP })
}
