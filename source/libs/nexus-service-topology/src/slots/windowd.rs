// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor's (`windowd`) declared slots (TASK-0324 P4a) — split out of
//! `slots/mod.rs` by the structure gate when the USB host controller's table joined it
//! (TASK-0328 U1).
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace — the numbers are the contract between init and
//! the compositor

use super::SlotPair;

/// windowd's own server endpoint (clients send here).
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
/// The shared CAP_MOVE reply inbox for its outbound calls.
pub const REPLY: SlotPair = SlotPair::new(8, 7);
/// Present/attach/cursor handoff to gpud (its own response endpoint).
pub const GPUD: SlotPair = SlotPair::new(5, 6);
/// Dynamic Apps menu (`OP_LIST_APPS`).
pub const BUNDLEMGRD: SlotPair = SlotPair::new(9, REPLY.recv);
/// Greeter/login relay.
pub const SESSIOND: SlotPair = SlotPair::new(10, REPLY.recv);
/// Theme GET/SET persistence.
pub const SETTINGSD: SlotPair = SlotPair::new(11, REPLY.recv);
/// `OP_LAUNCH` from the shell (abilitymgr answers on its own endpoint).
pub const ABILITYMGR: SlotPair = SlotPair::new(12, 13);
/// Focus relay `OP_SET_FOCUS`.
pub const IMED: SlotPair = SlotPair::new(14, REPLY.recv);
/// Settings push channel (RFC-0083): RECV half, drained per frame.
pub const WATCH_RECV: u32 = 0x40;
/// Settings push channel: SEND half, cloned per `OP_WATCH`.
pub const WATCH_SEND: u32 = 0x41;
/// Session push channel (TASK-0324 P7-c): RECV half — sessiond's state pushes land here,
/// a waitset member of the compositor loop (no probe cadence, no login poll).
pub const SESSION_WATCH_RECV: u32 = 0x42;
/// Session push channel: SEND half, cloned once and moved with sessiond's `OP_WATCH`.
pub const SESSION_WATCH_SEND: u32 = 0x43;
