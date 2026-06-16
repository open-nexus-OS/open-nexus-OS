// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0033 DriverKit core — the shared "submit + fence + buffers + QoS" contract.
//!
//! CONTEXT: Every accelerator device-class server (GPU/NPU/VPU/Audio) needs the same
//! plumbing: a **bounded in-flight submit ring** with per-slot lifecycle + backpressure,
//! **completion tracking** that a timeline fence can mirror, **buffer budgets**, and **QoS**
//! hints. Today gpud's `CtrlQueue` (ADR-0032) is a one-off prototype of exactly this. This
//! crate promotes that pattern into a reusable, device-agnostic library so a device server
//! shrinks to just MMIO / command-encoding / reset — the ring, fences, budgets, and tracing
//! hooks live here.
//!
//! OWNERS: @kernel @runtime @drivers
//! STATUS: Draft (RFC-0033 Phase 3) — host-first contract; consumers wired in Phase 4 (#49).
//! API_STABILITY: Unstable
//! TEST_COVERAGE: golden host tests (`cargo test -p nexus-driverkit`) — the deterministic
//!   oracle; QEMU proves the gpud/windowd integration later.
//!
//! DESIGN: This crate is **pure, `no_std`, and allocation-free** — [`SubmitRing`] is a fixed
//! 32-slot busy-bitmask + round-robin allocator (a faithful generalisation of gpud's ring),
//! [`BufferBudget`] is bounded counters, [`Qos`] is an enum + depth policy. None of it touches
//! MMIO, the router, or the scheduler: the device server drives `try_alloc`/`complete` from
//! its own submit/harvest path, and signals a kernel timeline fence (`nexus_abi::fence_signal`)
//! to [`SubmitRing::completed`] so consumers can `fence_wait`. Keeping the contract pure makes
//! the host the deterministic oracle (RFC-0033 §Proof); see
//! `docs/architecture/02-selftest-and-ci.md`.
//!
//! ```
//! use nexus_driverkit::{SubmitRing, Qos};
//! let mut ring = SubmitRing::new(4);
//! // Producer: reserve a slot (backpressure when full), encode + submit the command.
//! let (slot, ticket) = ring.try_alloc().expect("ring not full");
//! // ... device-specific: encode command into slot, ring doorbell ...
//! // Consumer/harvest: the device reported `slot` done.
//! let done = ring.complete(slot).unwrap();
//! assert_eq!(done, ticket);
//! assert_eq!(ring.completed(), 1); // the value a completion fence is signalled to
//! let _ = Qos::Frugal.target_depth(ring.capacity()); // 1 in-flight → low power
//! ```

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

mod buffers;
mod qos;
mod ring;

pub use buffers::{BufferBudget, BufferError};
pub use qos::Qos;
pub use ring::{RingError, Slot, SubmitRing, Ticket, MAX_SLOTS};
