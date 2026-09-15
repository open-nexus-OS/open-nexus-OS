// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The recv-wake regression probe (#102 family) and its instrument (TASK-0054C
//! P2-b): every bounded step is "the child's frame, or execd's timer" — a kernel one-shot
//! on the declared timer-notify pair, a waitset member beside the probe reply endpoint. No
//! receive deadline. Split out of `os_lite.rs` (structure gate); a child module so the
//! probe's slots, payload and helpers stay private to the service.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`SELFTEST: exec child blocking recv wake ok`, `SELFTEST: exec child
//!   eof on exit ok`; every `execd: FAIL recv-wake probe (…)` names its hop).

use super::*;
use nexus_ipc::timer::{NotifyTimer, Waitset};

/// P0.2 recv-wake regression gate: spawn the probe child, let it PARK in a
/// plain blocking ipc recv, then send it one message — the child must wake
/// and reply. Runs ONCE after `ready`, bounded (~2s worst case, ~50ms
/// healthy), headless, no user interaction. This is the deterministic
/// reproducer for the #102-family finding (proof boot 2026-07-07T12-12-27:
/// exec'd children parked in blocking recv were never woken by senders;
/// messages sat in their queue until a NonBlocking poll). Verdict markers:
/// `SELFTEST: exec child blocking recv wake ok` on success, a
/// `execd: FAIL recv-wake probe (…)` naming the failing hop otherwise.
pub(super) fn run_recv_wake_probe() {
    if recvwake_payload::RECVWAKE_ELF.is_empty() {
        emit_line("execd: recv-wake probe skipped (no elf)");
        return;
    }
    // The probe slots are pinned BEFORE execd runs (TASK-0324 P7-d: init's cap-distribution
    // pass precedes every resume) — one presence check, fail-closed.
    match nexus_abi::cap_clone(PROBE_REPLY_RECV_SLOT) {
        Ok(clone) => {
            let _ = nexus_abi::cap_close(clone);
        }
        Err(_) => {
            emit_line("execd: FAIL recv-wake probe (slots not wired)");
            return;
        }
    }
    let pid = match exec(recvwake_payload::RECVWAKE_ELF, 16, 0) {
        Ok(pid) => pid as u32,
        Err(_) => {
            emit_line("execd: FAIL recv-wake probe (spawn)");
            return;
        }
    };
    // Grants BEFORE resume (#102 discipline): ping RECV and reply SEND into the child's declared
    // probe slots. Each hop fails LOUD with its own marker; a mis-pinned execd slot is init's
    // `FAIL declared slot`, so the slot-occupancy dump that used to name an order drift is gone
    // with the order (TASK-0324 P4f-6).
    let ping_clone = nexus_abi::cap_clone(PROBE_PING_RECV_SLOT);
    let reply_clone = nexus_abi::cap_clone(PROBE_REPLY_SEND_SLOT);
    if ping_clone.is_err() || reply_clone.is_err() {
        emit_line("execd: FAIL recv-wake probe (grant clone)");
        return;
    }
    let ping_ok = ping_clone
        .and_then(|clone| {
            nexus_abi::cap_transfer_to_slot(
                pid as nexus_abi::Pid,
                clone,
                nexus_abi::Rights::RECV,
                PROBE_CHILD_PING_RECV_SLOT,
            )
            .map_err(|_| nexus_abi::AbiError::Unsupported)
        })
        .is_ok();
    let reply_ok = reply_clone
        .and_then(|clone| {
            nexus_abi::cap_transfer_to_slot(
                pid as nexus_abi::Pid,
                clone,
                nexus_abi::Rights::SEND,
                PROBE_CHILD_REPLY_SEND_SLOT,
            )
            .map_err(|_| nexus_abi::AbiError::Unsupported)
        })
        .is_ok();
    if !ping_ok || !reply_ok {
        emit_line(if !ping_ok {
            "execd: FAIL recv-wake probe (grant ping xfer)"
        } else {
            "execd: FAIL recv-wake probe (grant reply xfer)"
        });
        return;
    }
    if nexus_abi::task_resume(pid).is_err() {
        emit_line("execd: FAIL recv-wake probe (resume)");
        return;
    }
    // The probe's INSTRUMENT (TASK-0054C P2-b): a kernel one-shot on execd's declared
    // timer-notify pair, a waitset member beside the probe reply endpoint. Every bounded
    // step below is "the child's frame, or the timer's" — the timer is the FAIL witness a
    // kernel that never wakes the child would otherwise never produce. No receive deadline.
    let Ok(mut timer) = NotifyTimer::bind(topo::TIMER) else {
        emit_line("execd: FAIL recv-wake probe (timer)");
        return;
    };
    let Ok(waitset) = Waitset::over(&[PROBE_REPLY_RECV_SLOT, timer.recv_slot()]) else {
        emit_line("execd: FAIL recv-wake probe (waitset)");
        return;
    };
    let mut buf = [0u8; 16];
    // 1. Wait for the child's ARMED byte (the timer bounds a FAILED proof at 2 s).
    match probe_wait(&waitset, &mut timer, 2_000_000_000, &mut buf, false) {
        ProbeWake::Frame(n) if n >= 1 && buf[0] == PROBE_MSG_ARMED => {}
        ProbeWake::Frame(_) => {
            emit_line("execd: FAIL recv-wake probe (protocol armed)");
            return;
        }
        ProbeWake::Timer => {
            emit_line("execd: FAIL recv-wake probe (armed timeout)");
            return;
        }
        ProbeWake::Eof | ProbeWake::Error => {
            emit_line("execd: FAIL recv-wake probe (armed recv error)");
            return;
        }
    }
    // 2. TWO park/wake cycles (the counter repro proved wake 1 can succeed
    //    while the SECOND park is never woken again — wake-then-lost class;
    //    a single-cycle gate is half a gate). Per cycle: a park window in which
    //    the timer is EXPECTED to fire first, one ping to the parked child, a
    //    wait for its woke-reply bounded by the timer.
    for cycle in 0..2u32 {
        match probe_wait(&waitset, &mut timer, 30_000_000, &mut buf, false) {
            ProbeWake::Timer => {}
            ProbeWake::Frame(_) => {
                emit_line("execd: FAIL recv-wake probe (early reply)");
                return;
            }
            ProbeWake::Eof | ProbeWake::Error => {
                emit_line("execd: FAIL recv-wake probe (park recv error)");
                return;
            }
        }
        let ping = [0xB1u8];
        let ping_hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, ping.len() as u32);
        // A waited send (queue space or the child's death).
        if nexus_abi::ipc_send_v1(PROBE_PING_SEND_SLOT, &ping_hdr, &ping, 0, 0).is_err() {
            emit_line("execd: FAIL recv-wake probe (ping send)");
            return;
        }
        match probe_wait(&waitset, &mut timer, 2_000_000_000, &mut buf, false) {
            ProbeWake::Frame(n) if n >= 1 && buf[0] == PROBE_MSG_WOKE => {}
            ProbeWake::Frame(_) => {
                emit_line("execd: FAIL recv-wake probe (protocol woke)");
                return;
            }
            ProbeWake::Timer => {
                emit_line(if cycle == 0 {
                    "execd: FAIL recv-wake probe (child never woke from blocking recv)"
                } else {
                    "execd: FAIL recv-wake probe (child never woke AGAIN — second park lost)"
                });
                return;
            }
            ProbeWake::Eof | ProbeWake::Error => {
                emit_line("execd: FAIL recv-wake probe (woke recv error)");
                return;
            }
        }
    }
    emit_line("execd: recv-wake probe ok");
    let _ = nexus_abi::debug_println("SELFTEST: exec child blocking recv wake ok");

    // 3. DEATH WAKES (TASK-0324 P7-b): the child returns from `run` and exits, dropping the
    //    reply SEND cap it was granted. execd owns the reply endpoint and holds its own SEND
    //    base — an owner is never its own peer — so the waitset reads ready on the kernel's
    //    EOF latch the moment the child's cap table is reaped and the EOF-opted receive says
    //    `PeerClosed`, with no timeout deciding anything. (The timer only bounds a FAILED
    //    proof: a kernel that does not wake would otherwise park execd forever.)
    match probe_wait(&waitset, &mut timer, 2_000_000_000, &mut buf, true) {
        ProbeWake::Eof => {
            emit_line("execd: recv-wake probe eof on exit ok");
            let _ = nexus_abi::debug_println("SELFTEST: exec child eof on exit ok");
        }
        ProbeWake::Timer => emit_line("execd: FAIL recv-wake probe (no eof on child exit)"),
        ProbeWake::Frame(_) => emit_line("execd: FAIL recv-wake probe (frame after exit)"),
        ProbeWake::Error => emit_line("execd: FAIL recv-wake probe (eof recv error)"),
    }
    timer.close();
    waitset.close();
}

