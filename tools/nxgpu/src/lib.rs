// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU program tool's library (host only, ADR-0072, TASK-0215 5c-2 P2b): WGSL
//! parsed and validated by naga, then lowered into the system's IR
//! (`nexus_gfx::shader::ir`) — naga's IR never leaves this crate. A fragment program's colour
//! output is written through the epilog the pipeline's blend names (the GPU has no blend unit,
//! so blending is program code linked after the main part).
//! OWNERS: @gpu @runtime
//! STATUS: Experimental — the compositor's subset: straight-line fragment programs with
//!   location inputs, immediates (push constants), 2D textures and samplers, overrides fixed per
//!   pipeline (a select on one folds), arithmetic, the common math functions, constructors,
//!   swizzles and component access; anything else is refused by name
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/lower.rs` — the compositor's WGSL programs lowered and run by the
//!   reference interpreter against the CPU rasterizer's formulas, the overrides' variants; the
//!   refusals. `tests/pipelines.rs` — the committed pipelines rebuilt byte for byte.

mod lower;
mod overrides;

pub use lower::lower_fragment;
pub use nexus_gfx::shader::ir::builder::Blend;
use nexus_gfx::shader::rogue::codegen::{self, CodegenError, Layout, Options, Program};

/// WGSL compiled for the board GPU: lowered with the pipeline's overrides, then the backend over
/// the setup programs' layout.
pub fn compile_fragment(
    src: &str,
    entry: &str,
    overrides: &[(&str, f64)],
    blend: Blend,
    fused_sample: bool,
) -> Result<Program, FrontendError> {
    let m = lower_fragment(src, entry, overrides, blend)?;
    codegen::compile(&m, Layout::BUNDLE, Options { fused_sample }).map_err(FrontendError::Codegen)
}

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
    /// An override value refused: a key naming no override or given twice, a value outside the
    /// override's type, an override with neither a value nor a literal default.
    Override(String),
    /// The lowered module broke a rule of the IR.
    Ir(nexus_gfx::shader::ir::IrError),
    /// The backend refused the module.
    Codegen(CodegenError),
}
