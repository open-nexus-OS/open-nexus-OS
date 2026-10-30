// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the memd service loop (OS target). Binds the declared server slots init pinned,
//! binds the kernel's ONE pressure sink to the declared endpoint (ADR-0085, syscall 67) and
//! waits on BOTH — a waitset over the server endpoint and the pressure endpoint, no clock, no
//! recv timeout. A pressure frame is judged at once ([`Authority::on_pressure`]); a request
//! is answered through [`Memd::answer`] with the kernel-attributed sender id; a kill order
//! rides the declared abilitymgr leg and is answered on the CAP_MOVE inbox. Markers are the
//! proof: `memd: pressure (level= free= total=)` for every kernel frame (the first at the
//! bind), `memd: band (pid= band=)` per push, `memd: kill (pid= band= reason=)` at the order
//! and ONE `SELFTEST: mem pressure kill ok (band= reason=)` when the victim's death came back
//! as its `gone` band. At `critical` a frozen app is squeezed before any kill (TASK-0333 6f-1,
//! ADR-0087): `memd: squeeze (pid= band= pages= …)` with the parent's totals and
//! `SELFTEST: mem squeeze ok (pages= net= ratio=% reason=)` when pages were recorded and frames
//! came back net; a kernel-sourced level is read again after it (`settle`). Next, before any kill
//! (TASK-0333 6f-2, ADR-0088), a squeezed frozen app is swapped out by this service's pager
//! (`pager.rs`): `memd: swap out (pid= band= pages= slots=)` and `SELFTEST: swap out ok (…)`; the
//! sink carries the kernel's page-in requests and drops to the pager, and a thaw pages the app
//! back: `memd: swap in (pid= pages=)`, `SELFTEST: swap in ok (pages=)` when every slot came back.
//! Every proven kill is recorded in the state store (TASK-0333 6f-3, `crate::journal`) and read
//! back: `SELFTEST: memd journal ok (reason= seq= records=)`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU-proven (`memd: ready`, `memd: pressure (` on every lane; the drill's
//!   squeeze and kill chains on the usb-visible lane; the kernel-sourced squeeze, swap-out, kill
//!   and journal — `reason=oom` — on the lowmem lane); the policy is host-tested in
//!   tests/contract.rs, the journal in tests/journal.rs

extern crate alloc;

use nexus_ipc::timer::Waitset;
use nexus_ipc::{Client as _, KernelClient, KernelServer, Wait};
use nexus_service_topology::slots::memd as topo;
use nexus_wire::memd::{band_name, source_name, SOURCE_DRILL, SOURCE_KERNEL};

use crate::answer::{Memd, Outcome, Peers};
use crate::authority::{Authority, BandEvent, Decision, Pending};
use crate::bands::Applied;
use crate::journal::{reason_name, KillRecord, Ledger, Store, StoreRefused, Written};
use crate::order::{
    encode_mem_kill, encode_mem_squeeze, mem_kill_carried_out, mem_squeeze_reply, Orders, Squeezed,
    SwappedOut,
};
use crate::pager::{PageIn, Pager};

pub type Result<T> = core::result::Result<T, &'static str>;

/// Denied bands and drills logged per boot; the rest are counted silently.
const NOISY_LINES_MAX: u32 = 8;
/// Requests served per pass before the loop returns to the pressure endpoint.
const BATCH: usize = 32;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// The orders on the declared leg: `OP_MEM_KILL` and `OP_MEM_SQUEEZE` to abilitymgr, answered on
/// the inbox (a waited exchange: the answer or the authority's death — no clock). A squeeze's
/// answer comes once the parent's batches ended (one app: the measured squeeze time). The
/// swap-out is this service's own pager (ADR-0088).
struct Carrier {
    pager: Pager,
}

impl Orders for Carrier {
    fn kill(&mut self, pid: u32) -> bool {
        let mut rsp = [0u8; 16];
        let req = encode_mem_kill(pid);
        match nexus_ipc::exchange::call_into(topo::ABILITYMGR.send, topo::REPLY, &req, &mut rsp) {
            Ok(n) => mem_kill_carried_out(&rsp[..n.min(rsp.len())], pid),
            Err(_) => false,
        }
    }

    fn squeeze(&mut self, pid: u32) -> Option<Squeezed> {
        let mut rsp = [0u8; 48];
        let req = encode_mem_squeeze(pid);
        let n = nexus_ipc::exchange::call_into(topo::ABILITYMGR.send, topo::REPLY, &req, &mut rsp)
            .ok()?;
        mem_squeeze_reply(&rsp[..n.min(rsp.len())], pid)
    }

