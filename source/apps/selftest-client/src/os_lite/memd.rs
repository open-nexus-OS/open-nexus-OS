// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: memd probe (RFC-0100 §3, ADR-0086, TASK-0333 6e-2). The harness holds a declared
//! route to the one memory authority and proves two things from the side it can see:
//!   - the SINK: memd answers `STATUS` saying the kernel's pressure endpoint is bound to it
//!     (`sink=1`) — `SELFTEST: memd bound ok (level= bands=)`, on every lane, at the end phase;
//!   - the FAN-OUT (6e-3): on a QEMU lane with a display, once a frozen app has been seen
//!     running again (the lifecycle phase proved the thaw; the app is on screen), the harness
//!     runs the pressure DRILL at `warn` — memd tells gpud and windowd, windowd tells the
//!     running window, app-host raises `MemoryPressure` (`SELFTEST: mem pressure fanout ok`);
//!   - the SQUEEZE (6f-1, ADR-0087): at the FIRST freeze, before any thaw, the drill at
//!     `critical` — memd squeezes the frozen app through the lifecycle authority and the parent
//!     and says `SELFTEST: mem squeeze ok (pages= net= ratio=% reason=drill)`;
//!   - the SWAP (6f-2, ADR-0088): still at the first freeze, the next drill at `critical` — memd,
//!     the pager, seals the squeezed app's records out to the `swap` partition
//!     (`SELFTEST: swap out ok (pages= slots= reason=drill)`); the thaw that follows pages them
//!     back (`SELFTEST: swap in ok (pages=)`) and the app runs on (the lifecycle phase's
//!     `ability lifecycle ok`, the fan-out's `apphost: mem pressure` from the thawed window);
//!   - the KILL (6e-2): once an app is frozen once more (the restore phase hid it), the drill
//!     at `critical` — a simulated level over the real bands; memd squeezes the frozen app first
//!     (one drill), swaps it out (the next), then picks it as the victim, orders the lifecycle
//!     authority,
//!     which orders the parent; the reap comes back as the band `gone` and memd says
//!     `SELFTEST: mem pressure kill ok (band=suspended reason=drill)`. The real level's kill
//!     (`reason=oom`) is the `lowmem` lane's (6f-3).
//! The drill never runs on a board: it ends an app the operator may be using. It cannot run
//! on a lane without apps either — its arming conditions (a frozen app ran again; an app is
//! frozen now) are the visible lane's choreography, which no headless lane produces. Polled
//! once a second in the end phase's idle loop — the harness has no event channel from memd.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder (`SELFTEST: memd bound ok (` every lane; the kill chain
//!   on the usb-visible lane)
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

use nexus_abi::syscall::memory::{MEM_PRESSURE_CRITICAL, MEM_PRESSURE_WARN};
use nexus_wire::memd as wire;

use crate::markers::{
    emit_bytes, emit_line, emit_u64, M_SELFTEST_MEMD_BOUND_FAIL, M_SELFTEST_MEMD_BOUND_OK,
    M_SELFTEST_MEM_PRESSURE_DRILL,
};
use crate::os_lite::boot_cfg::{self, Machine};

/// The authority's state as `STATUS` answers it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Status {
    level: u8,
    sink: bool,
    bands: u8,
    suspended: u8,
    resumed: u16,
}

/// One request on the declared route; the reply rides the harness's CAP_MOVE inbox (a waited
/// exchange: the answer or memd's death — no clock).
fn call(req: &[u8], rsp: &mut [u8]) -> Option<usize> {
    let route = nexus_service_topology::slots::selftest_client::MEMD;
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    nexus_ipc::exchange::call_into(route.send, reply, req, rsp).ok()
}

fn status() -> Option<Status> {
    let mut rsp = [0u8; wire::REPLY_MAX_BYTES];
    let n = call(&wire::encode_status(), &mut rsp)?;
    let (status, level, sink, bands, suspended, resumed, _kills, _free, _total) =
        wire::decode_status_reply(wire::OP_STATUS, &rsp[..n])?;
    (status == wire::STATUS_OK).then_some(Status {
        level,
        sink: sink == 1,
        bands,
        suspended,
        resumed,
    })
}

/// The sink proof: `SELFTEST: memd bound ok (level= bands=)` when memd holds the kernel's
/// pressure endpoint, the FAIL line otherwise (memd absent, or the bind refused).
pub(crate) fn bound_probe() {
    match status() {
        Some(s) if s.sink => {
            emit_bytes(M_SELFTEST_MEMD_BOUND_OK.as_bytes());
            emit_bytes(b"level=");
            emit_bytes(nexus_abi::syscall::memory::mem_pressure_name(s.level).as_bytes());
            emit_bytes(b" bands=");
            emit_u64(u64::from(s.bands));
            emit_bytes(b")\n");
        }
        Some(_) => emit_line(M_SELFTEST_MEMD_BOUND_FAIL),
        None => emit_line(M_SELFTEST_MEMD_BOUND_FAIL),
    }
}

