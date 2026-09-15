// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS netstackd IPC facade runtime loop (poll, recv, RPC dispatch)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

extern crate alloc;

use nexus_net_os::SmoltcpVirtioNetStack;

pub(crate) fn run_facade_loop(mut net: SmoltcpVirtioNetStack) -> ! {
    use nexus_net::NetStack as _;

    use crate::os::ipc::handles::ReplyCapSlot;
    use crate::os::ipc::parse::has_valid_wire_header;
    use crate::os::ipc::reply::status_frame;
    use crate::os::ipc::wire::STATUS_MALFORMED;

    use crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
    use crate::os::facade::dispatch::{dispatch_op, DispatchControl, FacadeContext};
    use crate::os::facade::state::FacadeState;

    // The facade listens on netstackd's DECLARED server slot (TASK-0324 P4f-4). It answers every
    // request on the caller's CAP_MOVE reply cap, so the pair's send half is never used here.
    // Ownership model: this loop is the sole owner of `net` + `state`, and each handler receives
    // temporary exclusive borrows through `FacadeContext` for one request turn.
    /// The network stack's poll cadence (timers, retransmits, DHCP): a PERIODIC kernel timer on
    /// the declared notify pair (TASK-0054C P2-b) — never a receive deadline.
    const POLL_INTERVAL_NS: u64 = 5_000_000;
    let svc_recv_slot = nexus_service_topology::slots::netstackd::SERVER.recv;
    let mut state = FacadeState::new();
    crate::os::facade::authz::resolve(&mut state.policy);
    // ONE waitset over the facade endpoint and the poll timer: a request or the tick wakes
    // the loop (the 5 ms timed park that used to double as the clock is gone).
    let mut poll_timer = nexus_ipc::timer::NotifyTimer::bind_with_interval(
        nexus_service_topology::slots::netstackd::TIMER,
        POLL_INTERVAL_NS,
    )
    .ok();
    let waitset = poll_timer
        .as_ref()
        .and_then(|t| nexus_ipc::timer::Waitset::over(&[svc_recv_slot, t.recv_slot()]).ok());
    if let Some(t) = poll_timer.as_mut() {
        t.arm_in(POLL_INTERVAL_NS);
    }
    if waitset.is_none() {
        let _ = nexus_abi::debug_println(
            "netstackd: FAIL waitset/timer (blocking on the facade endpoint alone)",
        );
    }
    let recv_flags = if waitset.is_some() {
        nexus_abi::IPC_SYS_TRUNCATE | nexus_abi::IPC_SYS_NONBLOCK
    } else {
        nexus_abi::IPC_SYS_TRUNCATE
    };

    loop {
        let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        net.poll(now_ms);

        // Prefer the currently configured IP (DHCP or static fallback). This keeps the facade usable
        // under non-DHCP backends (e.g. 2-VM socket/mcast harness).
        let bind_ip = net
            .get_ipv4_config()
            .or_else(|| net.get_dhcp_config())
            .map(|c| c.ip)
            .unwrap_or(QEMU_USERNET_FALLBACK_IP);

        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let mut buf = [0u8; 512];
        // One request per wake (non-blocking behind the waitset: a queued request keeps the
        // waitset ready, so the next turn takes the next frame); no deadline anywhere.
        let recv_result =
            nexus_abi::ipc_recv_v2(svc_recv_slot, &mut hdr, &mut buf, &mut sid, recv_flags, 0);
        match recv_result {
            Ok(n) => {
                // Log first IPC receipt to confirm message flow.
                static FIRST_IPC_LOGGED: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !FIRST_IPC_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    let _ = nexus_abi::trace_line("netstackd: first ipc recv");
                }
                let n = n as usize;
                let req = &buf[..n];
                let reply_slot = if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
                    Some(ReplyCapSlot::new(hdr.src as u32))
                } else {
                    None
                };

                let reply = |frame: &[u8]| {
                    if let Some(slot) = reply_slot {
                        let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
                        // Blocking reply: avoid silently dropping replies under queue pressure.
                        let _ = nexus_abi::ipc_send_v1(slot.raw(), &rh, frame, 0, 0);
                        let _ = nexus_abi::cap_close(slot.raw());
                    }
                };

                if !has_valid_wire_header(req) {
                    reply(&status_frame(0, STATUS_MALFORMED));
                    continue;
                }

                let mut reply_fn = reply;
                let mut ctx = FacadeContext {
                    net: &mut net,
                    state: &mut state,
                    now_ms,
                    bind_ip,
                    reply_slot,
                    sender_service_id: sid,
                };
                match dispatch_op(&mut ctx, req, &mut reply_fn) {
                    DispatchControl::ContinueLoop => continue,
                    DispatchControl::Handled => {}
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                // The tick woke us without a request: the stack is driven on the next turn.
            }
            Err(_) => {
                static IPC_RECV_ERR_LOGGED: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !IPC_RECV_ERR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    let _ = nexus_abi::debug_println("netstackd: ipc recv err");
                }
            }
        }

        if let Some(t) = poll_timer.as_mut() {
            t.drain();
        }
        // WAIT — no clock: the next request or the next poll tick.
        if let Some(ws) = waitset.as_ref() {
            let _ = ws.wait();
        }
    }
}
