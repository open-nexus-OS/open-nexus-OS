// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `socd` — the ONE writer of the syscon and pinctrl windows (RFC-0106,
//! TASK-0245B P2). A consumer sends `BRING_UP <node path>`; socd checks that the
//! requester holds `soc.glue` (policyd, deny-by-default), plans the node's steps
//! from the tree it was handed (`nexus-soc`: power domain, resets released,
//! clocks on) over the provider windows init granted by compatible, executes
//! them with read-back and answers a verdict. A tree without providers (QEMU
//! virt) makes every plan empty: the honest answer is `NOT_NEEDED`, never `OK`.
//! `verdict` is the pure part: request bytes + tree + windows → reply bytes, so
//! the host tests drive the same code the service loop runs.
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional (P2: bring-up + clock rate; pads and power domains follow P3)
//! API_STABILITY: Internal (the wire is `nexus_wire::soc`)
//! TEST_COVERAGE: tests/contract.rs — NotNeeded on virt, the eMMC verdict over a
//!   register file seeded from the measured board state, denied/no-such-node/
//!   malformed replies; QEMU: `socd: ready (no soc glue in this tree)` +
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
