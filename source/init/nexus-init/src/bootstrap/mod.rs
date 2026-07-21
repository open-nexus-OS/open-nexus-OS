// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap subsystem — split from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

pub(crate) mod diag;
pub(crate) mod endpoints;
pub(crate) mod helpers;
pub(crate) mod orchestrator;
pub(crate) mod policyd;
pub(crate) mod responder;
pub(crate) mod resume;
pub(crate) mod route_builder;
pub(crate) mod route_provision;
pub(crate) mod spawn;
pub(crate) mod types;
pub(crate) mod wiring;

pub(crate) use types::{BootstrapState, CtrlChannel};
