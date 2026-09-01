// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Measured-boot handoff consumption (ADR-0059 ABI v1; TASK-0289
//! A3). `nxboot` writes one CRC'd record page at a fixed address ABOVE
//! every kernel-managed range before jumping here; this module captures it
//! ONCE, pre-SATP (the page is deliberately unmapped afterwards — nothing
//! in the kernel can touch it again), and keeps a validated copy for the
//! userland surface (bootctld, TASK-0289 B1). The record carries
//! MEASUREMENT (what booted, qemu-soft-root), never policy. Layout parity
//! with `userspace/bootfmt/src/handoff.rs` is contractual (ADR-0059); the
//! `KSELFTEST: boot handoff ok (measured)` rung in the QEMU ladder is the
//! cross-implementation proof — drift breaks it loudly.
//! OWNERS: @kernel-team @security
//! STATUS: Experimental (TASK-0289 Phase A)
//! PUBLIC API: capture_early(), emit_marker(), get()
//! TEST_COVERAGE: QEMU marker ladder (loader writes, kernel validates)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

/// Fixed page address (ADR-0059, frozen after the TASK-0289-A memory-map
/// audit). Parity const: `bootfmt::handoff::ADDR`.
pub const HANDOFF_ADDR: usize = 0x9300_0000;
const MAGIC: &[u8; 8] = b"NXHO0001";
const VERSION: u16 = 1;
/// CRC32 covers bytes [0..56); the CRC itself sits at [56..60).
/// Public: the measured-surface syscall (TASK-0289 B1) copies exactly
/// this many bytes out, and userspace decodes them with `bootfmt`.
pub const RECORD_LEN: usize = 60;
const CRC_OFF: usize = 56;

// ADR-0059 assertion: the page lies outside EVERY kernel-managed range —
// above the user VMO arena (which ends below it) and inside guest RAM
// (0x9400_0000 = the contracted `qemu-launcher.sh -m 320M` end) — so it
// can never be recycled into a user VMO or clobbered by the kernel.
const _: () = assert!(
    HANDOFF_ADDR >= crate::mm::USER_VMO_ARENA_BASE + crate::mm::USER_VMO_ARENA_LEN,
    "handoff page must sit above the user VMO arena (ADR-0059)"
);
const _: () = assert!(HANDOFF_ADDR + 4096 <= 0x9400_0000, "handoff page must lie inside guest RAM");

/// Validated measured-boot record (field-for-field the ADR-0059 v1 page).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootHandoff {
    /// 0 = slot a, 1 = slot b.
    pub boot_slot: u8,
    /// Whether the loader consumed a trial try this boot.
    pub tries_decremented: bool,
    /// `rollback_index` of the booted NXBD.
    pub rollback_index: u32,
    /// Streamed sha256 of the booted image payload.
    pub image_sha256: [u8; 32],
    /// BSB `seq` the selection was read under.
    pub bsb_seq: u64,
}

enum Capture {
    /// No magic at the page — an honest direct-kernel dev boot.
    Absent,
    /// Magic present but CRC/version/fields invalid — corrupt, never
    /// surfaced as a measured claim.
    Invalid,
    /// Validated record + the raw page bytes it decoded from (kept
    /// verbatim so the userland surface re-decodes with the SAME
    /// host-tested `bootfmt` codec instead of a third parity twin).
    Present(BootHandoff, [u8; RECORD_LEN]),
}

static mut CAPTURED: Capture = Capture::Absent;

/// Captures the record. MUST run on the boot hart BEFORE the kernel
/// address space is activated (`KernelState::new`) — the page is bare-mode
/// reachable only; after SATP it is deliberately unmapped.
///
/// # Safety
/// Single-threaded early boot only (no other hart runs kernel code yet);
/// reads a fixed physical page that only the loader ever writes.
pub unsafe fn capture_early() {
    let mut raw = [0u8; RECORD_LEN];
    for (i, slot) in raw.iter_mut().enumerate() {
        *slot = unsafe { core::ptr::read_volatile((HANDOFF_ADDR + i) as *const u8) };
    }
    let capture = decode(&raw);
    unsafe { core::ptr::addr_of_mut!(CAPTURED).write(capture) };
}

fn decode(raw: &[u8; RECORD_LEN]) -> Capture {
    if &raw[0..8] != MAGIC {
        return Capture::Absent;
    }
    let stored = u32::from_le_bytes([raw[56], raw[57], raw[58], raw[59]]);
    if crc32_ieee(&raw[..CRC_OFF]) != stored {
        return Capture::Invalid;
    }
    if u16::from_le_bytes([raw[8], raw[9]]) != VERSION || raw[10] > 1 || raw[11] > 1 {
        return Capture::Invalid;
    }
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&raw[16..48]);
    let mut seq = [0u8; 8];
    seq.copy_from_slice(&raw[48..56]);
    Capture::Present(
        BootHandoff {
            boot_slot: raw[10],
            tries_decremented: raw[11] == 1,
            rollback_index: u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]),
            image_sha256: sha,
            bsb_seq: u64::from_le_bytes(seq),
        },
        *raw,
    )
}

/// Read access for the userland surface (TASK-0289 B1: bootctld exposure).
pub fn get() -> Option<BootHandoff> {
    match unsafe { &*core::ptr::addr_of!(CAPTURED) } {
        Capture::Present(record, _) => Some(*record),
        _ => None,
    }
}

/// The validated raw record bytes for `SYSCALL_BOOT_HANDOFF` (TASK-0289
/// B1). `None` for absent AND invalid captures — a corrupt page is never
/// surfaced as measurement.
pub fn raw() -> Option<[u8; RECORD_LEN]> {
    match unsafe { &*core::ptr::addr_of!(CAPTURED) } {
        Capture::Present(_, raw) => Some(*raw),
        _ => None,
    }
}

/// Emits the honest boot-chain marker (call once, after logging is up).
/// Present ⇒ measured rung; absent ⇒ named direct-kernel dev boot; a
/// corrupt record is named too and NEVER surfaced as measured.
pub fn emit_marker() {
    match unsafe { &*core::ptr::addr_of!(CAPTURED) } {
        Capture::Present(record, _) => {
            log_info!(target: "selftest", "KSELFTEST: boot handoff ok (measured)");
            log_info!(
                target: "boot",
                "neuron: boot handoff slot={} rbidx={} seq={} (qemu-soft-root)",
                if record.boot_slot == 0 { "a" } else { "b" },
                record.rollback_index,
                record.bsb_seq
            );
        }
        Capture::Invalid => {
            log_info!(target: "boot", "neuron: boot handoff invalid (crc) - treated as absent");
        }
        Capture::Absent => {
            log_info!(target: "boot", "neuron: boot handoff absent (direct kernel)");
        }
    }
}

/// crc32 (IEEE, table-free) — ABI parity with `bootfmt::crc32_ieee`.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
