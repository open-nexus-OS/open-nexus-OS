// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: the responder's wait (TASK-0324 P8): ONE waitset over every control channel plus
//! init's own timer-notify endpoint. A child's death reaches it through the kernel's EOF latch
//! on that child's control endpoint; a scheduled respawn through the one-shot timer armed at
//! its due time. No idle cadence, no safety net: nothing pending means zero wakes.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`SELFTEST: supervision restart ok`, `init: service restarted`)

use crate::bootstrap::CtrlChannel;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn build_ctrl_waitset(
    ctrl_channels: &[CtrlChannel],
    timer_ep: Option<u32>,
) -> Option<nexus_abi::Cap> {
    let ws = nexus_abi::waitset_create().ok()?;
    for chan in ctrl_channels {
        let _ = nexus_abi::waitset_add(ws, chan.ctrl_req_parent_slot);
    }
    if let Some(ep) = timer_ep {
        let _ = nexus_abi::waitset_add(ws, ep);
    }
    Some(ws)
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub(crate) fn build_ctrl_waitset(
    _ctrl_channels: &[CtrlChannel],
    _timer_ep: Option<u32>,
) -> Option<u32> {
    None
}

/// init's one-shot supervision timer (TASK-0324 P8): a notify endpoint init mints for itself,
/// a kernel timer bound to it, armed at the earliest scheduled respawn and disarmed otherwise.
pub(crate) struct ResponderClock {
    notify_ep: Option<u32>,
    timer: Option<u32>,
    armed_ns: u64,
}

impl ResponderClock {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    pub(crate) fn new() -> Self {
        let notify_ep =
            nexus_abi::ipc_endpoint_create_v2(crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT, 4).ok();
        let timer = notify_ep.and_then(|ep| nexus_abi::timer_create(ep, 0).ok());
        if notify_ep.is_none() || timer.is_none() {
            crate::os_payload::debug_write_bytes(
                b"init: FAIL supervision timer (respawns wait for the next event)\n",
            );
        }
        Self { notify_ep, timer, armed_ns: 0 }
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    pub(crate) fn new() -> Self {
        Self { notify_ep: None, timer: None, armed_ns: 0 }
    }

    pub(crate) fn notify_ep(&self) -> Option<u32> {
        self.notify_ep
    }

    /// Arms the one-shot at `due` (or leaves it disarmed): one kernel call per CHANGE.
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    pub(crate) fn arm(&mut self, due: Option<u64>) {
        let Some(timer) = self.timer else {
            return;
        };
        let want = due.unwrap_or(0);
        if want == self.armed_ns {
            return;
        }
        if self.armed_ns != 0 {
            let _ = nexus_abi::timer_cancel(timer);
            self.armed_ns = 0;
        }
        if want != 0 && nexus_abi::timer_set(timer, want).is_ok() {
            self.armed_ns = want;
        }
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    pub(crate) fn arm(&mut self, _due: Option<u64>) {}

    /// Drains the notify endpoint after a wake (the kernel disarmed a fired one-shot).
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    pub(crate) fn drain(&mut self) {
        let Some(ep) = self.notify_ep else {
            return;
        };
        let mut buf = [0u8; 32];
        loop {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            if nexus_abi::ipc_recv_v1(
                ep,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            )
            .is_err()
            {
                return;
            }
            self.armed_ns = 0;
        }
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    pub(crate) fn drain(&mut self) {}
}

/// Reactive idle for the responder loop: block until a control channel is ready, a child
/// died (the kernel's EOF latch on its channel) or the supervision one-shot fired — no
/// deadline (TASK-0324 P8). Without a waitset: a cooperative yield.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub(crate) fn responder_idle(waitset: Option<nexus_abi::Cap>) {
    match waitset {
        Some(ws) => {
            let _ = nexus_abi::waitset_wait(ws, 0);
        }
        None => {
            let _ = nexus_abi::yield_();
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub(crate) fn responder_idle(_waitset: Option<u32>) {
    let _ = nexus_abi::yield_();
}
