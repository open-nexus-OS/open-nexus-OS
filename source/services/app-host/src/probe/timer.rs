// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: the app's clock timer (TASK-0324 P7-d): a ONE-SHOT kernel timer on the app's
//! timer-notify pair (minted by execd), armed at the next minute boundary and drained as a
//! waitset member next to the event channel — the clock ticks without a recv timeout.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`apphost: clock tick applied`)

/// Arms the app's one-shot clock timer at `deadline` (0 = none): one kernel call per CHANGE.
pub(super) fn arm_clock_timer(timer: u32, deadline: Option<u64>, armed_ns: &mut u64) {
    let want = deadline.unwrap_or(0);
    if want == *armed_ns {
        return;
    }
    if *armed_ns != 0 {
        let _ = nexus_abi::timer_cancel(timer);
        *armed_ns = 0;
    }
    if want != 0 && nexus_abi::timer_set(timer, want).is_ok() {
        *armed_ns = want;
    }
}

/// Drains the timer-notify endpoint; `true` if the one-shot fired.
pub(super) fn drain_timer_notify() -> bool {
    let mut fired = false;
    let mut buf = [0u8; 32];
    loop {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        if nexus_abi::ipc_recv_v1(
            super::TIMER_RECV_SLOT,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        )
        .is_err()
        {
            return fired;
        }
        fired = true;
    }
}
