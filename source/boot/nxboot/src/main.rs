// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot binary shell (TASK-0289 Phase A). On the bare-metal
//! riscv target this is the complete first-stage loader: virtio probe →
//! `flow::run` (BSB → select/actuate → GPT → NXBD verify → digest →
//! fallback) → measured handoff page → icache-fenced jump. Every terminal
//! failure prints a deterministic `nxboot: PANIC (...)` marker and SBI-
//! resets (wait-loop doctrine — never hang, never boot unverified bytes).
//! On host targets it is an inert stub so the workspace builds/lints
//! uniformly; the machine logic is host-tested through the library half.
//! NOTE: nothing boots through this binary until the A4 flip — running it
//! today is a harness misconfiguration and fails loudly.
//! OWNERS: @security @runtime
//! STATUS: Experimental
//! TEST_COVERAGE: lib + tests/loader_flow.rs; QEMU proof arrives at A4
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

#![cfg_attr(all(target_arch = "riscv64", target_os = "none"), no_std, no_main)]
#![cfg_attr(all(target_arch = "riscv64", target_os = "none"), feature(alloc_error_handler))]
// The single unsafe allowance lives in `arch` (ADR-0059: one bounded
// early-asm/MMIO module); everything else denies unsafe.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod arch;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod virtio;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod boot {
    extern crate alloc;

    use alloc::format;
    use alloc::string::String;

    use bootfmt::bsb::Slot;
    use nxboot::flow::{self, Event, FlowError, Reason};

    use crate::arch;
    use crate::virtio::VirtioDisk;

    fn slot_ch(slot: Slot) -> char {
        match slot {
            Slot::A => 'a',
            Slot::B => 'b',
        }
    }

    fn reason_str(reason: Reason) -> String {
        match reason {
            Reason::Nxbd => String::from("nxbd"),
            Reason::Sig => String::from("sig"),
            Reason::Digest => String::from("digest"),
            Reason::Rollback { have, min } => format!("rollback {have} < min {min}"),
            Reason::Io => String::from("io"),
        }
    }

    fn emit(event: &Event) {
        let line = match *event {
            Event::BsbOk { slot, seq } => {
                format!("nxboot: bsb ok (slot={} seq={seq})\n", slot_ch(slot))
            }
            Event::Tries { slot, from, to } => {
                format!("nxboot: tries {from}->{to} (slot={} trial)\n", slot_ch(slot))
            }
            Event::Exhausted { attempted, to } => format!(
                "nxboot: fallback (slot={} exhausted) -> slot={}\n",
                slot_ch(attempted),
                slot_ch(to)
            ),
            Event::VerifyOk { slot, desc } => {
                let build = desc.build_id_str();
                let id8 = &build[..build.len().min(8)];
                format!(
                    "nxboot: verify ok (slot={} build={id8} rbidx={})\n",
                    slot_ch(slot),
                    desc.rollback_index
                )
            }
            Event::VerifyFail { slot, reason } => {
                format!("nxboot: verify FAIL (slot={} {})\n", slot_ch(slot), reason_str(reason))
            }
            Event::VerifyFallback { to } => {
                format!("nxboot: fallback -> slot={}\n", slot_ch(to))
            }
        };
        arch::uart_puts(&line);
    }

    fn panic_reset(msg: &str) -> ! {
        arch::uart_puts("nxboot: PANIC (");
        arch::uart_puts(msg);
        arch::uart_puts(")\n");
        arch::system_reset()
    }

    /// Rust-side entry, reached from the `_start` asm (via the `no_mangle`
    /// export in `arch`) with the firmware registers (a0 = hartid,
    /// a1 = DTB) intact.
    pub fn run(hartid: usize, dtb: usize) -> ! {
        let Some(mut disk) = VirtioDisk::probe() else {
            panic_reset("no virtio-blk transport");
        };
        let dest = arch::load_region();
        match flow::run(&mut disk, dest, &mut |e| emit(&e)) {
            Ok(loaded) => {
                let handoff = bootfmt::handoff::Handoff {
                    boot_slot: loaded.slot,
                    tries_decremented: loaded.tries_decremented,
                    rollback_index: loaded.desc.rollback_index,
                    image_sha256: loaded.desc.image_sha256,
                    bsb_seq: loaded.bsb_seq,
                };
                arch::write_handoff(&bootfmt::handoff::encode_page(&handoff));
                arch::uart_puts(&format!("nxboot: jump slot={}\n", slot_ch(loaded.slot)));
                arch::jump_kernel(loaded.desc.load_addr, hartid, dtb)
            }
            Err(FlowError::BsbInvalid) => panic_reset("bsb invalid on both blocks"),
            Err(FlowError::Disk) => panic_reset("disk io"),
            Err(FlowError::BothSlotsBad { .. }) => panic_reset("both slots bad"),
        }
    }

    #[panic_handler]
    fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
        arch::uart_puts("nxboot: PANIC (rust panic)\n");
        arch::system_reset()
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn main() {
    // Host stub: the loader only exists as a bare-metal artifact.
}
