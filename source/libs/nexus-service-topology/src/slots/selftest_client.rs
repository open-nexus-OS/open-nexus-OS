// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The proof harness's (`selftest-client`) declared slots — split out of `slots.rs`
//! by the structure gate (TASK-0054C P2-b): the harness reaches every service, so its table
//! is the longest, and it is the one consumer that is not a service.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal — the numbers are the contract between init and the harness

//! selftest-client, the proof harness (TASK-0324 P4f-5).
//!
//! Before P4f-5 every number here was TRANSFER ORDER inside init's selftest arm: the harness
//! hardcoded eight of them in `route_with_retry`, the reply inbox in eight files and rngd in
//! three, and the arm's comments ("LAST in this arm so every earlier slot keeps its historical
//! number") were the only contract. Init now pins every leg here BEFORE the harness first runs,
//! and the harness checks init's routing answers against this table. The numbers are the
//! effective ones of the order-based layout, so the move is behaviour-neutral by construction.
use crate::SlotPair;

/// The shared CAP_MOVE reply inbox (probes clone its SEND half into every request).
pub const REPLY: SlotPair = SlotPair::new(0x18, 0x17);
/// vfsd (`SELFTEST: vfs …`).
pub const VFSD: SlotPair = SlotPair::new(3, 4);
/// packagefsd (`pkg:/` reads).
pub const PACKAGEFSD: SlotPair = SlotPair::new(5, REPLY.recv);
/// policyd (allow/deny, ABI filter, audit probes).
pub const POLICYD: SlotPair = SlotPair::new(7, 8);
/// bundlemgrd (list, volume status, malformed frame).
pub const BUNDLEMGRD: SlotPair = SlotPair::new(9, 0x0A);
/// updated (OTA stage/switch/commit).
pub const UPDATED: SlotPair = SlotPair::new(0x0B, 0x0C);
/// samgrd (registry register/lookup).
pub const SAMGRD: SlotPair = SlotPair::new(0x0D, 0x0E);
/// execd (spawn, crash, restart probes).
pub const EXECD: SlotPair = SlotPair::new(0x0F, 0x10);
/// keystored (device key, signing).
pub const KEYSTORED: SlotPair = SlotPair::new(0x11, 0x12);
/// statefsd (CRUD, persistence, encryption). Answers on the harness' CAP_MOVE reply inbox
/// since TASK-0054C P2-f: statefsd replies only to a sender that moved a reply cap, so slot
/// 0x14 — a RECV cap on statefsd's own response endpoint — is gone with the sharing.
pub const STATEFSD: SlotPair = SlotPair::new(0x13, REPLY.recv);
/// logd (append/query, the `nexus-log` sink).
pub const LOGD: SlotPair = SlotPair::new(0x15, 0x16);
/// inputd (answers on the reply inbox).
pub const INPUTD: SlotPair = SlotPair::new(0x19, REPLY.recv);
/// netstackd's facade — it answers every RPC on the caller's CAP_MOVE cap only.
pub const NETSTACKD: SlotPair = SlotPair::new(0x1A, REPLY.recv);
/// dsoftbusd (local IPC probe).
pub const DSOFTBUSD: SlotPair = SlotPair::new(0x1C, 0x1D);
/// rngd (entropy probe).
pub const RNGD: SlotPair = SlotPair::new(0x1E, 0x1F);
/// timed (coalescing probe).
pub const TIMED: SlotPair = SlotPair::new(0x20, 0x23);
/// metricsd (counters, retention, spans).
pub const METRICSD: SlotPair = SlotPair::new(0x21, 0x22);
/// pinched (compute broker; re-resolved by name after a restart, ADR-0057).
pub const PINCHED: SlotPair = SlotPair::new(0x24, 0x25);
/// settingsd (watch probe).
pub const SETTINGSD: SlotPair = SlotPair::new(0x26, 0x27);
/// ingressd — the gateway answers on the caller's CAP_MOVE cap.
pub const INGRESSD: SlotPair = SlotPair::new(0x28, REPLY.recv);
/// imed (the IME authority's negative probe).
pub const IMED: SlotPair = SlotPair::new(0x2A, 0x2B);
/// imed's on-screen-keyboard endpoint — a clone made for the harness; replies ride the
/// probe's own minted channel.
pub const IMED_OSK: SlotPair = SlotPair::new(0x2C, REPLY.recv);
/// bootctld (reset/target lane; a successful reset never answers).
pub const BOOTCTLD: SlotPair = SlotPair::new(0x2D, REPLY.recv);
/// virtioblkd's request endpoint for the deny probes — NOT a block-plane grant: the
/// probes prove that a sender without one is refused.
pub const VIRTIOBLKD: SlotPair = SlotPair::new(0x2E, REPLY.recv);
/// The read-only device tree the harness reads its boot mode/profile from (`/chosen`).
pub const DEVICE_TREE: u32 = 0x31;
/// Timer-notify endpoint (TASK-0054C P2-b): the harness's FAIL witness for an event that
/// must arrive, and its settle before a verdict — never a recv deadline.
pub const TIMER: SlotPair = SlotPair::new(0x36, 0x35);
