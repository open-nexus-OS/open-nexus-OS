// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The device's completion wait and its bound (TASK-0054C P2-b): a kernel one-shot
//! on the device-watchdog pair its owner declares and hands in, a waitset member beside the
//! IRQ endpoint.
//! A request the device never completes ends in `virtio-blk: timeout` from the watchdog's
//! frame — never from a receive deadline or a clock compare; the poll fallback (no IRQ line)
//! is bounded by the same timer. Split out of `mmio.rs` (structure gate); a child module so
//! the driver's fields stay private.
//! OWNERS: @storage
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`blk: watchdog on`, `blk: irq completion on`, every
//!   block-plane read in the ladder).

use super::{emit_line, VirtioBlkMmio, VirtioError};

impl VirtioBlkMmio {
    /// Completion wait bounded by the device watchdog (2 s): IRQ-blocked when a line is bound
    /// (marker-honest), yield-poll otherwise — the bound is the watchdog's frame either way,
    /// never a receive deadline (TASK-0054C P2-b). Self-terminating.
    pub(super) fn wait_done(&self, slot_idx: usize) -> Result<(), VirtioError> {
        const COMPLETION_BOUND_NS: u64 = 2_000_000_000;
        if let Some(t) = self.watchdog.borrow_mut().as_mut() {
            t.arm_in(COMPLETION_BOUND_NS);
        }
        let result = self.wait_done_turns(slot_idx);
        if let Some(t) = self.watchdog.borrow_mut().as_mut() {
            t.disarm();
        }
        result
    }

    fn wait_done_turns(&self, slot_idx: usize) -> Result<(), VirtioError> {
        loop {
            self.drain()?;
            if self.state.borrow().slots[slot_idx].done {
                if self.irq_ep != 0 {
                    self.ack_irq();
                    if !self.irq_logged.get() {
                        self.irq_logged.set(true);
                        emit_line("blk: irq completion on");
                    }
                }
                return Ok(());
            }
            if self.irq_ep != 0 {
                // Block until the device interrupt or the watchdog instead of burning
                // scheduler round-trips per sector.
                let woke = match self.wait_ws.borrow().as_ref() {
                    Some(ws) => ws.wait().ok(),
                    None => {
                        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
                        let mut buf = [0u8; 16];
                        let _ = nexus_abi::ipc_recv_v1(
                            self.irq_ep,
                            &mut hdr,
                            &mut buf,
                            nexus_abi::IPC_SYS_TRUNCATE,
                            0,
                        );
                        Some(0)
                    }
                };
                // The watchdog member woke us: a fire (timeout), or the kernel's EOF latch
                // (spurious — the drain says which); either way re-arm the IRQ and re-check.
                if woke == Some(1) && self.watchdog.borrow_mut().as_mut().is_some_and(|t| t.drain())
                {
                    emit_line("virtio-blk: timeout");
                    return Err(VirtioError::Unsupported);
                }
                self.ack_irq();
            } else {
                if !self.poll_logged.get() {
                    self.poll_logged.set(true);
                    emit_line("blk: poll fallback (no irq)");
                }
                let _ = nexus_abi::yield_();
                if self.watchdog.borrow_mut().as_mut().is_some_and(|t| t.drain()) {
                    emit_line("virtio-blk: timeout");
                    return Err(VirtioError::Unsupported);
                }
            }
        }
    }
}
