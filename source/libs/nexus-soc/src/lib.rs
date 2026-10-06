// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SoC glue model (RFC-0106, TASK-0245B): how a consumer node's
//! `clocks` / `resets` / `power-domains` / `pinctrl-0` become register writes on
//! the syscon windows its providers own. Three layers, all `no_std` and free of
//! `unsafe`: the **tables** (per provider compatible, keyed by the binding
//! header's ids: register offset, gate/mux/div/FC fields, reset polarity, parent
//! rates, a power domain's control and status bits — every entry with a
//! provenance line), the **planner** (a node + its providers' windows → an
//! ordered, bounded list of steps: domain, resets released, gates on, the rates
//! the binding demands (`assigned-clock-rates`, met exactly or refused); pads
//! follow in TASK-0245B P3), and the **executor** (steps over a
//! `nexus_hal::Bus`, every write read back, every self-clearing bit and every
//! power domain's status polled with a bound; a step already satisfied writes
//! nothing). On top, the two **node
//! operations** every caller runs: `bring_up` (plan, then execute; an empty plan
//! is `NotNeeded`) and `clock_rate` (a named clock's rate, read back). `socd` runs
//! them on real windows while the OS runs; the loader, before any service exists,
//! for its boot disk (TASK-0246B P2); the tests on a register file seeded from the
//! board's measured state.
//!
//! No address lives here: windows come from the tree's `reg`, tables hold offsets.
//!
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional (P1: tables for the Block-1/2 consumers, planner, executor;
//!   the node operations since TASK-0246B P2; the display set — power domain 7,
//!   `hmclk` at its demanded rate — since TASK-0245B P3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/k1.rs — the eMMC/USB/UART plans against the measured APMU
//!   state (no writes) and a cold register file (exactly the documented bits),
//!   rates read back, a stuck frequency-change bit; the display pipeline from the
//!   stock state (no writes) and from cold (the domain's rising request, the
//!   demanded rate), a domain that never reports on, a rate no parent makes, an
//!   unmeasured domain; the node operations (not needed, up, refused, faulted; a
//!   named rate, an unknown name)
//! RFC: docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md

#![no_std]
#![forbid(unsafe_code)]

mod field;
mod node;
mod ops;
pub mod pad;
mod plan;
mod provider;
pub mod table;

pub use field::Field;
pub use node::{bring_up, bring_up_with, clock_rate, BringUp, BringUpError, RateError};
pub use ops::{Executor, Fault, Pause, Report, GPIO_LEVEL_POLL_READS};
pub use pad::{pad_bits, Bias, PAD_OWNED};
pub use plan::{plan, Plan, PlanError, Registers, Step, MAX_STEPS};
pub use provider::{Provider, ProviderKind, Providers};
