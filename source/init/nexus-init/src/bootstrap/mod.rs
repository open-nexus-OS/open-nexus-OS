// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap subsystem — split from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

pub(crate) mod blk_plane;
pub(crate) mod core_plane;
pub(crate) mod diag;
pub(crate) mod distribute;
pub(crate) mod endpoints;
pub(crate) mod fault_fixture;
pub(crate) mod gateway_route;
pub(crate) mod handshake;
pub(crate) mod helpers;
pub(crate) mod labels;
pub(crate) mod orchestrator;
pub(crate) mod persist;
pub(crate) mod policyd;
pub(crate) mod policyd_slots;
pub(crate) mod respawn;
pub(crate) mod responder;
pub(crate) mod resume;
pub(crate) mod route_builder;
pub(crate) mod route_provision;
pub(crate) mod settings_watch_route;
pub(crate) mod spawn;
pub(crate) mod supervision;
pub(crate) mod types;
pub(crate) mod volume_spawn;
pub(crate) mod wiring;

pub(crate) use types::{BootstrapState, CtrlChannel};
