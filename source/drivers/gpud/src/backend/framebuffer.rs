// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The shared framebuffer resource is gpud's (RFC-0098 C7, TASK-0251 P1). The memory a
//! scanout engine reads has to be made FOR that engine — inside its DMA reach, and for a display
//! controller without an MMU in one block (RFC-0098 C4) — and only the holder of the device's
//! capability can make it so. gpud makes it once, at the first grant, at the size of the one
//! layout (`nexus_display_proto::layout::RESOURCE_BYTES`), keeps it for its lifetime and hands
//! windowd clones (`crate::framebuffer_grant`). The virtio device reads it through its runs
//! (`vmo_create_for`); the board's display controller takes the contiguous kind with its driver
//! (TASK-0251 P2).
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU display lane (`gpud: framebuffer granted`, the attach that follows)

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::VirtioGpuBackend;

impl VirtioGpuBackend {
    /// The framebuffer object, made on first use for this device, then kept. `None` when the
    /// kernel refused it (named on the console; the grant then refuses too).
    pub(crate) fn make_framebuffer(&mut self) -> Option<u32> {
        if let Some(vmo) = self.framebuffer_vmo {
            return Some(vmo);
        }
        let len = nexus_display_proto::layout::RESOURCE_BYTES;
        match nexus_abi::vmo_create_for(self.device, len) {
            Ok(vmo) => {
                self.framebuffer_vmo = Some(vmo);
                Some(vmo)
            }
            Err(_) => {
                let _ =
                    nexus_abi::debug_println("gpud: FAIL framebuffer create (the kernel refused)");
                None
            }
        }
    }
}
