// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the delivery of fired timer caps (RFC-0062): each is popped from the timer table,
//! sent to its notification endpoint as `OP_TIMER_FIRED`, and the receiver waiting there is
//! woken; then the delivering hart re-arms for its own next deadline. Everything it touches is
//! behind its own lock — the timer table, the endpoint, the woken thread's record, the run
//! queues — so a hart delivers with or without the kernel lock (TASK-0330 P4): the timer
//! interrupt and the boot hart's idle delivery inside it, a secondary's idle loop without
//! (`ipc_entry::deliver_timers`).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: timer cap` (fire, cancel, a waitset's member), `SELFTEST: timed
//!   coalesce ok`, the rank-check lane (QEMU); `KINIT: bkl idle split` shows what is left under
//!   the lock
//! INVARIANTS: the caller holds no ranked lock; the events are sent inside the timer table's
//!   hold that popped them (`HartTimers::deliver`) — in a timer's order, before its
//!   endpoint's death
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use super::*;

pub(crate) const OP_TIMER_FIRED: u8 = 0x30;

/// Delivers the fired timer caps and re-arms this hart for its own next deadline; returns how
/// many caps fired.
pub(crate) fn process_expired_timers(
    timer: &dyn Timer,
    hart_timers: &crate::timer::HartTimers,
    router: &ipc::Router,
    scheduler: &mut Scheduler,
) -> usize {
    let now = timer.now();
    let fired = hart_timers.deliver(now, |timer_id, state| {
        let mut payload = [0u8; 29];
        payload[0] = OP_TIMER_FIRED;
        payload[1..5].copy_from_slice(&timer_id.0.to_le_bytes());
        payload[5..9].copy_from_slice(&(state.seq.wrapping_add(1)).to_le_bytes());
        payload[9..13].copy_from_slice(&state.missed.to_le_bytes());
        payload[13..21].copy_from_slice(&state.deadline_ns.to_le_bytes());
        payload[21..29].copy_from_slice(&now.to_le_bytes());
        let header = ipc::header::MessageHeader::new(
            0,
            state.notify_ep,
            OP_TIMER_FIRED as u16,
            0,
            payload.len() as u32,
        );
        let msg = ipc::Message::new(header, ipc::Payload::from_slice(&payload), None);
        if router.send(state.notify_ep, msg).is_ok() {
            if let Ok(Some(waiter)) = router.pop_recv_waiter(state.notify_ep) {
                let _ = task::wake::wake(task::Pid::from_raw(waiter), scheduler);
            }
        }
    });
    runtime::rearm_own(timer, now);
    fired
}
