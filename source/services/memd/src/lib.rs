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
//! - PAGER (6f-2, ADR-0088): this service holds the kernel's sink, so it is the swap stage's
//!   pager too — it moves a squeezed frozen app's sealed records to the `swap` partition (blkd
//!   grants it to memd alone) at `critical` before any kill, answers the kernel's page-in
//!   requests at once, pages an app's slots back when it thaws, frees the slots of a dead
//!   object. It only ever holds ciphertext: the kernel keeps the key.
//!
//! - JOURNAL (6f-3): every kill this authority ordered and saw carried out is recorded in the
//!   state store (`/state/memd/journal`, a ring of the newest sixteen) — victim, band, reason,
//!   level, free frames, when.
//!
//! [`bands`] is the table and the victim rule, [`authority`] the state machine over levels and
//! bands, [`answer`] one request → one reply, [`order`] the kill order's bytes, [`swap`] the
//! pager's books, [`journal`] the kill ledger's record; the host tests drive exactly the code
//! the service loop runs.
//! OWNERS: @runtime
//! STATUS: Experimental (6e-2: bands, the sink, the kill; 6e-3: the fan-out; 6f-2: the pager;
//!   6f-3: the journal)
//! API_STABILITY: Internal (the wire is `nexus_wire::memd`)
//! TEST_COVERAGE: tests/contract.rs — the victim rule (`test_reject_kill_of_foreground_or_visible`),
//!   the sender gates (`test_reject_*`), the status and drill replies, the kill proof;
//!   tests/swap.rs — the pager's books; tests/journal.rs — the kill ledger's record and the boot's
//!   ledger over a store that refuses (`test_reject_*`); QEMU — the kernel-sourced chain on the
//!   `lowmem` lane
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

#![cfg_attr(not(any(test, nexus_env = "host")), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod answer;
pub mod authority;
pub mod bands;
pub mod gpu;
pub mod journal;
pub mod order;
pub mod swap;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod pager;
