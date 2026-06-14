// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: GpudContract — simulated gpud service for Chain-Tests.
//! OWNERS: @tools-team
//!
//! Simulated gpud service.
//!
//! Emits only the real markers observed from the gpud service:
//!   gpud: virtio-gpu probed → gpud: ready → gpud: handoff attach ack → gpud: display ready

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

/// Simulated gpud service.
///
/// Emits only the real markers observed from the gpud service:
///   gpud: virtio-gpu probed → gpud: ready
///
/// Cursor architecture: hardware cursor upload (OP_UPLOAD_CURSOR) is disabled
/// due to QEMU virtio-gpu quirk (UPDATE_CURSOR corrupts scanout resource).
/// Cursor is rendered via BlendCursor embedded in every frame CommandBuffer.
///
/// IDL: source/drivers/gpud/idl/gpud.capnp
pub struct GpudContract {
    /// Whether to simulate receiving a windowd handoff VMO.
    pub handoff: bool,
    /// Whether the GPU present pipeline (blur/flush) markers fire.
    /// True for with_handoff_and_cursor(); no-op for probe_only/with_handoff.
    pub gpu_present: bool,
    /// Number of animation frames to simulate after the initial present.
    /// Each frame = transfer_to_host + resource_flush marker pair (120Hz pacer).
    /// 0 = no animation frames (non-animated display path).
    pub animation_frames: usize,
    id: Option<ServiceId>,
}

impl GpudContract {
    pub fn probe_only() -> Self {
        Self { handoff: false, gpu_present: false, animation_frames: 0, id: None }
    }

    pub fn with_handoff() -> Self {
        Self { handoff: true, gpu_present: false, animation_frames: 0, id: None }
    }

    /// Software cursor path (BlendCursor embedded in CB, no OP_UPLOAD_CURSOR).
    pub fn with_handoff_and_cursor() -> Self {
        Self { handoff: true, gpu_present: true, animation_frames: 0, id: None }
    }

    /// Animation path: emits `n` GPU frame pairs after handoff.
    /// Models the 120Hz kernel-timer-driven present loop:
    ///   timer fires → windowd tick() → PresentDamage(CB) → gpud renders
    ///   → transfer_to_host + resource_flush → present ack → rearm timer.
    /// `n` should be ≥ 8 to cover the spring settling window (~67ms at 120Hz).
    pub fn with_animation_frames(n: usize) -> Self {
        Self { handoff: true, gpu_present: true, animation_frames: n, id: None }
    }
}

impl Contract for GpudContract {
    fn service_name(&self) -> &'static str {
        "gpud"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id =
            self.id.ok_or_else(|| ContractError::new(ServiceId(0), "gpud: service id not set"))?;

        bus.emit_marker(id, "gpud: virtio-gpu probed");
        bus.emit_marker(id, "gpud: ready");

        if self.handoff {
            bus.emit_marker(id, "gpud: recv OP_SET_FRAMEBUFFER_VMO");
            bus.emit_marker(id, "gpud: set_scanout ok");
            bus.emit_marker(id, "gpud: handoff attach ack");
            // Software cursor: cursor rendered via BlendCursor in every frame CB.
            // No OP_UPLOAD_CURSOR / "gpud: cursor uploaded" — hardware path disabled.
            bus.emit_marker(id, "gpud: cursor on");
            bus.emit_marker(id, "gpud: display ready (w=1280, h=800)");
            // Phase 2+7: GPU blur+present pipeline markers (fire when CB is processed)
            if self.gpu_present {
                // Present-chain hops (graphical-output bisection). String-identical
                // to source/drivers/gpud/src/markers.rs GPUD_CHAIN_*; a real run
                // shows how far a frame gets, this spec verifies the hop order.
                bus.emit_marker(id, "gpud: chain G1 recv present-damage");
                bus.emit_marker(id, "gpud: chain G2 parse ok");
                bus.emit_marker(id, "gpud: chain G3 exec ok (commands applied)");
                bus.emit_marker(id, "gpud: chain G4 scanout ok (frame presented)");
                bus.emit_marker(id, "gpud: backend submit ok");
                bus.emit_marker(id, "gpud: present scanout damage ok");
                bus.emit_marker(id, "gpud: transfer_to_host ok");
                bus.emit_marker(id, "gpud: resource flush ok");
                // Phase 7: pacer-driven frame ack
                bus.emit_marker(id, "gpud: present ack");
                // Animation frames: each pair = one 120Hz timer tick rendering the
                // animated CB (BlitSurface + BlurBackdrop + FillSdfRoundedRect +
                // BlendCursor), per gpud.capnp PresentDamage path.
                for _ in 0..self.animation_frames {
                    bus.emit_marker(id, "gpud: transfer_to_host ok");
                    bus.emit_marker(id, "gpud: resource flush ok");
                }
            }
        }

        Ok(())
    }
}
