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
//! as its `gone` band.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU-proven (`memd: ready`, `memd: pressure (` on every lane; the drill's
//!   kill chain on the usb-visible lane); the policy is host-tested in tests/contract.rs

extern crate alloc;

use nexus_ipc::timer::Waitset;
use nexus_ipc::{Client as _, KernelClient, KernelServer, Wait};
use nexus_service_topology::slots::memd as topo;
use nexus_wire::memd::{band_name, source_name, SOURCE_DRILL, SOURCE_KERNEL};

use crate::answer::{Memd, Outcome, Peers};
use crate::authority::{Authority, BandEvent, Decision, Pending};
use crate::bands::Applied;
use crate::order::{encode_mem_kill, mem_kill_carried_out, KillOrder};

pub type Result<T> = core::result::Result<T, &'static str>;

/// Denied bands and drills logged per boot; the rest are counted silently.
const NOISY_LINES_MAX: u32 = 8;
/// Requests served per pass before the loop returns to the pressure endpoint.
const BATCH: usize = 32;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// The kill order on the declared leg: `OP_MEM_KILL` to abilitymgr, answered on the inbox (a
/// waited exchange: the answer or the authority's death — no clock).
struct AbilitymgrOrders;

impl KillOrder for AbilitymgrOrders {
    fn kill(&mut self, pid: u32) -> bool {
        let mut rsp = [0u8; 16];
        let req = encode_mem_kill(pid);
        match nexus_ipc::exchange::call_into(topo::ABILITYMGR.send, topo::REPLY, &req, &mut rsp) {
            Ok(n) => mem_kill_carried_out(&rsp[..n.min(rsp.len())], pid),
            Err(_) => false,
        }
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

    /// Drains every pending frame; each is judged at once and told onward.
    fn drain(&self, authority: &mut Authority, orders: &mut dyn KillOrder, fanout: &Fanout) {
        let Some(frames) = self.frames.as_ref() else { return };
        let mut buf = [0u8; 32];
        while let Ok(n) = frames.recv_into(Wait::NonBlocking, &mut buf) {
            let Some((level, free, total)) =
                nexus_abi::syscall::memory::decode_memory_pressure(&buf[..n])
            else {
                continue;
            };
            emit(&alloc::format!(
                "memd: pressure (level={} free={free} total={total})",
                nexus_abi::syscall::memory::mem_pressure_name(level)
            ));
            let decision = authority.on_pressure(level, free, total, orders);
            say_decision(level, "oom", decision);
            let told = fanout.tell(level, SOURCE_KERNEL, free, total);
            say_told(level, SOURCE_KERNEL, told);
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
    }
}

/// The band's marker, and the kill proof when the pending victim is gone. `true` asks the
/// caller to read the pool's level again (a kernel-sourced kill may need a successor).
fn say_band(event: BandEvent) -> bool {
    match event {
        BandEvent::Applied { pid, band, applied } => {
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
        BandEvent::KillProven { pending: Pending { band, source, .. }, again } => {
            emit(&alloc::format!(
                "SELFTEST: mem pressure kill ok (band={} reason={})",
                band_name(band),
                source.reason()
            ));
            again
        }
    }
}

/// The pool's level now, judged (after a kernel-sourced kill was proven).
fn judge_again(authority: &mut Authority, orders: &mut dyn KillOrder) {
    let Ok(stats) = nexus_abi::mm_stats() else { return };
    let level = stats.pressure_level.min(u64::from(u8::MAX)) as u8;
    let decision = authority.on_pressure(level, stats.free, stats.total, orders);
    say_decision(level, "oom", decision);
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
    let mut orders = AbilitymgrOrders;
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
                                if say_band(event) {
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
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                    if waitset.is_none() {
                        let _ = nexus_abi::yield_();
                    }
                    break;
                }
                Err(nexus_ipc::IpcError::Disconnected) => return Err("disconnected"),
                Err(_) => {
                    let (should_log, verdict) = breaker.on_error();
                    if should_log {
                        emit("memd: recv error (transient)");
                    }
                    match verdict {
                        nexus_ipc::resilience::BreakerVerdict::Continue => {
                            let _ = nexus_abi::yield_();
                        }
                        nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                            return Err("endpoint defect");
                        }
                    }
                    break;
                }
            }
        }
        // WAIT — no clock: a request or a pressure frame ends it.
        if let Some(ws) = waitset.as_ref() {
            let _ = ws.wait();
        }
    }
}