    fn swap_ready(&mut self) -> bool {
        self.pager.ready()
    }

    fn swap_out(&mut self, pid: u32) -> Option<SwappedOut> {
        self.pager.swap_out(pid)
    }
}

/// The kernel's sink: bound to the declared SEND half; drained from the RECV half.
struct Sink {
    frames: Option<KernelClient>,
}

impl Sink {
    fn bind() -> Self {
        if nexus_abi::mem_pressure_bind(Some(topo::PRESSURE.send)).is_err() {
            emit("memd: sink FAIL (bind refused; the kernel's level is never heard)");
            return Self { frames: None };
        }
        let frames = KernelClient::new_with_slots(topo::PRESSURE.send, topo::PRESSURE.recv).ok();
        if frames.is_none() {
            emit("memd: sink FAIL (no client on the pressure endpoint)");
            let _ = nexus_abi::mem_pressure_bind(None);
        }
        Self { frames }
    }

    fn bound(&self) -> bool {
        self.frames.is_some()
    }

    /// Drains every pending frame: a level is judged at once and told onward; a page-in request
    /// is answered at once and a drop frees its slots (ADR-0088: the pager's notices ride the
    /// same sink).
    fn drain(&self, authority: &mut Authority, carrier: &mut Carrier, fanout: &Fanout) {
        let Some(frames) = self.frames.as_ref() else { return };
        let mut buf = [0u8; 160];
        // A dead object's slots arrive 32 a frame: counted over the pass, said once.
        let (mut dropped, mut freed) = (0usize, 0usize);
        while let Ok(n) = frames.recv_into(Wait::NonBlocking, &mut buf) {
            let frame = &buf[..n.min(buf.len())];
            if let Some((token, slot)) = nexus_abi::decode_page_in(frame) {
                say_page_in(carrier.pager.page_in(token, slot));
                continue;
            }
            let mut slots = [0u32; nexus_abi::SWAP_DROP_BATCH];
            if let Some(count) = nexus_abi::decode_swap_drops(frame, &mut slots) {
                dropped += count;
                freed += carrier.pager.dropped(&slots[..count]);
                continue;
            }
            let Some((level, free, total)) =
                nexus_abi::syscall::memory::decode_memory_pressure(frame)
            else {
                continue;
            };
            emit(&alloc::format!(
                "memd: pressure (level={} free={free} total={total})",
                nexus_abi::syscall::memory::mem_pressure_name(level)
            ));
            let decision = authority.on_pressure(level, free, total, carrier);
            say_decision(level, "oom", decision);
            settle(authority, carrier, decision);
            let told = fanout.tell(level, SOURCE_KERNEL, free, total);
            say_told(level, SOURCE_KERNEL, told);
        }
        if dropped > 0 {
            emit(&alloc::format!("memd: swap drop (slots={dropped} freed={freed})"));
        }
    }
}

/// The level told onward (TASK-0333 6e-3): to gpud (its caches) and to windowd (every app
/// window, as a DSL event through app-host) on the declared legs, NONBLOCK — the authority
/// never waits on a consumer; a full queue drops this frame and the next change tells the
/// truth of its moment. A leg that is not pinned (a lane without a display) tells nobody.
struct Fanout {
    gpud: Option<KernelClient>,
    windowd: Option<KernelClient>,
}

impl Fanout {
    fn bind() -> Self {
        Self {
            gpud: KernelClient::new_with_slots(topo::GPUD.send, topo::GPUD.recv).ok(),
            windowd: KernelClient::new_with_slots(topo::WINDOWD.send, topo::WINDOWD.recv).ok(),
        }
    }

    /// Tells `level`; answers `(gpud took it, windowd took it)`.
    fn tell(&self, level: u8, source: u8, free: u64, total: u64) -> (bool, bool) {
        let frame = nexus_wire::memd::encode_level(level, source, free, total);
        let send = |leg: &Option<KernelClient>| {
            leg.as_ref().is_some_and(|c| c.send(&frame, Wait::NonBlocking).is_ok())
        };
        (send(&self.gpud), send(&self.windowd))
    }
}

fn say_told(level: u8, source: u8, (gpud, windowd): (bool, bool)) {
    emit(&alloc::format!(
        "memd: level told (level={} source={} gpud={} windowd={})",
        nexus_abi::syscall::memory::mem_pressure_name(level),
        source_name(source),
        u8::from(gpud),
        u8::from(windowd)
    ));
}

