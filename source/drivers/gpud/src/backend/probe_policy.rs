// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Probe-time GPU capability proof + scanout policy enforcement,
//! split out of `backend/mod.rs::probe` (structure gate). The virgl build runs
//! the 3D self-test cascade (context → SUBMIT_3D → RT clear → shaders → draw
//! readback → gradient → layer composite) and emits exactly one of
//! `gpud: virgl ready` / `gpud: cpu fallback`; then BOTH builds apply
//! `scanout_policy`: a GL device is driven through the GL render target or the
//! service exits loudly (TASK-0324 P0).
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
                }
                // Validate the draw-state path (resource → surface → fb → clear).
                if self.virgl_rt_clear_test().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_RT_CLEAR_OK);
                }
                // Validate TGSI shader creation (vertex + fragment).
                if self.virgl_shader_test().is_ok() {
                    let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_SHADER_OK);
                    // Full-pipeline draw proof with readback pixel verification.
                    // Solid-red FS over a blue clear: center pixel (BGRA bytes)
                    // tells us exactly how far the pipeline got.
                    match self.virgl_draw_selftest() {
                        Ok([0, 0, 255, 255]) => {
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_OK);
                            self.virgl_draw_ok = true;
                        }
                        Ok([255, 0, 0, 255]) => {
                            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_NOOP);
                        }
                        Ok(_) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_DRAW_MISMATCH);
                        }
                        Err(_) => {
                            let _ = nexus_abi::debug_println("gpud: virgl draw submit fail");
                        }
                    }
                    // M1a: GPU vector pipeline — per-pixel gradient proof.
                    match self.virgl_gradient_selftest() {
                        Ok(true) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_GRADIENT_OK);
                        }
                        Ok(false) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_VIRGL_GRADIENT_FLAT);
                        }
                        Err(_) => {
                            let _ = nexus_abi::debug_println("gpud: virgl gradient submit fail");
                        }
                    }
                    // G2: GPU layer compositor primitive proof (textured layer +
                    // rounded mask + opacity composited into an RT, readback).
                    match self.virgl_composite_selftest() {
                        Ok(true) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_OK);
                        }
                        Ok(false) => {
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_OFF);
                        }
                        Err(_) => {
                            let _ = nexus_abi::debug_println("gpud: virgl composite submit fail");
                        }
                    }
                }
            } else {
                self.virgl_capable = false;
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
                let _ = nexus_abi::debug_println(crate::markers::GPUD_FAIL_GL_DRAW_UNAVAILABLE);
                Err(GpuDriverError::Unsupported)
            }
        }
    }
}
