// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Probe-time GPU capability proof + scanout policy enforcement,
//! split out of `backend/mod.rs::probe` (structure gate). The virgl build runs
//! the 3D self-test cascade (context → SUBMIT_3D → RT clear) and emits exactly
//! one of `gpud: virgl ready` / `gpud: cpu fallback`; the pipeline library and
//! the draw, gradient and layer-composite readbacks follow when windowd opens its
//! context with its library (TASK-0215 5c-3 P3, `virgl_context.rs`). Then BOTH
//! builds apply `scanout_policy`: a GL device is driven through the GL render
//! target or the service exits loudly (TASK-0324 P0).
//! OWNERS: @gpu
//! STATUS: Functional

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::VirtioGpuBackend;
use crate::error::GpuDriverError;
#[allow(unused_imports)]
use crate::markers::{GPUD_CPU_FALLBACK, GPUD_VIRGL_READY};

impl VirtioGpuBackend {
    /// Virgl capability detection + self-test cascade (virgl build), CPU
    /// fallback marker otherwise. Exactly one of the two markers, never both.
    pub(super) fn probe_gpu_capabilities(&mut self) {
        // Virgl capability detection.
        // When the `virgl` feature is compiled in, probe for GPU acceleration.
        // On QEMU with `-device virtio-gpu-pci,virgl=on`, the device reports
        // virgl capability in its config space. Without the feature or when
        // virgl is not detected, CPU fallback is used for blur operations.
        // `self.virgl_capable` is set during `probe_os()` feature negotiation:
        // true iff the device offered (and we acked) VIRTIO_GPU_F_VIRGL. Create
        // the 3D context; emit `virgl ready` ONLY on success, `cpu fallback`
        // otherwise — exactly one of the two markers, never both.
        #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
        {
            if self.virgl_capable && self.create_virgl_context().is_ok() {
                let _ = nexus_abi::debug_println(GPUD_VIRGL_READY);
                // Validate the SUBMIT_3D wire format against virglrenderer.
                if self.submit3d_selftest().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_SUBMIT3D_OK);
                } else {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_SUBMIT3D_FAIL);
                }
                // Validate the draw-state path (resource → surface → fb → clear).
                if self.virgl_rt_clear_test().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_RT_CLEAR_OK);
                } else {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_RT_CLEAR_FAIL);
                }
                // The draws need a pipeline library — windowd's, taken when windowd opens its
                // context before its attach (TASK-0215 5c-3 P3): the readback proofs of the
                // pipelines run there (`virgl_context.rs`). Until then the draw is unproven and
                // the scanout policy defers the decision to the first scanout.
            } else {
                // `virgl_capable` is NOT cleared here. It records what the DEVICE
                // offered and we acked during feature negotiation — a fact — and
                // overwriting it with the outcome of one probe is the same
                // conflation that made this task necessary. Every consumer gates
                // on `virgl_draw_ok` (or the context id) as well, so leaving the
                // negotiated fact intact changes no behaviour except that a later
                // proof is still possible.
                let _ = nexus_abi::debug_println(GPUD_CPU_FALLBACK);
            }
        }
        #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
        {
            // Host fallback: no virgl possible, always CPU fallback.
            // Marker emitted via println! (host) or debug_println (OS).
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            let _ = nexus_abi::debug_println(GPUD_CPU_FALLBACK);
            #[cfg(not(all(feature = "os-lite", target_os = "none")))]
            let _ = GPUD_CPU_FALLBACK;
        }
    }

    /// Scanout policy (TASK-0324 P0): a GL device is driven through the GL
    /// render target or not at all — the 2D plane-row scanout is black on
    /// every GL display backend. Decided once, here, in both builds.
    pub(super) fn enforce_scanout_policy(&self) -> Result<(), GpuDriverError> {
        #[cfg(feature = "virgl")]
        let draw_ok = self.virgl_draw_ok;
        #[cfg(not(feature = "virgl"))]
        let draw_ok = false;
        match super::scanout_policy::scanout_path(self.gl_device, draw_ok, cfg!(feature = "virgl"))
        {
            Ok(_) => Ok(()),
            Err(super::scanout_policy::ScanoutFatal::GlDeviceWithoutVirglFeature) => {
                let _ = nexus_abi::debug_println(crate::markers::GPUD_FAIL_GL_DEVICE_NEEDS_VIRGL);
                Err(GpuDriverError::Unsupported)
            }
            Err(super::scanout_policy::ScanoutFatal::GlDrawUnavailable) => {
                // `needed_now = false`: nobody has asked for a scanout yet, and
                // on QEMU's GTK backend the window may not be realized — the
                // same transient the display mode already refuses to trust
                // (RFC-0074). The rule decides, in one tested place.
                use super::scanout_policy::{draw_verdict, DrawVerdict};
                match draw_verdict(self.gl_device, draw_ok, false) {
                    DrawVerdict::DeferToFirstNeed => {
                        let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_DRAW_DEFERRED);
                        Ok(())
                    }
                    _ => {
                        let _ =
                            nexus_abi::debug_println(crate::markers::GPUD_FAIL_GL_DRAW_UNAVAILABLE);
                        Err(GpuDriverError::Unsupported)
                    }
                }
            }
        }
    }
}
