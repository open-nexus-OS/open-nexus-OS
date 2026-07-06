// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: queryd — the QuerySpec v1 service skeleton: hosts `nexus-query`
//! behind the capnp wire contract (`tools/nexus-idl/schemas/queryspec.capnp`,
//! frame = `[opcode u8][capnp]`), derives per-app namespaces from caller
//! identity (nothing on the wire selects one), and gates every frame on
//! `nexus.permission.QUERY` (fail-closed). Host-loopback tested; boot wiring
//! and the statefsd-journal Kv ride with Phase 6 (TASK-0080C).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/loopback.rs (round-trip, namespace isolation, denial)
//! ADR: docs/dev/dsl/db-queries.md

#![forbid(unsafe_code)]

mod server;
mod wire;

pub use server::{Caps, DenyAll, QuerydServer, StaticCaps};
pub use wire::{OP_CREATE_TABLE, OP_DELETE, OP_PUT, OP_QUERY};
