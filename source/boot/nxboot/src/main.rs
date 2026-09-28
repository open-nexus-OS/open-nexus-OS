// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot binary shell (TASK-0289 Phase A). On the bare-metal
//! riscv target this is the complete first-stage loader: tree + console +
//! kernel window (RFC-0098) → the boot disk (virtio or SDHCI, `probe`) →
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
mod platform;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod probe;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod virtio;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
mod boot {
    extern crate alloc;

    use alloc::format;
    use alloc::string::String;

    use bootfmt::bsb::Slot;
    use nxboot::flow::{self, Event, FlowError, Reason};
    use storage::trace::{LoaderTrace, OsTrace, TraceError};

    use crate::arch;
    use crate::probe::BootDisk;

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

    /// The boot trace (RFC-0107): the loader's console text in its slot of the boot disk's
    /// `trace` partition, rewritten at each milestone — so a board boot can be read without a
    /// serial adapter. Diagnostics only: a trace that cannot be kept never stops the boot.
    struct Trace(Option<LoaderTrace>);

    impl Trace {
        fn open(disk: &BootDisk) -> Self {
            match LoaderTrace::open(disk) {
                Ok(t) => {
                    arch::uart_puts(&format!("nxboot: trace slot={} seq={}\n", t.slot(), t.seq()));
                    Self(Some(t))
                }
                Err(e) => {
                    let why = match e {
                        TraceError::NoPartition => "no partition",
                        TraceError::TooSmall => "too small",
                        TraceError::Io => "io",
                        TraceError::NotThisBoot => "not this boot",
                    };
                    arch::uart_puts(&format!("nxboot: trace none ({why})\n"));
                    Self(None)
                }
            }
        }

        /// Everything printed so far into the slot; `complete` on the loader's last write.
        fn keep(&mut self, disk: &mut BootDisk, complete: bool) {
            let Some(t) = self.0.as_mut() else { return };
            let (text, lost) = arch::captured();
            if t.write(disk, text, lost, complete).is_err() {
                self.0 = None;
                arch::uart_puts("nxboot: trace FAIL (write)\n");
            }
        }

        /// `/chosen/nexus,trace`: the slot's first LBA and the boot's sequence number.
        fn handoff(&self) -> Option<String> {
            self.0.as_ref().map(|t| format!("{} {}", t.slot_lba(), t.seq()))
        }
    }

    /// RFC-0107 Phase 3: the previous boot's console ring, if the DRAM kept it across the reset —
    /// found by its layout in the kernel window this boot's image is about to overwrite. What the
    /// block owner never wrote (its tail, or all of it when the kernel stopped before the owner
    /// ran) goes into that boot's OS region, marked rescued. Diagnostics only: nothing here stops
    /// the boot, and a window without the ring is the measurement that the DRAM was scrubbed.
    fn rescue(trace: &Trace, disk: &mut BootDisk, base: usize, len: usize) {
        use nexus_console_ring::{find_ring, Reader, RingBytes, BYTES};
        let Some(t) = trace.0.as_ref() else { return };
        let Some((lba, seq)) = t.previous() else {
            arch::uart_puts("nxboot: rescue none (first boot)\n");
            return;
        };
        let window = arch::phys_slice(base, len);
        // Every kernel start zeroes its `.bss`, so a ring in the window is the most recent
        // kernel run's — the previous boot's when it stamped the number, and when it did not
        // (it stopped before reading the tree) still that run's, as long as nothing was kept
        // for that boot yet.
        let Some((at, stamped)) = find_ring(window) else {
            arch::uart_puts("nxboot: rescue none (no ring in ram)\n");
            return;
        };
        if stamped != seq && stamped != 0 {
            arch::uart_puts(&format!(
                "nxboot: rescue none (ring of seq={stamped} in ram, not seq={seq})\n"
            ));
            return;
        }
        let mut os = match OsTrace::open(disk, t.partition(), lba, seq) {
            Ok(os) => os,
            Err(_) => {
                arch::uart_puts(&format!(
                    "nxboot: rescue none (slot of seq={seq} not its record)\n"
                ));
                return;
            }
        };
        if stamped == 0 && !os.is_empty() {
            arch::uart_puts(&format!("nxboot: rescue none (unstamped ring, seq={seq} has text)\n"));
            return;
        }
        let ring = RingBytes(&window[at..at + BYTES]);
        let mut reader = Reader::from(os.len() as u64);
        let mut buf = [0u8; 4096];
        let (mut kept, mut lost) = (0usize, 0u64);
        loop {
            let batch = reader.read(&ring, &mut buf);
            if batch.len == 0 && batch.lost == 0 {
                break;
            }
            lost += batch.lost;
            match os.rescue(disk, &buf[..batch.len], batch.lost > 0) {
                Ok(n) if n == batch.len && batch.len == buf.len() => kept += n,
                Ok(n) => {
                    kept += n;
                    break;
                }
                Err(_) => {
                    arch::uart_puts("nxboot: rescue FAIL (write)\n");
                    return;
                }
            }
        }
        let how = if stamped == 0 { " unstamped" } else { "" };
        arch::uart_puts(&format!("nxboot: rescue ok (seq={seq}{how} bytes={kept} lost={lost})\n"));
    }

