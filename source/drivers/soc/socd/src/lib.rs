// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `socd` — the ONE writer of the syscon and pinctrl windows (RFC-0106,
//! TASK-0245B P2). A consumer sends `BRING_UP <node path>`; socd checks that the
//! requester holds `soc.glue` (policyd, deny-by-default), brings the node up from
//! the tree it was handed (`nexus_soc::bring_up`: power domain, resets released,
//! clocks on, every write read back) over the provider windows init granted by
//! compatible and answers a verdict; `CLOCK_RATE` answers `nexus_soc::clock_rate`.
//! The node operations are the library's, so the loader — which runs them for its
//! boot disk before any service exists (TASK-0246B P2) — means the same by them. A tree without providers (QEMU
//! virt) makes every plan empty: the honest answer is `NOT_NEEDED`, never `OK`.
//! `verdict` is the pure part: request bytes + tree + windows → reply bytes, so
//! the host tests drive the same code the service loop runs.
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional (P2: bring-up + clock rate; P3: the display set — power
//!   domain 7, hmclk at its demanded rate — and the marker's register words; pads follow)
//! API_STABILITY: Internal (the wire is `nexus_wire::soc`)
//! TEST_COVERAGE: tests/contract.rs — NotNeeded on virt, the eMMC and display
//!   verdicts over a register file seeded from the measured board state, denied/
//!   no-such-node/malformed/unmeasured-domain replies, the marker with every
//!   register a bring-up touched before and after (a line too long counts what it
//!   drops); QEMU: `socd: ready (no soc glue in this tree)` +
//!   `SELFTEST: soc glue not needed ok`
//! RFC: docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md

#![cfg_attr(not(any(test, nexus_env = "host")), no_std)]
// The single unsafe allowance lives in `bus` (the mapped windows); everything else denies.
#![deny(unsafe_code)]

pub mod verdict;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod bus;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
