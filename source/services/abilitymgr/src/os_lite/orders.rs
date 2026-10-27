// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The OS half of the lifecycle policy (TASK-0333 6d, RFC-0111 §4–5, ADR-0083): the
//! clock the pure `seen` policy runs on (a one-shot kernel timer armed at the first hidden
//! app's grace, a waitset member next to the server endpoint — no tick, no recv timeout), the
//! sightings the compositor sends (`OP_SEEN`, taken from windowd's kernel sender id alone) and
//! the orders carried to execd (`OP_FREEZE`/`OP_THAW`, a request answered on the declared
//! execd leg). Markers are the proof: `abilitymgr: suspend (inst=` after execd froze the
//! process, `abilitymgr: thaw (inst=` after it ran again, and ONE
//! `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` on the first full cycle of a boot.
//! TASK-0333 6e-2 (RFC-0100 §3, ADR-0086): the same glue feeds the memory authority — every
//! band change goes to memd on the declared leg (NONBLOCK, a refused push stays due) and
//! memd's kill order (`OP_MEM_KILL`, from its kernel sender id alone) is judged by the pure
//! `mem_kill` policy and carried to execd as `OP_KILL`: `abilitymgr: kill (inst= reason=memory)`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: policy host-tested in `seen.rs`; this glue on the visible QEMU lane and the
//!   board (the markers above)

use nexus_ipc::timer::{NotifyTimer, Waitset};
use nexus_ipc::{Client as _, KernelClient, Wait};
use nexus_service_topology::slots::abilitymgr as topo;

use super::{emit_inst_line, emit_line};
use crate::bands::BandFeed;
use crate::lifecycle::{AbilityState, Broker};
use crate::mem_kill::{encode_mem_kill_reply, handle_mem_kill, MemKillOutcome};
use crate::protocol::{STATUS_DENIED, STATUS_FAILED, STATUS_MALFORMED, STATUS_OK, STATUS_UNKNOWN};
use crate::seen::{handle_seen, Order, SeenOutcome};

/// The loop's clock and its proof state.
pub(super) struct Lifecycle {
    /// The waitset over the server endpoint and the timer (`None`: the loop blocks on the
    /// server endpoint alone and no app is ever frozen — reported once at start).
    pub(super) waitset: Option<Waitset>,
    timer: Option<NotifyTimer>,
    /// The compositor's kernel identity: the one sender whose sightings count.
    windowd_sid: u64,
    /// A freeze was carried out this boot (the SELFTEST line needs the full cycle).
    froze: bool,
    /// The SELFTEST line was emitted (once per boot).
    proven: bool,
    /// The memory authority's kernel identity: the one sender whose kill orders count.
    memd_sid: u64,
    /// The declared leg to memd (bound at the first push; `None` until init pinned it).
    memd: Option<KernelClient>,
    /// What memd last heard of every process.
    bands: BandFeed,
    /// A denied kill order was named this boot (the rest are counted silently).
    kill_denied_said: bool,
}

impl Lifecycle {
    /// Binds the declared timer pair and the waitset over the server's receive endpoint and
    /// the timer's. Without either the loop still serves; it only never freezes.
    pub(super) fn bind() -> Self {
        let timer = NotifyTimer::bind(topo::TIMER).ok();
        let waitset =
            timer.as_ref().and_then(|t| Waitset::over(&[topo::SERVER.recv, t.recv_slot()]).ok());
        if waitset.is_none() {
            emit_line("abilitymgr: FAIL freeze clock (no timer/waitset; apps never freeze)");
        }
        Self {
            waitset,
            timer,
            windowd_sid: nexus_abi::service_id_from_name(b"windowd"),
            froze: false,
            proven: false,
            memd_sid: nexus_abi::service_id_from_name(b"memd"),
            memd: None,
            bands: BandFeed::new(),
            kill_denied_said: false,
        }
    }