    /// The DRAM-retention probe (RFC-0107 Phase 3's measurement, independent of the kernel): the
    /// last page of the kernel window carries this boot's sequence number; the next loader reads
    /// it before it loads its image. `kept` = the DRAM keeps its content across the board's
    /// reset path (so a ring that is missing was never written — the kernel stopped before its
    /// first lines); `lost` = the reset scrubs it (so the ring cannot be rescued on this board).
    /// A page the kernel may well reuse, so `stale` or `lost` after a long run proves nothing;
    /// after a boot that kept no OS text it does.
    fn dram_probe(trace: &Trace, base: usize, len: usize) {
        const MAGIC: &[u8; 8] = b"NXDRAM1\0";
        let Some(t) = trace.0.as_ref() else { return };
        let page = arch::load_region(base + len - 4096, 4096);
        let found = (page[..8] == *MAGIC).then(|| {
            u64::from_le_bytes([
                page[8], page[9], page[10], page[11], page[12], page[13], page[14], page[15],
            ])
        });
        match (found, t.previous()) {
            (Some(seq), Some((_, prev))) if seq == prev => {
                arch::uart_puts(&format!("nxboot: dram probe kept (seq={seq})\n"));
            }
            (Some(seq), _) => arch::uart_puts(&format!("nxboot: dram probe stale (seq={seq})\n")),
            (None, Some(_)) => arch::uart_puts("nxboot: dram probe lost\n"),
            (None, None) => arch::uart_puts("nxboot: dram probe none (first boot)\n"),
        }
        page[..8].copy_from_slice(MAGIC);
        page[8..16].copy_from_slice(&t.seq().to_le_bytes());
    }

    /// A terminal failure once the disk is known: the reason on the console and in the trace.
    fn fail(trace: &mut Trace, disk: &mut BootDisk, msg: &str) -> ! {
        arch::uart_puts("nxboot: PANIC (");
        arch::uart_puts(msg);
        arch::uart_puts(")\n");
        trace.keep(disk, true);
        arch::system_reset()
    }

    /// Rust-side entry, reached from the `_start` asm (via the `no_mangle`
    /// export in `arch`) with the firmware registers (a0 = hartid,
    /// a1 = DTB) intact.
    pub fn run(hartid: usize, dtb: usize) -> ! {
        // RFC-0098 C1: the tree first — the console, the memory map and the
        // transports all come from it; nothing below knows an address.
        let tree = crate::platform::init(dtb);
        let Some((base, len)) = crate::platform::kernel_window(&tree) else {
            panic_reset("no kernel window in the first memory bank");
        };
        // RFC-0098 C5 (TASK-0246B): the boot disk is the first candidate — virtio, the SD hosts
        // in the tree, the SD hosts behind PCI — that carries a valid BSB; its record names it.
        let Some((mut disk, record_buf, record_len)) = crate::probe::find(&tree) else {
            panic_reset("no boot disk");
        };
        let boot_disk = core::str::from_utf8(&record_buf[..record_len]).unwrap_or("?");
        // RFC-0107: from here on the boot leaves its console text on the disk.
        let mut trace = Trace::open(&disk);
        trace.keep(&mut disk, false);
        // RFC-0107 Phase 3: before the window is overwritten, what the previous boot left in it.
        dram_probe(&trace, base, len);
        rescue(&trace, &mut disk, base, len);
        let dest = arch::load_region(base, len);
        match flow::run(&mut disk, dest, &mut |e| emit(&e)) {
            Ok(loaded) => {
                let handoff = bootfmt::handoff::Handoff {
                    boot_slot: loaded.slot,
                    tries_decremented: loaded.tries_decremented,
                    rollback_index: loaded.desc.rollback_index,
                    image_sha256: loaded.desc.image_sha256,
                    bsb_seq: loaded.bsb_seq,
                };
                // RFC-0098 C2: the kernel receives OUR copy of the tree, with the
                // loader's decisions in /chosen/nexus,* — the slot, the measured
                // record (ADR-0059 v1 bytes) and, on QEMU, the lane's fw_cfg knobs
                // re-expressed there (the kernel never reads fw_cfg).
                let record = bootfmt::handoff::encode_record(&handoff);
                // RFC-0098 C5: the record of the disk the volume was just read from.
                // RFC-0107: the trace's slot, for the OS writer.
                let handoff_trace = trace.handoff();
                let dtb = crate::platform::prepare_dtb(
                    &tree,
                    slot_ch(loaded.slot),
                    &record,
                    boot_disk,
                    handoff_trace.as_deref(),
                );
                // RFC-0098 C6: the image is position-independent; it runs where
                // this loader put it and fixes itself up there.
                arch::uart_puts(&format!(
                    "nxboot: jump slot={} base=0x{base:x}\n",
                    slot_ch(loaded.slot)
                ));
                trace.keep(&mut disk, true);
                arch::jump_kernel(base as u64, hartid, dtb)
            }
            Err(FlowError::BsbInvalid) => fail(&mut trace, &mut disk, "bsb invalid on both blocks"),
            Err(FlowError::Disk) => fail(&mut trace, &mut disk, "disk io"),
            Err(FlowError::BothSlotsBad { .. }) => fail(&mut trace, &mut disk, "both slots bad"),
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