fn say_decision(level: u8, reason: &str, decision: Decision) {
    match decision {
        Decision::None => {}
        Decision::NoVictim => emit(&alloc::format!(
            "memd: no victim (level={} reason={reason})",
            nexus_abi::syscall::memory::mem_pressure_name(level)
        )),
        Decision::Kill { pid, band, ordered: true } => {
            emit(&alloc::format!("memd: kill (pid={pid} band={} reason={reason})", band_name(band)))
        }
        Decision::Kill { pid, band, ordered: false } => emit(&alloc::format!(
            "memd: kill FAIL (pid={pid} band={} reason={reason}; the authority refused)",
            band_name(band)
        )),
        Decision::Squeeze { pid, band, squeezed: Some(s) } => {
            emit(&alloc::format!(
                "memd: squeeze (pid={pid} band={} pages={} zero={} kept={} taken={} stored={} reason={reason})",
                band_name(band),
                s.pages,
                s.zero,
                s.kept,
                s.taken,
                s.stored
            ));
            // The proof: pages recorded and frames given back net (ADR-0087).
            if s.pages > 0 && s.net_frames() > 0 {
                emit(&alloc::format!(
                    "SELFTEST: mem squeeze ok (pages={} net={} ratio={}% reason={reason})",
                    s.pages,
                    s.net_frames(),
                    s.ratio_percent()
                ));
            }
        }
        Decision::Squeeze { pid, band, squeezed: None } => emit(&alloc::format!(
            "memd: squeeze FAIL (pid={pid} band={} reason={reason}; the authority refused)",
            band_name(band)
        )),
        Decision::SwapOut { pid, band, out: Some(o) } => {
            emit(&alloc::format!(
                "memd: swap out (pid={pid} band={} pages={} slots={} reason={reason})",
                band_name(band),
                o.pages,
                o.slots
            ));
            // The proof: sealed records on the pager's disk, their pages released (ADR-0088).
            emit(&alloc::format!(
                "SELFTEST: swap out ok (pages={} slots={} reason={reason})",
                o.pages,
                o.slots
            ));
        }
        Decision::SwapOut { pid, band, out: None } => emit(&alloc::format!(
            "memd: swap out FAIL (pid={pid} band={} reason={reason}; nothing went)",
            band_name(band)
        )),
    }
}

/// A page-in that did not bring the page back, named (a served one is counted by the kernel:
/// `mm_stats` `page_ins`).
fn say_page_in(outcome: PageIn) {
    match outcome {
        PageIn::Back | PageIn::Unknown => {}
        PageIn::Stale => emit("memd: page-in stale (slot freed)"),
        PageIn::Dead => emit("memd: page-in refused (the record did not open; its waiters died)"),
        PageIn::Later => emit("memd: page-in later (no frame or no disk now)"),
    }
}

/// A thaw (ADR-0088): the app's slots paged back before it asks for them.
fn swap_in(carrier: &mut Carrier, pid: u32) {
    let (back, held) = carrier.pager.swap_in(pid);
    if held == 0 {
        return;
    }
    emit(&alloc::format!("memd: swap in (pid={pid} pages={back} slots={held})"));
    if back == held {
        emit(&alloc::format!("SELFTEST: swap in ok (pages={back})"));
    }
}

/// After a kernel-sourced squeeze or swap-out the pool is read again at once (ADR-0087/0088): a
/// level still at `critical` takes the next step — the next frozen app, the swap-out, then the
/// first kill — without waiting for a frame the kernel sends only on a level CHANGE. Bounded:
/// each step marks a process.
fn settle(authority: &mut Authority, orders: &mut dyn Orders, mut decision: Decision) {
    for _ in 0..2 * crate::bands::MAX_PROCESSES {
        if !matches!(decision, Decision::Squeeze { .. } | Decision::SwapOut { .. }) {
            return;
        }
        let Ok(stats) = nexus_abi::mm_stats() else { return };
        let level = stats.pressure_level.min(u64::from(u8::MAX)) as u8;
        decision = authority.on_pressure(level, stats.free, stats.total, orders);
        say_decision(level, "oom", decision);
    }
}

/// The state store on the declared statefsd leg, answered on the CAP_MOVE inbox (a waited
/// exchange: statefsd's answer or its death; no other exchange is open on the inbox then).
struct StateStore(statefs::client::StatefsClient);

impl StateStore {
    fn open() -> Option<Self> {
        let client = KernelClient::new_with_slots(topo::STATEFSD.send, topo::STATEFSD.recv).ok()?;
        let reply = KernelClient::new_with_slots(topo::REPLY.send, topo::REPLY.recv).ok()?;
        Some(Self(statefs::client::StatefsClient::from_clients(client, reply)))
    }
}

