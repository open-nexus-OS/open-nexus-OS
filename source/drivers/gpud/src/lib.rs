// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU driver service: virtio-gpu MMIO protocol + backend.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

pub mod backend;
/// Numeric UART diagnostics for one-shot failure paths (TASK-0309). Empty on
/// every build except the real OS one — its own `#![cfg]` gates the contents.
pub mod diag;
pub mod error;
/// TASK-0329 G1 (RFC-0101 Phase 2): the board GPU — its glue, window, firmware, GPU MMU,
/// interrupt and fence bridge (the files live beside the display backend's).
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[path = "backend/gpu/mod.rs"]
pub(crate) mod gpu;
pub mod markers;
pub mod protocol;

/// Virgl 3D command-stream encoders (Phase 2). Compiled for the `virgl`
/// feature build and for host unit tests; excluded from the 2D-only build so
/// it never adds dead code there.
#[cfg(any(test, feature = "virgl"))]
pub mod virgl;

/// GL-presented scanout (GPU compositor G0–G3): the displayed scanout is a
/// virgl render target presented by the host GL, the whole frame composed into
/// it each present. Empty unless the virgl OS build is active.
#[cfg(feature = "virgl")]
pub mod gl_scanout;

/// GL-scanout bring-up (G0): swapchain RTs + wallpaper texture + splash +
/// initial SET_SCANOUT. Split out of `gl_scanout.rs` (structure-gate).
#[cfg(feature = "virgl")]
pub mod gl_scanout_init;

/// The ONE readback authority (RFC-0093 §5): display truth is read through a dedicated
/// probe RT, never from the scanout.
#[cfg(feature = "virgl")]
pub mod gl_probe;

/// The QEMU path's pipelines (TASK-0215 5c-2 P6): the pipeline library's programs
/// created in the virgl context and bound per draw — every virgl draw goes
/// through it. Empty unless the virgl OS build is active.
#[cfg(feature = "virgl")]
pub mod virgl_pipelines;

/// GPU vector draws (G3): a layer's drop shadow and the gradient-filled rounded
/// rectangle, with the library's pipelines. Empty unless the virgl OS build is active.
#[cfg(feature = "virgl")]
pub mod virgl_vector;

/// GPU layer compositor (G2): the CompositeLayer draw op (content texture +
/// transform + opacity + rounded mask + shadow), a retained-layer compositor model.
#[cfg(feature = "virgl")]
pub mod virgl_composite;
mod virgl_composite_selftest;

/// CPU fallback for the vector pipeline (non-virgl 2D path).
pub mod cpu_vector;

/// gpud's frame clock (TASK-0324 P7-d): the one-shot timer pacing the self-presented phases.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod frame_clock;
/// The framebuffer grant and the attach that follows it (RFC-0098 C7, RFC-0093 §5).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod framebuffer_grant;
/// The present chain's hops (validate, execute, damage, scan out, trace).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod present_chain;
/// The ONE pixel readback (RFC-0095): the `OP_READBACK` request and the CPU half of the copy
/// (the 2D virtio path and the board's controller); host-tested.
#[cfg(any(test, all(feature = "os-lite", target_os = "none")))]
pub(crate) mod readback;
/// Reply encoding for the request loop (RFC-0093 §5).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod reply;
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub mod service;

/// No-alloc UART stat emitters for the service loop (present stats window,
/// deadline-NACK marker, handoff timing) — split out of `service.rs`.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) mod service_stats;
