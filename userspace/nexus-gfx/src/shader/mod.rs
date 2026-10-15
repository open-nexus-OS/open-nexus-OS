// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU programs (ADR-0072): written in WGSL, lowered into our IR, compiled per code
//! target into the pipeline library (RFC-0101 §7b). `ir` is the IR with its validator and
//! reference interpreter (5c-2 P2); `rogue` is the board GPU's instruction encoding (P1); the
//! backends and the library follow (P3–P4).
//! OWNERS: @ui @runtime @gpu
//! STATUS: Experimental (`rogue`); the module, library, function and reflection files are
//!   placeholders the library work (P4) fills
//! API_STABILITY: Unstable

pub mod function;
pub mod ir;
pub mod library;
pub mod module;
pub mod reflection;
pub mod rogue;
