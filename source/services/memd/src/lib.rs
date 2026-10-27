// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `memd` — the ONE memory authority (RFC-0100 §3, ADR-0086, TASK-0333 6e-2). The
//! kernel says how tight memory is (ADR-0085: one `OP_MEMORY_PRESSURE` frame per level change
//! into the one sink this service binds); the lifecycle authority (abilitymgr) says what each
//! app process is to the user (its BAND: foreground > visible > background-declared > hidden >
//! suspended, pushed as it changes); this service decides WHO GOES when the level says
//! somebody must — the lowest band first, the longest there first — and orders the kill
//! through the lifecycle authority, which orders the parent (execd, ADR-0082). It never kills
//! a process itself, never a service (only processes the lifecycle authority named are in its
//! table), never the foreground, and at `critical` nothing on screen.
//!
//! WHO MAY DO WHAT (deny-by-default, the kernel sender id decides):
//! - BAND: the lifecycle authority alone. A band from anyone else is dropped.
//! - STATUS: every route holder (the proof harness).
//! - DRILL: the proof harness alone — a simulated level over the real bands; the victim, the
//!   order, the kill and the reap are the live path. The harness runs it on a QEMU lane only.
//! - LEVEL (6e-3): every level — the kernel's and a drill's — is told onward on the declared
//!   legs, NONBLOCK: to gpud (its caches) and to windowd, which tells every app window
//!   (`OP_SURFACE_PRESSURE`; app-host raises the host event `MemoryPressure`). The kernel's own
//!   answer to `warn` — the purge of the objects their owners unlocked (RFC-0100 §4) — needs no
//!   word from this service.
//!
//! [`bands`] is the table and the victim rule, [`authority`] the state machine over levels and
//! bands, [`answer`] one request → one reply, [`order`] the kill order's bytes; the host tests
//! drive exactly the code the service loop runs.
//! OWNERS: @runtime
//! STATUS: Experimental (6e-2: bands, the sink, the kill; 6e-3: the fan-out)
//! API_STABILITY: Internal (the wire is `nexus_wire::memd`)
//! TEST_COVERAGE: tests/contract.rs — the victim rule (`test_reject_kill_of_foreground_or_visible`),
//!   the sender gates (`test_reject_*`), the status and drill replies, the kill proof
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

#![cfg_attr(not(any(test, nexus_env = "host")), no_std)]
#![forbid(unsafe_code)]

pub mod answer;
pub mod authority;
pub mod bands;
pub mod gpu;
pub mod order;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
