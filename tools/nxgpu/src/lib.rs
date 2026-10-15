// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU program tool's library (host only, ADR-0072, TASK-0215 5c-2 P2b): WGSL
//! parsed and validated by naga, then lowered into the system's IR
//! (`nexus_gfx::shader::ir`) — naga's IR never leaves this crate. A fragment program's colour
//! output is written through the epilog the pipeline's blend names (the GPU has no blend unit,
//! so blending is program code linked after the main part).
//! OWNERS: @gpu @runtime
//! STATUS: Experimental — the compositor's subset: straight-line fragment programs with
//!   location inputs, immediates (push constants), 2D textures and samplers, arithmetic, the
//!   common math functions, constructors, swizzles and component access; anything else is
//!   refused by name
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/lower.rs` — the compositor's WGSL programs lowered and run by the
//!   reference interpreter against the CPU rasterizer's formulas; the refusals.

mod lower;

pub use lower::lower_fragment;
pub use nexus_gfx::shader::ir::builder::Blend;

/// Why WGSL did not become an IR module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendError {
    /// The parser's message, with the source location rendered.
    Parse(String),
    /// naga's validator refused the module.
    Validate(String),
    /// No entry point of that name and stage.
    NoEntryPoint(String),
    /// A construct outside the subset, by name.
    Unsupported(&'static str),
    /// The lowered module broke a rule of the IR.
    Ir(nexus_gfx::shader::ir::IrError),
}
