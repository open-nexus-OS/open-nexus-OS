// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE way a service paces itself or bounds a device (TASK-0054C P2-b,
//! RFC-0093 §7): a kernel timer bound to a DECLARED notify endpoint (`slots::<svc>::
//! TIMER_*` for pacing, `WATCHDOG_*` for a device that may never answer — both minted and
//! pinned by init from the topology), drained as a waitset member next to whatever else the
//! service waits on. No receive carries a deadline any more: the timer fires a frame, the
//! waitset says which member woke, the service reads its own clock only to COMPUTE a
//! deadline it hands to the kernel — never to decide a wait.
//!
//! One endpoint per timer: the kernel's fired frame names the timer id, but a service only
//! holds the timer's cap slot, so sharing an endpoint between two timers would leave the
//! drain unable to say which one fired.
//!
//! A waitset can report the timer's member READY without a fire: the kernel's last-sender
//! EOF latch (RFC-0079, TASK-0324 P8) is set on an endpoint whose only SEND cap is its own
//! owner's — exactly a timer-notify endpoint once init closed its minting cap — and the
//! latch ends a waitset wait so a supervisor learns of a peer that never wrote. The first
//! receive clears it. So a wake on the timer member is CONFIRMED by [`NotifyTimer::drain`]:
//! `true` = the timer fired; `false` = a spurious wake, wait again.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU — every paced service and both device watchdogs run through here;
//!   `check-wait-not-poll.sh` rule 4 proves no raw deadline remains beside it.

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use nexus_service_topology::SlotPair;

use crate::{IpcError, Result};

/// A kernel timer on its own declared notify endpoint.
pub struct NotifyTimer {
    cap: u32,
    recv: u32,
    armed_ns: u64,
    periodic: bool,
}

impl NotifyTimer {
    /// Binds a ONE-SHOT kernel timer to `pair.send` (the declared notify endpoint's SEND
    /// half); `pair.recv` is the half the service drains / waits on.
    pub fn bind(pair: SlotPair) -> Result<Self> {
        Self::bind_with_interval(pair, 0)
    }

    /// Binds a PERIODIC kernel timer (`interval_ns` between fires once armed) — the pacing
    /// form for a loop that must run its own work at a cadence (a network stack's poll, a
    /// hot-plug re-probe).
    pub fn bind_with_interval(pair: SlotPair, interval_ns: u64) -> Result<Self> {
        let cap =
            nexus_abi::timer_create(pair.send, interval_ns).map_err(|_| IpcError::Unsupported)?;
        Ok(Self { cap, recv: pair.recv, armed_ns: 0, periodic: interval_ns != 0 })
    }

    /// The RECV slot of the notify endpoint — add it to a waitset with [`Waitset::add`].
    pub fn recv_slot(&self) -> u32 {
        self.recv
    }

    /// The deadline currently armed (0 = disarmed).
    pub fn armed_ns(&self) -> u64 {
        self.armed_ns
    }

    /// Arms the timer at the absolute `deadline_ns` (0 = disarm): one kernel call per CHANGE,
    /// none while the same deadline stands.
    pub fn arm_at(&mut self, deadline_ns: u64) {
        if deadline_ns == self.armed_ns {
            return;
        }
        if self.armed_ns != 0 {
            let _ = nexus_abi::timer_cancel(self.cap);
            self.armed_ns = 0;
        }
        if deadline_ns != 0 && nexus_abi::timer_set(self.cap, deadline_ns).is_ok() {
            self.armed_ns = deadline_ns;
        }
    }

    /// Arms the timer `delta_ns` from now. The clock is read to COMPUTE the deadline the
    /// kernel owns from then on — it never decides the wait.
    pub fn arm_in(&mut self, delta_ns: u64) -> u64 {
        let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(delta_ns).max(1);
        self.arm_at(deadline);
        deadline
    }

    /// Disarms the timer (a no-op when nothing is armed).
    pub fn disarm(&mut self) {
        self.arm_at(0);
    }

    /// Drains every fired frame queued on the notify endpoint (non-blocking); `true` when at
    /// least one fired. A fired one-shot is disarmed by the kernel, so the armed deadline is
    /// forgotten here too.
    pub fn drain(&mut self) -> bool {
        let mut fired = false;
        let mut buf = [0u8; 32];
        loop {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            if nexus_abi::ipc_recv_v1(
                self.recv,
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
            if !self.periodic {
                self.armed_ns = 0;
            }
        }
    }

    /// Releases the timer capability (a timer created for one probe step, not a service's
    /// lifetime).
    pub fn close(self) {
        let _ = nexus_abi::timer_cancel(self.cap);
        let _ = nexus_abi::cap_close(self.cap);
    }

    /// Blocks until the timer fires (no deadline: the kernel timer IS the bound), then drains.
    /// The sleep primitive for a step that must simply let time pass.
    pub fn wait_fired(&mut self) -> Result<()> {
        let mut buf = [0u8; 32];
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        nexus_abi::ipc_recv_v1(self.recv, &mut hdr, &mut buf, nexus_abi::IPC_SYS_TRUNCATE, 0)
            .map_err(IpcError::Kernel)?;
        if !self.periodic {
            self.armed_ns = 0;
        }
        let _ = self.drain();
        Ok(())
    }
}

/// A kernel waitset over declared endpoints: `wait` blocks (no deadline) until one member
/// has a frame and returns that member's index in `add` order.
pub struct Waitset {
    cap: u32,
    members: u32,
}

impl Waitset {
    /// Creates an empty waitset.
    pub fn new() -> Result<Self> {
        let cap = nexus_abi::waitset_create().map_err(|_| IpcError::Unsupported)?;
        Ok(Self { cap, members: 0 })
    }

    /// Creates a waitset over `slots` (RECV halves), in that member order.
    pub fn over(slots: &[u32]) -> Result<Self> {
        let mut ws = Self::new()?;
        for &slot in slots {
            ws.add(slot)?;
        }
        Ok(ws)
    }

    /// Adds a member; returns its index (the value [`Self::wait`] reports for it).
    pub fn add(&mut self, recv_slot: u32) -> Result<u32> {
        nexus_abi::waitset_add(self.cap, recv_slot).map_err(|_| IpcError::Unsupported)?;
        let idx = self.members;
        self.members += 1;
        Ok(idx)
    }

    /// Blocks until a member has a pending frame; returns its index. No deadline — pacing is
    /// a [`NotifyTimer`] member, never this call's clock.
    pub fn wait(&self) -> Result<u32> {
        nexus_abi::waitset_wait(self.cap, 0).map_err(|_| IpcError::Unsupported)
    }

    /// Releases the waitset capability (one built for a probe step, not a service's loop).
    pub fn close(self) {
        let _ = nexus_abi::cap_close(self.cap);
    }
}