impl Store for StateStore {
    fn get(
        &mut self,
        key: &str,
    ) -> core::result::Result<Option<alloc::vec::Vec<u8>>, StoreRefused> {
        match self.0.get(key) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(statefs::StatefsError::NotFound) => Ok(None),
            Err(_) => Err(StoreRefused),
        }
    }

    fn put(&mut self, key: &str, value: &[u8]) -> core::result::Result<(), StoreRefused> {
        self.0.put(key, value).map_err(|_| StoreRefused)
    }
}

/// The proven kill, recorded (6f-3): the journal written to the state store and read back.
fn journal_kill(ledger: &mut Ledger, pending: &Pending) {
    let uptime_ms = nexus_abi::nsec().map(|ns| ns / 1_000_000).unwrap_or(0);
    let kill = KillRecord::of(pending, uptime_ms);
    let Some(mut store) = StateStore::open() else {
        emit("memd: journal FAIL (no statefsd leg)");
        return;
    };
    match ledger.record(kill, &mut store) {
        Written::Proven { kept, records, reset } => {
            if reset {
                emit("memd: journal reset (the stored record was not this layout)");
            }
            emit(&alloc::format!(
                "SELFTEST: memd journal ok (reason={} seq={} records={records})",
                reason_name(kept.reason),
                kept.seq
            ));
        }
        Written::ReadRefused => emit("memd: journal FAIL (read refused; the kill waits)"),
        Written::WriteRefused => emit("memd: journal FAIL (write refused; written with the next)"),
        Written::ReadBackDiffers => emit("memd: journal FAIL (read-back differs)"),
    }
}

/// The band's marker, and the kill proof when the pending victim is gone. `true` asks the
/// caller to read the pool's level again (a kernel-sourced kill may need a successor).
fn say_band(event: BandEvent, carrier: &mut Carrier, ledger: &mut Ledger) -> bool {
    match event {
        BandEvent::Applied { pid, band, applied } => {
            // A thaw: the app runs again — its swapped pages come back before it asks.
            if applied == (Applied::Moved { from: nexus_wire::memd::BAND_SUSPENDED }) {
                swap_in(carrier, pid);
            }
            match applied {
                Applied::Entered | Applied::Moved { .. } | Applied::Gone { .. } => {
                    emit(&alloc::format!("memd: band (pid={pid} band={})", band_name(band)))
                }
                Applied::Full => {
                    emit(&alloc::format!("memd: band dropped (pid={pid}; table full)"))
                }
                Applied::UnknownBand => emit(&alloc::format!("memd: band malformed (pid={pid})")),
                Applied::Unchanged | Applied::UnknownGone => {}
            }
            false
        }
        BandEvent::KillProven { pending, again } => {
            emit(&alloc::format!(
                "SELFTEST: mem pressure kill ok (band={} reason={})",
                band_name(pending.band),
                pending.source.reason()
            ));
            journal_kill(ledger, &pending);
            again
        }
    }
}

/// The pool's level now, judged (after a kernel-sourced kill was proven).
fn judge_again(authority: &mut Authority, orders: &mut dyn Orders) {
    let Ok(stats) = nexus_abi::mm_stats() else { return };
    let level = stats.pressure_level.min(u64::from(u8::MAX)) as u8;
    let decision = authority.on_pressure(level, stats.free, stats.total, orders);
    say_decision(level, "oom", decision);
    settle(authority, orders, decision);
}

