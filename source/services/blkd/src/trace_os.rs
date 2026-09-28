// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: The boot trace's OS region, kept by the block owner (RFC-0107 Phase 2, TASK-0327B
//! P2). The kernel keeps every console byte in its ring (read-only here, the one reader: the
//! topology declares the slot for blkd alone); the loader handed this boot's trace slot over in
//! `/chosen/nexus,trace`. blkd — the one process that holds the disk, so the first that can
//! write it, and a diagnostics channel must depend on as little as possible — appends what the
//! ring holds to that slot's OS region, paced by a periodic kernel timer (RFC-0093 §7): the
//! trace lags the console by at most one period, also while no client asks for anything. The
//! slot must already be this boot's record in the `trace` partition (name AND type) at the place
//! the ring rule gives its number; the partition has no selector, so no client can name it.
//! Nothing is allocated per tick (the bump heap never frees): one chunk buffer, reused.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: storage::trace (the writer, host), nexus-console-ring (the reader, host); every
//!   QEMU lane's trace contract compares the kept text with the UART log byte for byte

extern crate alloc;

use nexus_abi::console_ring::{ConsoleRing, Reader};
use nexus_ipc::timer::NotifyTimer;
use nexus_service_topology::slots;
use storage::gpt::Partition;
use storage::trace::{OsTrace, TraceError, CHOSEN_KEY, SLOTS};

use crate::disk_os::Disk;

/// The pacing period: the most the trace lags the console.
const PERIOD_NS: u64 = 250_000_000;
/// One append.
const CHUNK: usize = 4096;
/// The most one tick keeps, so a backlog never holds the clients up for long.
const TICK_BYTES: usize = 64 * 1024;

pub(crate) struct Keeper {
    ring: ConsoleRing,
    reader: Reader,
    trace: OsTrace,
    timer: NotifyTimer,
    buf: alloc::boxed::Box<[u8; CHUNK]>,
    seq: u64,
    announced: bool,
    broken: bool,
}

fn emit(msg: &str) {
    let _ = nexus_abi::debug_println(msg);
}

/// `blkd: trace os none (<why>)` — once, at start: what this boot's OS text is not kept for.
fn none(why: &str) -> Option<Keeper> {
    emit(&alloc::format!("blkd: trace os none ({why})"));
    None
}

/// `"<slot's first LBA> <seq>"`, as the loader wrote it.
fn handoff() -> Option<(u64, u64)> {
    let bytes = nexus_abi::device_tree::map_read_only(slots::blkd::DEVICE_TREE)?;
    let tree = nexus_fdt::Fdt::new(bytes).ok()?;
    let value = tree.chosen().ok()?.nexus_str(CHOSEN_KEY)?;
    let mut fields = value.split_ascii_whitespace();
    let lba = fields.next()?.parse().ok()?;
    let seq = fields.next()?.parse().ok()?;
    fields.next().is_none().then_some((lba, seq))
}

impl Keeper {
    /// Every precondition, or one line saying which is missing.
    pub(crate) fn open(dev: &Disk, part: Option<&Partition>) -> Option<Self> {
        let Some(ring) = ConsoleRing::map(slots::blkd::CONSOLE_RING) else {
            return none("no-ring");
        };
        let Some((lba, seq)) = handoff() else {
            return none("no-handoff");
        };
        let Some(part) = part else {
            return none("no-partition");
        };
        let trace = match OsTrace::open(dev, part, lba, seq) {
            Ok(trace) => trace,
            Err(TraceError::Io) => return none("io"),
            Err(_) => return none("not-this-boot"),
        };
        let Ok(mut timer) = NotifyTimer::bind_with_interval(slots::blkd::TRACE_TIMER, PERIOD_NS)
        else {
            return none("no-timer");
        };
        timer.arm_in(PERIOD_NS);
        Some(Self {
            ring,
            reader: Reader::new(),
            trace,
            timer,
            buf: alloc::boxed::Box::new([0u8; CHUNK]),
            seq,
            announced: false,
            broken: false,
        })
    }

    /// The timer's RECV half: a waitset member beside the server endpoint.
    pub(crate) fn wake_slot(&self) -> u32 {
        self.timer.recv_slot()
    }

    /// On every wake of the service loop: when the timer fired, keep what the ring holds.
    pub(crate) fn tick(&mut self, dev: &mut Disk) {
        if !self.timer.drain() || self.broken {
            return;
        }
        let mut kept = 0;
        while kept < TICK_BYTES {
            let batch = self.reader.read(&self.ring, &mut self.buf[..]);
            if batch.len == 0 && batch.lost == 0 {
                break;
            }
            if self.trace.append(dev, &self.buf[..batch.len], batch.lost > 0).is_err() {
                self.broken = true;
                emit("blkd: trace os FAIL (write)");
                return;
            }
            kept += batch.len;
            if batch.len < CHUNK {
                break;
            }
        }
        if kept > 0 && !self.announced {
            self.announced = true;
            let slot = (self.seq - 1) % u64::from(SLOTS);
            emit(&alloc::format!("blkd: trace os ok (slot={slot} seq={})", self.seq));
        }
    }
}