const POLL_INTERVAL_NS: u64 = 1_000_000_000;

/// The drill's steps, each with its own arming condition. The squeeze (6f-1) and the swap (6f-2)
/// need a FROZEN app that has not run again yet — the first freeze — so the thaw that follows
/// proves the way back.
/// `warn` (6e-3) proves the fan-out and needs a RUNNING app window at the end of it (the level
/// reaches app-host only while the process runs), so it fires as soon as a frozen app has run
/// again — the lifecycle phase's thaw, the app focused. `critical` (6e-2) proves the kill and
/// needs a FROZEN app, so it fires once an app is frozen again (the restore phase hides it once
/// more) — and again, once a second, while memd answers it with a squeeze instead of the kill.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Squeeze,
    Swap,
    Warn,
    Critical,
    Done,
}

/// Critical drills at the last step at most: a squeeze, a swap-out, then the kill (one spare).
const CRITICAL_TRIES: u8 = 4;

/// The drill's arming state, ticked from the end phase's idle loop.
pub(crate) struct Drill {
    /// Runs on QEMU alone (never on a board); a lane without apps never arms it.
    lane: bool,
    next_poll_ns: u64,
    step: Step,
    critical_tries: u8,
}

impl Drill {
    pub(crate) fn new() -> Self {
        let lane = boot_cfg::machine() == Machine::QemuVirt;
        Self { lane, next_poll_ns: 0, step: Step::Squeeze, critical_tries: 0 }
    }

    /// Once a second: `warn` when a frozen app has run again at least once (it is on screen
    /// now), `critical` when an app is frozen now — `SELFTEST: mem pressure drill (level=
    /// status= pid= band=)` names memd's answer each time, as ONE line (memd's own lines land
    /// between the request and the answer). The fan-out's proof is memd's own line when both
    /// legs took the level and app-host's when the window heard it; the kill's is memd's own
    /// line when the victim's death came back.
    pub(crate) fn tick(&mut self, now_ns: u64) {
        if !self.lane || self.step == Step::Done || now_ns < self.next_poll_ns {
            return;
        }
        self.next_poll_ns = now_ns.saturating_add(POLL_INTERVAL_NS);
        let Some(s) = status() else { return };
        // A first freeze the poll never saw (the thaw came first): the squeeze and swap steps
        // are passed — the ladder names the missing rung, the rest of the drill still runs.
        if matches!(self.step, Step::Squeeze | Step::Swap) && s.resumed >= 1 {
            self.step = Step::Warn;
        }
        let (level, name) = match self.step {
            Step::Squeeze | Step::Swap if s.suspended >= 1 => (MEM_PRESSURE_CRITICAL, "critical"),
            Step::Warn if s.resumed >= 1 => (MEM_PRESSURE_WARN, "warn"),
            Step::Critical if s.suspended >= 1 => (MEM_PRESSURE_CRITICAL, "critical"),
            _ => return,
        };
        let mut line = Line::new();
        line.put(M_SELFTEST_MEM_PRESSURE_DRILL.as_bytes());
        line.put(b"level=");
        line.put(name.as_bytes());
        let mut rsp = [0u8; wire::REPLY_MAX_BYTES];
        let answer = call(&wire::encode_drill(level), &mut rsp)
            .map(|n| wire::decode_drill_reply(wire::OP_DRILL, &rsp[..n]));
        let spared =
            matches!(answer, Some(Some((wire::STATUS_SQUEEZED | wire::STATUS_SWAPPED, _, _))));
        self.step = match self.step {
            Step::Squeeze => Step::Swap,
            Step::Swap => Step::Warn,
            Step::Warn => Step::Critical,
            Step::Critical => {
                self.critical_tries += 1;
                if spared && self.critical_tries < CRITICAL_TRIES {
                    Step::Critical
                } else {
                    Step::Done
                }
            }
            Step::Done => Step::Done,
        };
        match answer {
            None => line.put(b" status=unanswered pid=0 band=?)\n"),
            Some(None) => line.put(b" status=malformed pid=0 band=?)\n"),
            Some(Some((status, pid, band))) => {
                line.put(b" status=");
                line.put_u64(u64::from(status));
                line.put(b" pid=");
                line.put_u64(u64::from(pid));
                line.put(b" band=");
                line.put(wire::band_name(band).as_bytes());
                line.put(b")\n");
            }
        }
        line.emit();
    }
}

/// One console line, written whole: the drill's answer arrives after memd's own lines.
struct Line {
    buf: [u8; 96],
    len: usize,
}

impl Line {
    const fn new() -> Self {
        Self { buf: [0; 96], len: 0 }
    }

    fn put(&mut self, bytes: &[u8]) {
        let room = self.buf.len() - self.len;
        let n = bytes.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
    }

    fn put_u64(&mut self, mut v: u64) {
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.put(&digits[i..]);
    }

    fn emit(&self) {
        emit_bytes(&self.buf[..self.len]);
    }
}
