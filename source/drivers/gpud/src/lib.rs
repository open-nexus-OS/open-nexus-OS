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
pub mod error;
pub mod markers;
pub mod protocol;

/// Virgl 3D command-stream encoders (Phase 2). Compiled for the `virgl`
/// feature build and for host unit tests; excluded from the 2D-only build so
/// it never adds dead code there.
#[cfg(any(test, feature = "virgl"))]
pub mod virgl;

/// GL-presented scanout (GPU compositor G0/G1): the displayed scanout is a
/// virgl render target presented by the host GL, fed by a GPU blit of the
/// CPU-composited VMO. Empty unless the virgl OS build is active.
#[cfg(feature = "virgl")]
pub mod gl_scanout;

/// GPU vector pipeline (G3/M1b-c): SDF gradient fills + soft drop shadows as
/// virgl fragment-shader passes. Empty unless the virgl OS build is active.
#[cfg(feature = "virgl")]
pub mod virgl_vector;

/// GPU layer compositor (G2): the CompositeLayer draw op (content texture +
/// transform + opacity + rounded mask + shadow), the OHOS/Fuchsia/Apple model.
#[cfg(feature = "virgl")]
pub mod virgl_composite;

/// CPU fallback for the vector pipeline (non-virgl 2D path).
pub mod cpu_vector;

#[cfg(all(feature = "os-lite", target_os = "none"))]
pub mod service;
