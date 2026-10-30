// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The memory authority's (`memd`) declared slots (TASK-0333 6e-2/6e-3, RFC-0100 §3,
//! ADR-0086) — split out of `slots/mod.rs` by the structure gate when the level's fan-out legs
//! joined the table.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace — the numbers are the contract between init and
//! memd

use super::SlotPair;

/// memd's own server endpoint (the lifecycle authority's band pushes, the harness's
/// status and drill requests arrive here).
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
/// The shared CAP_MOVE reply inbox (the lifecycle authority answers a kill order on it).
pub const REPLY: SlotPair = SlotPair::new(6, 5);
/// The kill order: `OP_MEM_KILL` to the lifecycle authority, which orders the parent.
pub const ABILITYMGR: SlotPair = SlotPair::new(7, REPLY.recv);
/// The kernel's pressure endpoint (ADR-0085): the SEND half is bound with
/// `mem_pressure_bind`, the RECV half is the loop's second waitset member.
pub const PRESSURE: SlotPair = SlotPair::new(9, 8);
/// The level told to gpud (TASK-0333 6e-3): a one-way push, never answered.
pub const GPUD: SlotPair = SlotPair::new(10, REPLY.recv);
/// The level told to windowd (6e-3), which tells every app window: a one-way push.
pub const WINDOWD: SlotPair = SlotPair::new(11, REPLY.recv);
/// The RNG service (TASK-0333 6f-2, ADR-0088): the pager asks it to seed the kernel's swap key
/// when it first needs to swap (the key goes from rngd to the kernel, never through memd).
pub const RNGD: SlotPair = SlotPair::new(12, REPLY.recv);
/// The state store (TASK-0333 6f-3): memd's kill journal, one record per kill it ordered.
pub const STATEFSD: SlotPair = SlotPair::new(13, REPLY.recv);