pub fn service_main_loop() -> Result<()> {
    let server =
        KernelServer::new_with_slots(topo::SERVER.recv, topo::SERVER.send).map_err(|_| "bind")?;
    let sink = Sink::bind();
    let waitset = sink
        .bound()
        .then(|| Waitset::over(&[topo::SERVER.recv, topo::PRESSURE.recv]).ok())
        .flatten();
    let peers = Peers {
        abilitymgr: nexus_abi::service_id_from_name(b"abilitymgr"),
        harness: nexus_abi::service_id_from_name(b"selftest-client"),
        gpud: nexus_abi::service_id_from_name(b"gpud"),
    };
    let mut memd = Memd::new(peers, sink.bound());
    let mut orders = Carrier { pager: Pager::new() };
    let mut ledger = Ledger::new();
    let fanout = Fanout::bind();
    let _ = nexus_service_entry::ready("memd: ready");
    nexus_abi::service_verdict_flush("memd");

    // ONE request buffer for the service lifetime: a receive allocates nothing.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    let mut out = [0u8; nexus_wire::memd::REPLY_MAX_BYTES];
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    let mut noisy = 0u32;
    let wait = if waitset.is_some() { Wait::NonBlocking } else { Wait::Blocking };
    loop {
        sink.drain(memd.authority(), &mut orders, &fanout);
        for _ in 0..BATCH {
            match server.recv_request_with_header_into(wait, &mut recv_frame) {
                Ok((_hdr, frame_len, sender_service_id, reply)) => {
                    breaker.on_success();
                    let frame = &recv_frame[..frame_len.min(recv_frame.len())];
                    let (n, outcome) = memd.answer(frame, sender_service_id, &mut out, &mut orders);
                    let denied = matches!(
                        outcome,
                        Outcome::BandDenied | Outcome::DrillDenied | Outcome::GpuChargeDenied
                    );
                    if denied {
                        if noisy < NOISY_LINES_MAX {
                            noisy += 1;
                            emit(match outcome {
                                Outcome::BandDenied => "memd: band denied (sender)",
                                Outcome::GpuChargeDenied => "memd: gpu charge denied (sender)",
                                _ => "memd: drill denied (sender)",
                            });
                        }
                    } else {
                        match outcome {
                            Outcome::Band(event) => {
                                if say_band(event, &mut orders, &mut ledger) {
                                    judge_again(memd.authority(), &mut orders);
                                }
                            }
                            Outcome::Drill { level, decision } => {
                                say_decision(level, "drill", decision);
                                // The drill's level is told onward like the kernel's: the
                                // fan-out is proven when both legs took it.
                                let (_, free, total) = memd.authority().level();
                                let told = fanout.tell(level, SOURCE_DRILL, free, total);
                                say_told(level, SOURCE_DRILL, told);
                                if told == (true, true) {
                                    emit(&alloc::format!(
                                        "SELFTEST: mem pressure fanout ok (level={} sent=2)",
                                        nexus_abi::syscall::memory::mem_pressure_name(level)
                                    ));
                                } else {
                                    emit(&alloc::format!(
                                        "memd: fanout FAIL (level={} gpud={} windowd={})",
                                        nexus_abi::syscall::memory::mem_pressure_name(level),
                                        u8::from(told.0),
                                        u8::from(told.1)
                                    ));
                                }
                            }
                            Outcome::GpuCharge { client, bytes, total } => {
                                // 6e-4: the GPU's memory, charged to the client gpud holds it
                                // for; the compositor by name, anyone else by kernel id.
                                let windowd = nexus_abi::service_id_from_name(b"windowd");
                                match total {
                                    Some(total) if client == windowd => emit(&alloc::format!(
                                        "memd: gpu charge (client=windowd bytes={bytes} total={total})"
                                    )),
                                    Some(total) => emit(&alloc::format!(
                                        "memd: gpu charge (client={client:#x} bytes={bytes} total={total})"
                                    )),
                                    None => emit(&alloc::format!(
                                        "memd: FAIL gpu charge (client={client:#x} table full)"
                                    )),
                                }
                            }
                            Outcome::Malformed => emit("memd: request malformed"),
                            Outcome::Unsupported => emit("memd: request unsupported"),
                            Outcome::Status
                            | Outcome::BandDenied
                            | Outcome::DrillDenied
                            | Outcome::GpuChargeDenied => {}
                        }
                    }
                    match reply {
                        Some(reply) if n > 0 => {
                            let _ = reply.reply_and_close(&out[..n]);
                        }
                        // A cap with nothing to say is dropped (closes); no cap, no reply.
                        _ => {}
                    }
                }
                // Drained: the wait below (or, without a waitset, the next blocking receive)
                // sleeps until the next frame — no yield.
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => break,
                Err(nexus_ipc::IpcError::Disconnected) => return Err("disconnected"),
                Err(_) => {
                    let (should_log, verdict) = breaker.on_error();
                    if should_log {
                        emit("memd: recv error (transient)");
                    }
                    // A transient error waits like a drained queue (the breaker bounds a run of
                    // them); a defect ends the loop.
                    if verdict == nexus_ipc::resilience::BreakerVerdict::EndpointDefect {
                        return Err("endpoint defect");
                    }
                    break;
                }
            }
        }
        // A page-in that found no frame tries again on every pass (the kernel asks once).
        orders.pager.retry();
        // WAIT — no clock: a request or a pressure frame ends it.
        if let Some(ws) = waitset.as_ref() {
            let _ = ws.wait();
        }
    }
}
