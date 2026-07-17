// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Device bring-up: the OS-side virtio-gpu probe (feature negotiation, queue +
//! cursor-queue setup, scanout discovery) and the GPU ring-buffer IRQ binding.

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::transport::{
    read_reg, write_reg, GPU_CMD_VA, GPU_CURSOR_CMD_VA, GPU_CURSOR_QUEUE_VA, GPU_CURSOR_RESP_VA,
    GPU_QUEUE_VA, GPU_RESP_VA,
};
use super::virtqueue::{CtrlQueue, CTRL_QUEUE_INDEX, CURSOR_QUEUE_INDEX, RING_SLOTS};
use super::VirtioGpuBackend;
use crate::error::GpuDriverError;
use crate::protocol;

impl VirtioGpuBackend {
    pub(crate) fn probe_os(&mut self) -> Result<(), GpuDriverError> {
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_MAGIC_VALUE)
            != protocol::VIRTIO_MMIO_MAGIC
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_ID)
            != protocol::VIRTIO_GPU_DEVICE_ID
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 0);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 1 | 2);

        // Feature negotiation. The non-virgl build acknowledges no features
        // (the long-proven 2D path); the virgl build reads the device feature
        // bits and acks VIRGL + CONTEXT_INIT + VERSION_1 when the device (a
        // `virtio-gpu-gl` model) offers them, enabling the 3D command path.
        #[cfg(feature = "virgl")]
        {
            self.negotiate_features_virgl();
        }
        #[cfg(not(feature = "virgl"))]
        {
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 0);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, 0);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 1);
            write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, 0);
        }

        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 8);
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS) & 8 == 0 {
            // FEATURES_OK refused: the device rejected our negotiated set.
            #[cfg(feature = "virgl")]
            {
                self.virgl_capable = false;
            }
            return Err(GpuDriverError::CommandRejected);
        }
        // Control queue: multi-slot ring (batches a whole present, completes once).
        let ctrlq = CtrlQueue::new(
            self.mmio_base,
            CTRL_QUEUE_INDEX,
            GPU_QUEUE_VA,
            GPU_CMD_VA,
            GPU_RESP_VA,
            RING_SLOTS,
        )?;
        self.ctrlq = Some(ctrlq);
        // Cursor virtqueue (index 1) — hardware-cursor overlay path. Best-effort:
        // if it can't be set up, cursor falls back and 2D still works. Single-slot
        // (cursor commands are submitted one at a time, no batching).
        if let Ok(cursorq) = CtrlQueue::new(
            self.mmio_base,
            CURSOR_QUEUE_INDEX,
            GPU_CURSOR_QUEUE_VA,
            GPU_CURSOR_CMD_VA,
            GPU_CURSOR_RESP_VA,
            1,
        ) {
            self.cursorq = Some(cursorq);
        }
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 4);
        Ok(())
    }

    /// Bind this GPU's virtio-mmio completion IRQ (PLIC source) to `irq_ep` so the
    /// command-completion wait can BLOCK on the interrupt instead of busy-polling
    /// the used-ring. Wires both the control and cursor queues — they share the one
    /// device IRQ. Best-effort: on a denied/failed bind the queues keep `irq_ep = 0`
    /// and the legacy spin+yield wait stays in force, so a wrong IRQ never hangs a
    /// present, it only forgoes the reactive wake. Returns true when bound.
    pub(crate) fn bind_gpu_irq(&mut self, irq_num: u32, irq_ep: u32) -> bool {
        if nexus_abi::irq_bind(irq_num, irq_ep).is_err() {
            return false;
        }
        if let Some(q) = self.ctrlq.as_mut() {
            q.set_gpu_irq(irq_num, irq_ep);
        }
        if let Some(q) = self.cursorq.as_mut() {
            q.set_gpu_irq(irq_num, irq_ep);
        }
        true
    }
}
