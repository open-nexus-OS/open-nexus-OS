// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the lowmem lane's REAL memory pressure (TASK-0333 6f-3, RFC-0100 §5). On the
//! `lowmem` profile alone — and on QEMU alone — the harness stands in for a memory-hungry
//! workload: once an app is frozen (memd's `STATUS` counts one in the suspended band) it takes
//! memory as eager objects (every frame committed at create) until free frames are halfway
//! between the critical and the emergency watermark (`mm_stats` names both) — the kernel's OWN
//! level is `critical` then, with room both ways: the authority's squeeze and swap-out give
//! frames back without the level recovering past its hysteresis, and a burst elsewhere does not
//! reach `emergency` (a kill at once). The authority's chain runs from the kernel's frame, never
//! from a drill: memd squeezes the frozen app, swaps it out, kills it (`reason=oom`) and
//! journals the kill. While it holds, what the squeeze and the swap-out gave back is taken again
//! (once a second); once memd ordered a kill every object goes back and the level recovers:
//! `SELFTEST: mem pressure held (level= free= taken=)`, then `SELFTEST: mem pressure
//! released (level=normal free=)`; a step that does not run its course says `SELFTEST: mem
//! pressure hold FAIL (step=…)`. The harness is a service: memd's table holds the lifecycle
//! authority's app processes alone, so the pressure never makes the harness a victim. Polled
//! once a second from the end phase's idle loop, in place of the drill on this lane.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder (the lowmem lane: held, the kernel-sourced squeeze, swap-out
//!   and kill, the journal, released)
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

use nexus_abi::syscall::memory::{
    mem_pressure_name, MmStats, MEM_PRESSURE_CRITICAL, MEM_PRESSURE_NORMAL,
};

use crate::markers::{
    M_SELFTEST_MEM_PRESSURE_HELD, M_SELFTEST_MEM_PRESSURE_HOLD_FAIL,
    M_SELFTEST_MEM_PRESSURE_RELEASED,
};
use crate::os_lite::memd::{Line, Status};

/// One object's frames at most (8 MiB); the last one is cut to the frames still to take.
const CHUNK_FRAMES: u64 = 2_048;
/// Objects held at most: 48 × 8 MiB covers a 320 MiB pool with room to spare.
const MAX_CHUNKS: usize = 48;
/// Polls (one a second) the pressure is held without a kill before it is let go.
const HOLD_POLLS_MAX: u32 = 120;
/// Polls the level gets to recover after the objects went back.
const RELEASE_POLLS_MAX: u32 = 10;
const PAGE_BYTES: u64 = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Waiting for a frozen app.
    Armed,
    /// The level is critical under the harness's memory; waiting for memd's kill.
    Held,
    /// The memory went back; waiting for the level to recover.
    Released,
    Done,
}

/// The pressure's state, ticked with memd's status once a second.
pub(crate) struct Hold {
    step: Step,
    chunks: [nexus_abi::Handle; MAX_CHUNKS],
    held: usize,
    /// Frames taken over the hold (top-ups included).
    taken: u64,
    polls: u32,
    /// memd's kill count when the hold began: the next kill is the one this pressure caused.
    kills_before: u16,
}

impl Hold {
    pub(crate) const fn new() -> Self {
        Self {
            step: Step::Armed,
            chunks: [0; MAX_CHUNKS],
            held: 0,
            taken: 0,
            polls: 0,
            kills_before: 0,
        }
    }

    pub(crate) fn done(&self) -> bool {
        self.step == Step::Done
    }

    /// One poll: take at the first frozen app, let go at the next kill memd ordered.
    pub(crate) fn tick(&mut self, s: &Status) {
        match self.step {
            Step::Armed if s.suspended >= 1 => {
                self.kills_before = s.kills;
                match self.take() {
                    Some(stats) if at_least_critical(&stats) => {
                        self.say(M_SELFTEST_MEM_PRESSURE_HELD, None, &stats);
                        self.step = Step::Held;
                    }
                    stats => self.give_up(b"take", stats),
                }
            }
            Step::Held => {
                self.polls += 1;
                if s.kills > self.kills_before {
                    self.release();
                    self.polls = 0;
                    self.step = Step::Released;
                } else if self.polls > HOLD_POLLS_MAX {
                    self.give_up(b"kill", nexus_abi::mm_stats().ok());
                } else {
                    // What the authority's squeeze and swap-out gave back is taken again.
                    let _ = self.take();
                }
            }
            Step::Released => {
                self.polls += 1;
                match nexus_abi::mm_stats().ok() {
                    Some(stats) if stats.pressure_level == u64::from(MEM_PRESSURE_NORMAL) => {
                        self.say(M_SELFTEST_MEM_PRESSURE_RELEASED, None, &stats);
                        self.step = Step::Done;
                    }
                    stats if self.polls > RELEASE_POLLS_MAX => {
                        self.give_up(b"release", stats);
                    }
                    _ => {}
                }
            }
            Step::Armed | Step::Done => {}
        }
    }

    /// Takes memory until free frames are down to halfway between the critical and the
    /// emergency watermark (or the objects or the pool ran out); answers the pool as last read.
    /// Bounded: each pass holds one more object.
    fn take(&mut self) -> Option<MmStats> {
        loop {
            let stats = nexus_abi::mm_stats().ok()?;
            let target = (stats.watermark_critical + stats.watermark_emergency) / 2;
            let frames = stats.free.saturating_sub(target).min(CHUNK_FRAMES);
            if frames == 0 || self.held == MAX_CHUNKS {
                return Some(stats);
            }
            let Ok(handle) = nexus_abi::vmo_create((frames * PAGE_BYTES) as usize) else {
                return Some(stats);
            };
            self.chunks[self.held] = handle;
            self.held += 1;
            self.taken += frames;
        }
    }

    /// Every object back to the pool.
    fn release(&mut self) {
        for handle in self.chunks[..self.held].iter().copied() {
            let _ = nexus_abi::vmo_destroy(handle);
        }
        self.held = 0;
    }

    fn give_up(&mut self, step: &[u8], stats: Option<MmStats>) {
        self.release();
        match stats {
            Some(stats) => self.say(M_SELFTEST_MEM_PRESSURE_HOLD_FAIL, Some(step), &stats),
            None => {
                let mut line = Line::new();
                line.put(M_SELFTEST_MEM_PRESSURE_HOLD_FAIL.as_bytes());
                line.put(b"step=");
                line.put(step);
                line.put(b" stats=unanswered)\n");
                line.emit();
            }
        }
        self.step = Step::Done;
    }

    /// `<marker>[step= ]level= free= taken=)` as ONE line.
    fn say(&self, marker: &str, step: Option<&[u8]>, stats: &MmStats) {
        let mut line = Line::new();
        line.put(marker.as_bytes());
        if let Some(step) = step {
            line.put(b"step=");
            line.put(step);
            line.put(b" ");
        }
        let level = stats.pressure_level.min(u64::from(u8::MAX)) as u8;
        line.put(b"level=");
        line.put(mem_pressure_name(level).as_bytes());
        line.put(b" free=");
        line.put_u64(stats.free);
        line.put(b" taken=");
        line.put_u64(self.taken);
        line.put(b")\n");
        line.emit();
    }
}

fn at_least_critical(stats: &MmStats) -> bool {
    stats.pressure_level >= u64::from(MEM_PRESSURE_CRITICAL)
}