    /// Pushes every band that differs from what memd last heard (TASK-0333 6e-2): NONBLOCK on
    /// the declared leg, committed only when the send was taken — memd not yet up or a full
    /// queue leaves the band due for the next pass. Cheap: at most 32 instances.
    pub(super) fn publish_bands(&mut self, broker: &Broker) {
        let due = self.bands.changes(broker);
        if due.is_empty() {
            return;
        }
        if self.memd.is_none() {
            self.memd = KernelClient::new_with_slots(topo::MEMD.send, topo::MEMD.recv).ok();
        }
        let Some(client) = self.memd.as_ref() else { return };
        for (pid, band) in due {
            let frame = nexus_wire::memd::encode_band(pid, band);
            if client.send(&frame, Wait::NonBlocking).is_ok() {
                self.bands.commit(pid, band);
            }
        }
    }

    /// One `OP_MEM_KILL` frame from `sender_service_id` (TASK-0333 6e-2): refused unless it is
    /// the memory authority's; judged by the pure policy (never the foreground); the order
    /// carried to execd at once. Returns the reply frame.
    pub(super) fn on_mem_kill(
        &mut self,
        broker: &Broker,
        frame: &[u8],
        sender_service_id: u64,
    ) -> [u8; 9] {
        if !crate::mem_kill::mem_kill_frame_allowed(frame, sender_service_id, self.memd_sid) {
            if !self.kill_denied_said {
                self.kill_denied_said = true;
                emit_line("abilitymgr: kill denied (sender)");
            }
            return encode_mem_kill_reply(STATUS_DENIED, 0);
        }
        match handle_mem_kill(broker, frame) {
            MemKillOutcome::Malformed => {
                emit_line("abilitymgr: kill malformed");
                encode_mem_kill_reply(STATUS_MALFORMED, 0)
            }
            MemKillOutcome::Unknown { pid } => {
                emit_inst_line(b"abilitymgr: kill unknown (pid=", pid);
                encode_mem_kill_reply(STATUS_UNKNOWN, pid)
            }
            MemKillOutcome::Foreground { instance_id, pid } => {
                emit_inst_line(b"abilitymgr: kill denied (foreground; inst=", instance_id);
                encode_mem_kill_reply(STATUS_DENIED, pid)
            }
            MemKillOutcome::Victim { instance_id, pid, .. } => {
                if order_execd(OP_KILL, pid) {
                    nexus_abi::debug_ts_prefix();
                    super::emit_prefix(b"abilitymgr: kill (inst=");
                    super::emit_u32(instance_id);
                    super::emit_prefix(b" reason=memory)");
                    super::emit_newline();
                    encode_mem_kill_reply(STATUS_OK, pid)
                } else {
                    emit_inst_line(b"abilitymgr: FAIL kill (inst=", instance_id);
                    encode_mem_kill_reply(STATUS_FAILED, pid)
                }
            }
        }
    }

    /// One `OP_SEEN` frame from `sender_service_id`: refused unless it is the compositor's;
    /// applied through the pure policy; the order it yields is carried out at once.
    pub(super) fn on_seen(&mut self, broker: &mut Broker, frame: &[u8], sender_service_id: u64) {
        if sender_service_id != self.windowd_sid {
            emit_line("abilitymgr: seen denied (sender)");
            return;
        }
        let now = nexus_abi::nsec().unwrap_or(0);
        match handle_seen(broker, frame, now, app_sid) {
            SeenOutcome::Malformed => emit_line("abilitymgr: seen malformed"),
            // A window of a process this service never launched (the shell's own surfaces are
            // not instances) — routine, not a fault; the compositor reports every app window.
            SeenOutcome::Unknown { .. } => {}
            SeenOutcome::Applied { instance_id, state, order } => {
                match state {
                    AbilityState::Foreground => {
                        emit_inst_line(b"abilitymgr: fg (inst=", instance_id)
                    }
                    AbilityState::Background => {
                        emit_inst_line(b"abilitymgr: bg (inst=", instance_id)
                    }
                    _ => {}
                }
                if let Some(order) = order {
                    self.carry(order);
                }
            }
        }
        self.rearm(broker);
    }

