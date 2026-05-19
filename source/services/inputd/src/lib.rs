// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `inputd` service crate for bounded merge/config/route logic in TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod config;
mod error;
mod live_push;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod os_lite;
mod route;
mod service;
mod types;
mod visible_contract;
mod wire;

pub use config::{InitialPointerPosition, InputdConfig, QueueCapacity};
pub use error::InputdError;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub use os_lite::service_main_loop;
pub use pointer_state::{
    AbsoluteAxisCalibration, DisplayRect, PointerAxis, PointerExtent, PointerPosition,
    PointerSpace, PointerState, PointerStateError, PointerTransform,
};
pub use route::RouteTarget;
pub use service::InputdService;
pub use types::{ImeHook, InputDispatch};
pub use visible_contract::{
    visible_display_space, visible_display_start_position, visible_hover_target_contains,
    visible_pointer_transform, visible_route_space, LIVE_POINTER_DENOMINATOR,
    LIVE_POINTER_MAX_OUTPUT, LIVE_POINTER_NUMERATOR, LIVE_POINTER_THRESHOLD,
    VISIBLE_INPUT_CURSOR_END_X, VISIBLE_INPUT_CURSOR_END_Y, VISIBLE_INPUT_CURSOR_START_X,
    VISIBLE_INPUT_CURSOR_START_Y, VISIBLE_INPUT_LEFT_SQUARE_X, VISIBLE_INPUT_LEFT_SQUARE_Y,
    VISIBLE_INPUT_PROOF_HEIGHT, VISIBLE_INPUT_PROOF_WIDTH, VISIBLE_INPUT_RIGHT_SQUARE_X,
    VISIBLE_INPUT_RIGHT_SQUARE_Y, VISIBLE_INPUT_SQUARE_SIZE,
};
pub use wire::{decode_wire_batch, WireBatchReject};

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("inputd: ready");
}
