// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The device's completion wait (TASK-0314, TASK-0333 6f-3, ADR-0089) — reactive end to
//! end: a request waits for the device's interrupt and for nothing else. No watchdog period, no
//! poll, no deadline: a host that stalls the emulated device for seconds only delays the request
//! (until 6f-3 a 2 s watchdog gave requests up, their slots stayed busy, and four stalls left a
//! device that refused every later request — 2026-10-29). A device that FAILED says so itself: a
//! configuration-change interrupt with `DEVICE_NEEDS_RESET` in its status — the request, and
//! every later one, end in an error, with one line. The line is bound when the device opens,
//! before its first request (the warm-up read included), so no request ever polls. Split out of
//! `mmio.rs` (structure gate); a child module so the driver's fields stay private.
//! OWNERS: @storage
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the interrupt's meaning and the slot ownership host-tested in
//!   `tests/inflight.rs`; QEMU (`blk: irq bound`, `blk: irq completion on`, every block-plane
//!   read in the ladder).

use super::{emit_line, VirtioBlkMmio, VirtioError};
use crate::inflight::DeviceSays;

impl VirtioBlkMmio {
    /// Waits until the request in `slot_idx` is back on the used ring — woken by the device's
    /// interrupt, never by a clock — or the device says it failed.
    pub(super) fn wait_done(&self, slot_idx: usize) -> Result<(), VirtioError> {
        let mut refused = false;
        loop {
            self.drain()?;
            if self.state.borrow().inflight.is_done(slot_idx) {
                if !self.irq_logged.get() {
                    self.irq_logged.set(true);
                    emit_line("blk: irq completion on");
                }
                return Ok(());
            }
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 16];
            match nexus_abi::ipc_recv_v1(
                self.irq_ep,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(_) => refused = false,
                // The kernel's last-sender latch ends one receive without a frame (the receive
                // clears it); a second refusal in a row is the endpoint gone — and with it the
                // only way the device can answer.
                Err(_) if !refused => refused = true,
                Err(_) => {
                    emit_line("virtio-blk: irq endpoint lost");
                    return Err(VirtioError::Unsupported);
                }
            }
            if self.ack_irq() == DeviceSays::Failed {
                self.failed.set(true);
                emit_line("virtio-blk: device failed (needs reset)");
                return Err(VirtioError::Unsupported);
            }
        }
    }
}
