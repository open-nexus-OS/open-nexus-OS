// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap subsystem — split from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable

pub(crate) mod helpers;
pub(crate) mod orchestrator;
pub(crate) mod policyd;
pub(crate) mod responder;
pub(crate) mod route_builder;
pub(crate) mod spawn;
pub(crate) mod types;

pub(crate) use types::{BootstrapState, CtrlChannel};
