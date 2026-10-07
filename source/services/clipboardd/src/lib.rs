// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `clipboardd` — the ONE clipboard authority (RFC-0094, ADR-0070,
//! TASK-0067). Text items live in a bounded newest-first history
//! ([`history::History`]); the rules for who may do what live in
//! [`gate`]; [`answer`] turns one request (bytes + the kernel-attributed
//! sender) into one reply, so the host tests drive exactly the code the
//! service loop runs.
//!
//! WHO MAY DO WHAT (the gate, deny-by-default):
//! - WRITE: every route holder. The route is the capability — an app holds it
//!   only with `nexus.permission.CLIPBOARD` in its manifest, a service only by
//!   a declared topology edge. (The background-write model of the mobile
//!   reference; a stricter background rule is TASK-0087's, with flavors.)
//! - READ (the paste): the focused window's owner, the shell (desktop owner)
//!   and the on-screen keyboard (IME owner).
//! - LIST / RESTORE / CLEAR (the history): the shell and the keyboard only —
//!   an app pastes the newest item, it never browses what others copied.
//! - FOCUS: windowd alone, by kernel sender id. Until its first push every
//!   read is denied (fail-closed).
//!
//! Identity is always the kernel's `sender_service_id`, never a payload byte.
//! Contents never reach a marker or a log line — only sequence numbers and
//! lengths do.
//! OWNERS: @ui @runtime
//! STATUS: Functional (v1: text)
//! API_STABILITY: Internal (the wire is `nexus_wire::clipboardd`)
//! TEST_COVERAGE: tests/contract.rs — history order/eviction/dedupe, the gate
//!   matrix (`test_reject_*`), the query filter, malformed frames, markers
//! RFC: docs/rfcs/RFC-0094-content-transfer-v1-clipboardd.md

#![cfg_attr(not(any(test, nexus_env = "host")), no_std)]
#![forbid(unsafe_code)]

pub mod answer;
pub mod gate;
pub mod history;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
