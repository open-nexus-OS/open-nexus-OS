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

#[cfg(all(feature = "os-lite", target_os = "none"))]
pub mod service;
