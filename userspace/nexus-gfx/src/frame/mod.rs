// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The frame planner (TASK-0215 5c-3 P4, RFC-0101 §8d): one compositor model for every
//! executor. windowd's presents bring the desktop's layers (`CompositeLayer` commands, back to
//! front) and the windows' transforms; the planner keeps the last set and turns it,
//! frame after frame, into the frame's steps — every layer as drawn, a glass layer's blurred
//! backdrop read from its cache or written into it (the destination-so-far key, and nothing
//! changed within its reach), whether the atlas's content must be uploaded first, and what the
//! frame changes on the screen (`damage`: the present's `Damage` rectangles, every layer drawn
//! otherwise than before, every glass blurred again — RFC-0101 §8, TASK-0215 5c-3 P5). The QEMU
//! path's virgl executor and the board's GPU executor draw the same plan — the board's through
//! `board`, the plan lowered into its passes, the slot's pass limited to what the slot lacks
//! (`damage::History`, buffer age); the CPU path's own recipe commands (`BlitSurface`,
//! `BlurBackdrop`, `BlitAbsolute`) are not the planner's.
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_plan.rs`, `tests/frame_motion.rs` (host); the QEMU lanes'
//!   composite markers
//! INVARIANTS: no allocation (the plan is the frame's own storage); a frame is planned only for a
//!   present (TASK-0338: a scroll or a move is a present — no executor draws a frame of its own);
//!   a present without layers keeps the set; a transform holds until a present carries another;
//!   a glass key folds every layer drawn below it, as drawn; a present that brings layers and
//!   names no damage is damage everywhere, one naming `TileRect::NONE` is the layers' own changes
//!   alone; a plane of unknown or forgotten age is redrawn whole — a frame never shows a stale
//!   pixel where the scene changed.

pub mod board;
pub mod chain;
pub mod damage;
pub mod glass;
pub mod layers;
pub mod merge;
pub mod occlude;
pub mod pieces;
pub mod plan;

pub use board::{
    blur_radius, blur_scale, Board, DrawSpec, GlassCount, PassKind, PassSpec, Pipe, Target, Tex,
    BLUR_RADII,
};
pub use damage::{Area, History, HISTORY};
pub use glass::{CacheStep, GlassCache, WriteCause};
pub use layers::{FrameLayer, RetainedLayers, Taken, Transform, MAX_IDS, MAX_LAYERS};
pub use plan::{Frame, Glass, LayerStep, Planned, Writes};
