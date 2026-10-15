// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU programs (ADR-0072): written in WGSL, lowered into our IR, compiled per code
//! target into the pipeline library (RFC-0101 §7b). `ir` is the IR with its validator and
//! reference interpreter (5c-2 P2) — its interface is a program's reflection; `rogue` is the
//! board GPU's encoding and backend (P1, P3); `library` is the archive a client supplies and the
//! device validates (P4).
//! OWNERS: @ui @runtime @gpu
//! STATUS: Experimental
//! API_STABILITY: Unstable

pub mod ir;
pub mod library;
pub mod rogue;
