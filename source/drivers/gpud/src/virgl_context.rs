// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: QEMU's GPU as windowd opens its context on it (TASK-0215 5c-3 P3, RFC-0101 §7b): the
//! virtio GPU's one virgl context. Its capabilities name the host renderer's code target; an
//! open takes windowd's library — the virgl pipelines made from it (`virgl_pipelines.rs`) — and
//! proves them by readback on the device: a draw, a gradient, a layer composite (the probe ran
//! these at start on gpud's own library until 5c-3 P3). The context takes one library a boot; a
//! restarted windowd's identical library finds its pipelines standing. Lines on the stack.
//! OWNERS: @gpu @ui
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: the QEMU display lanes (`gpud: virgl library ok (client=windowd`, the readback
//!   proofs, `windowd: gpu context ok (`)

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_display_proto::gpu_context::{ContextOpened, GpuCapabilities};
use nexus_display_proto::{STATUS_OK, STATUS_REFUSED, STATUS_UNSUPPORTED};
use nexus_gfx::shader::tgsi;

use crate::backend::VirtioGpuBackend;
use crate::gpu_context::{refused, GpuService, NO_GPU};
use crate::markers;

impl GpuService for VirtioGpuBackend {
    fn capabilities(&self) -> GpuCapabilities {
        if self.virgl_ctx_id == 0 {
            return NO_GPU;
        }
        GpuCapabilities {
            status: STATUS_OK,
            max_contexts: 1,
            features: 0,
            code_target: tgsi::CODE_TARGET,
        }
    }

    fn open(&mut self, _client: u64, _priority: u8, library: &[u8]) -> ContextOpened {
        if self.virgl_ctx_id == 0 {
            return refused(STATUS_UNSUPPORTED);
        }
        let fresh = !self.pipelines.ready;
        match self.virgl_pipelines_take(library) {
            Ok(entries) => {
                if fresh {
                    let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_SHADER_OK);
                    self.prove_library_draws();
                }
                crate::diag::fmt_line(format_args!(
                    "gpud: virgl library ok (client=windowd ctx={} bytes={} entries={entries} fresh={fresh} draw_ok={})",
                    self.virgl_ctx_id,
                    library.len(),
                    self.virgl_draw_ok
                ));
                ContextOpened {
                    status: STATUS_OK,
                    ctx: self.virgl_ctx_id,
                    accepted: entries,
                    refused: 0,
                    first_refused: 0,
                }
            }
            Err(e) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_SHADER_FAIL);
                crate::diag::fmt_line(format_args!(
                    "gpud: FAIL virgl library (client=windowd bytes={} {e:?})",
                    library.len()
                ));
                refused(STATUS_REFUSED)
            }
        }
    }
}

impl VirtioGpuBackend {
    /// The readback proofs of the library's pipelines on the device, once a boot.
    fn prove_library_draws(&mut self) {
        // Full-pipeline draw proof with readback pixel verification: the gradient pipeline red
        // over a blue clear — the center pixel (BGRA bytes) tells exactly how far it got.
        match self.virgl_draw_selftest() {
            Ok([0, 0, 255, 255]) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_DRAW_OK);
                self.virgl_draw_ok = true;
            }
            Ok([255, 0, 0, 255]) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_DRAW_NOOP);
            }
            Ok(_) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_DRAW_MISMATCH);
            }
            Err(_) => {
                let _ = nexus_abi::debug_println("gpud: virgl draw submit fail");
            }
        }
        // Gradient proof: red at the top, blue at the bottom — interpolation and the
        // orientation of `uv` the programs assume.
        match self.virgl_gradient_selftest() {
            Ok(true) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_GRADIENT_OK);
            }
            Ok(false) => {
                let _ = nexus_abi::debug_println(markers::GPUD_VIRGL_GRADIENT_WRONG);
            }
            Err(_) => {
                let _ = nexus_abi::debug_println("gpud: virgl gradient submit fail");
            }
        }
        // G2: the layer compositor's primitive (the client-surface pipeline's textured layer
        // composited into a render target, read back).
        match self.virgl_composite_selftest() {
            Ok(true) => {
                let _ = nexus_abi::debug_println(markers::GPUD_LAYER_COMPOSITE_OK);
            }
            Ok(false) => {
                let _ = nexus_abi::debug_println(markers::GPUD_LAYER_COMPOSITE_OFF);
            }
            Err(_) => {
                let _ = nexus_abi::debug_println("gpud: virgl composite submit fail");
            }
        }
    }
}
