// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: gpud's frame clock (TASK-0324 P7-d): the ONE-SHOT kernel timer on the declared
//! timer-notify endpoint that paces the self-presented phases (bootstrap splash, boot-splash
//! hold, spin demo) — a synthetic vblank on a device without one, a waitset member next to the
//! server endpoint. Never a recv timeout, never a timer on the server endpoint. Outside those
//! phases the timer is disarmed: an idle gpud takes zero wakes.
//! OWNERS: @gfx @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`gpud: hold tick alive`, the reveal handshake); the timer/waitset
//! primitives are kernel-selftested (`KSELFTEST: waitset wake ok`).

use nexus_ipc::Wait;

/// The frame period (~120 Hz).
#[cfg(nexus_env = "os")]
pub(crate) const FRAME_PERIOD_NS: u64 = 8_333_333;

/// gpud's frame clock (TASK-0324 P7-d): a one-shot kernel timer on the declared timer-notify
/// endpoint, armed one frame period after the last self-presented frame while a phase needs
/// pacing (bootstrap splash, boot-splash hold, spin demo); disarmed otherwise.
#[derive(Default)]
pub(crate) struct FrameClock {
    pub(crate) timer: Option<nexus_ipc::timer::NotifyTimer>,
    pub(crate) waitset: Option<u32>,
    /// The one-shot fired: the next idle pass presents a frame.
    pub(crate) due: bool,
    pub(crate) last_frame_ns: u64,
}

#[cfg(nexus_env = "os")]
impl FrameClock {
    /// Arms the next frame (one call per change), then WAITS on the waitset; a timer frame
    /// marks the frame due. Returns the wait the server recv must use (non-blocking after a
    /// waitset wake; blocking only without a waitset).
    pub(crate) fn wait(&mut self, pacing: bool) -> Wait {
        let Some(ws) = self.waitset else {
            return Wait::Blocking;
        };
        if let Some(timer) = self.timer.as_mut() {
            let want =
                if pacing { self.last_frame_ns.saturating_add(FRAME_PERIOD_NS).max(1) } else { 0 };
            timer.arm_at(want);
        }
        let _ = nexus_abi::waitset_wait(ws, 0);
        if self.timer.as_mut().is_some_and(|t| t.drain()) {
            self.due = true;
        }
        Wait::NonBlocking
    }

    /// A frame was self-presented now: the next one is a period later.
    pub(crate) fn frame_presented(&mut self, now_ns: u64) {
        self.due = false;
        self.last_frame_ns = now_ns;
    }
}
