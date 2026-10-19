// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The frame planner (TASK-0215 5c-3 P4, RFC-0101 §8d): one compositor model for every
//! executor. windowd's presents bring the desktop's layers (`CompositeLayer` commands, back to
//! front); the planner keeps the last set with its scroll and transform overrides and turns it,
//! frame after frame, into the frame's steps — every layer as drawn, a glass layer's blurred
//! backdrop read from its cache or written into it (the destination-so-far key), and whether the
//! atlas's content must be uploaded first. The QEMU path's virgl executor and the board's GPU
//! executor draw the same plan — the board's through `board`, the plan lowered into its passes;
//! the CPU path's own recipe commands (`BlitSurface`, `BlurBackdrop`, `BlitAbsolute`) are not the
//! planner's.
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_plan.rs` (host); the QEMU lanes' composite markers
//! INVARIANTS: no allocation (the plan is the frame's own storage); a present without layers
//!   keeps the set; a scroll override lives until the next set, a transform override until
//!   windowd replaces it; a glass key folds every layer drawn below it, as drawn.

pub mod board;
pub mod glass;
pub mod layers;
pub mod plan;

pub use board::{
    blur_radius, Board, DrawSpec, GlassCount, PassKind, PassSpec, Pipe, Target, Tex, BLUR_RADII,
};
pub use glass::{CacheStep, GlassCache};
pub use layers::{FrameLayer, RetainedLayers, Taken, Transform, MAX_IDS, MAX_LAYERS};
pub use plan::{Frame, Glass, LayerStep, Planned};
