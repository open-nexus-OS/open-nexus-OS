// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Backend trait and implementations (CPU mock, virtio-gpu, null, trace).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

pub mod cpu_mock;
/// The board display controller's host half (EDID, mode policy, register model — D2).
pub mod dc;
pub mod error;
/// RFC-0101 Phase 2 (TASK-0329 G1): the GPU device's host half — the firmware image, the MIPS
/// boot, the start sequence, the interface structs, the command buffers, the page tables.
pub mod gpu;
pub mod traits;
pub mod types;
/// RFC-0101: the v2 device contract (contexts, buffers, batches, the timeline) and the
/// scanout contract (the plane ring, the frame clock, the pointer plane), with the mock.
pub mod v2;