    /// The timer fired (or a pass ended): every instance hidden past its grace is frozen, and
    /// the timer is re-armed at the next grace to run out.
    pub(super) fn pump(&mut self, broker: &mut Broker) {
        let now = nexus_abi::nsec().unwrap_or(0);
        for order in broker.due_freezes(now) {
            self.carry(order);
        }
        self.rearm(broker);
    }

    /// Drains the fired timer frame(s) after a waitset wake.
    pub(super) fn drain_timer(&mut self) {
        if let Some(t) = self.timer.as_mut() {
            t.drain();
        }
    }

    fn rearm(&mut self, broker: &Broker) {
        if let Some(t) = self.timer.as_mut() {
            t.arm_at(broker.next_freeze_deadline().unwrap_or(0));
        }
    }

    /// Carries one order to execd and emits the proof line when it was carried out.
    fn carry(&mut self, order: Order) {
        match order {
            Order::Freeze { instance_id, pid } => {
                if order_execd(OP_FREEZE, pid) {
                    self.froze = true;
                    emit_inst_line(b"abilitymgr: suspend (inst=", instance_id);
                } else {
                    emit_inst_line(b"abilitymgr: FAIL freeze (inst=", instance_id);
                }
            }
            Order::Thaw { instance_id, pid } => {
                if order_execd(OP_THAW, pid) {
                    emit_inst_line(b"abilitymgr: thaw (inst=", instance_id);
                    if self.froze && !self.proven {
                        self.proven = true;
                        emit_line("SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)");
                    }
                } else {
                    emit_inst_line(b"abilitymgr: FAIL thaw (inst=", instance_id);
                }
            }
        }
    }
}

/// execd's order ops (RFC-0111 §5): `[E,X,1,op, pid:u32le]` → `[E,X,1,op|0x80,status,pid]`.
const OP_FREEZE: u8 = 4;
const OP_THAW: u8 = 5;
/// An ORDERED kill (no crash report): the memory authority's victim (ADR-0086).
const OP_KILL: u8 = 6;

/// The kernel service id execd gives an app's process (`app:<bundle_id>`, execd's `identity`):
/// the key the compositor reports a window by.
fn app_sid(app_id: &str) -> u64 {
    let mut name = [0u8; 64];
    let app = app_id.as_bytes();
    let n = app.len().min(name.len() - 4);
    name[..4].copy_from_slice(b"app:");
    name[4..4 + n].copy_from_slice(&app[..n]);
    nexus_abi::service_id_from_name(&name[..4 + n])
}

/// One order to execd on the declared leg, answered on execd's response endpoint — the spawn
/// request's shape (`spawn_app`). `true` when execd reports the order carried out.
fn order_execd(op: u8, pid: u32) -> bool {
    let Ok(client) = KernelClient::new_with_slots(topo::EXECD.send, topo::EXECD.recv) else {
        return false;
    };
    let mut req = [b'E', b'X', 1, op, 0, 0, 0, 0];
    req[4..8].copy_from_slice(&pid.to_le_bytes());
    if client.send(&req, Wait::Blocking).is_err() {
        return false;
    }
    // WAIT for `[E,X,ver,op|0x80,status,pid:u32le]` — no clock: execd answers or dies (which
    // closes its endpoint and ends the wait).
    let mut rsp = [0u8; 16];
    loop {
        match client.recv_into(Wait::Blocking, &mut rsp) {
            Ok(n) if n >= 9 && rsp[3] == op | 0x80 => return rsp[4] == 0,
            Ok(_) => {} // unrelated frame on the shared channel — keep waiting
            Err(_) => return false,
        }
    }
}
