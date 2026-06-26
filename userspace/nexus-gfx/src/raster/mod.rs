// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: NexusGfx canonical software rasterizer — the single source of truth
//! for CPU pixel rendering. Every command the SDK defines is executed here, over
//! a borrowed [`Surface`]. The host reference backend ([`crate::backend::cpu_mock`])
//! and the live GPU driver's CPU/VMO fallback both call into these primitives, so
//! there is exactly one implementation of the fill/blur/blit/blend semantics
//! instead of three hand-synchronised copies.
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0067 (windowd↔compositor boundary; one rasterization SSOT)
//!
//! DESIGN: `no_std`, **allocation-free**, and `forbid(unsafe_code)`. Coverage
//! math is the shared `nexus-sdf` fixed-point AA (so edges match the GPU shaders);
//! the blur passes take caller-provided scratch slices so the live driver never
//! allocates on the per-frame path. The colour model is straight-alpha BGRA8888;
//! premultiplied sprites use [`blend_premultiplied`].

#![forbid(unsafe_code)]

mod blend;
mod blit;
mod blur;
mod fill;
mod surface;

pub use blend::{blend_over, blend_over_px, blend_premultiplied, blend_premultiplied_px};
pub use blit::{blit_from, blit_within, blit_within_blend};
pub use blur::{blur_box, blur_gaussian, saturate};
pub use fill::{
    drop_shadow, fill_gradient_aa, fill_rect_solid, fill_rounded_aa, fill_rounded_solid,
};
pub use surface::{Surface, BYTES_PER_PIXEL};

/// Why a rasterizer primitive could not run. The only failure mode is a
/// caller-supplied scratch slice that is too small for the requested region —
/// every other path clamps to the surface bounds and silently no-ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterError {
    /// A `scratch_row` / `scratch_col` slice was smaller than the region needs.
    ScratchTooSmall,
}