/// What ended one bounded probe step.
enum ProbeWake {
    /// The child's frame (`n` bytes in the caller's buffer).
    Frame(usize),
    /// The bound: the timer fired first.
    Timer,
    /// The child is gone (RFC-0079 EOF on the probe reply endpoint).
    Eof,
    /// A receive error that is neither.
    Error,
}

/// One bounded probe step: arm the timer `bound_ns` from now, wait on the waitset, and
/// read whichever member woke it. The reply endpoint is drained non-blocking (EOF-opted
/// when `eof` — the death-wake step); a spurious wake waits again.
fn probe_wait(
    ws: &Waitset,
    timer: &mut NotifyTimer,
    bound_ns: u64,
    buf: &mut [u8],
    eof: bool,
) -> ProbeWake {
    timer.arm_in(bound_ns);
    // The waitset says the reply endpoint is ready (a frame, or the kernel's EOF latch). The
    // death-wake step then receives BLOCKING with EOF opted in: the EOF decision (RFC-0079)
    // runs on the would-block branch only, so a non-blocking receive could never say
    // `PeerClosed` — and the waitset already guarantees the receive returns at once.
    let flags = nexus_abi::IPC_SYS_TRUNCATE
        | if eof { nexus_abi::IPC_SYS_EOF } else { nexus_abi::IPC_SYS_NONBLOCK };
    loop {
        match ws.wait() {
            Ok(0) => {
                let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
                match nexus_abi::ipc_recv_v1(PROBE_REPLY_RECV_SLOT, &mut hdr, buf, flags, 0) {
                    Ok(n) => {
                        timer.disarm();
                        return ProbeWake::Frame(n as usize);
                    }
                    Err(nexus_abi::IpcError::PeerClosed) => {
                        timer.disarm();
                        return ProbeWake::Eof;
                    }
                    Err(nexus_abi::IpcError::QueueEmpty) => continue,
                    Err(_) => {
                        timer.disarm();
                        return ProbeWake::Error;
                    }
                }
            }
            Ok(_) => {
                // The timer member woke us: a fire, or the kernel's EOF latch (spurious).
                if timer.drain() {
                    return ProbeWake::Timer;
                }
            }
            Err(_) => {
                timer.disarm();
                return ProbeWake::Error;
            }
        }
    }
}
