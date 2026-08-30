// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::all)]

//! CONTEXT: nxboot machine logic — the host-testable half of the
//! first-stage loader (RFC-0089 §7, ADR-0059). Slot selection over the BSB
//! (trial decrement BEFORE load, exhaustion fallback) and the build-baked
//! trust anchor live here; codecs (BSB/NXBD/handoff) come from `bootfmt`.
//! Only entry asm, MMIO uart and the SBI reset are target-only (see
//! `src/main.rs` / `src/arch.rs` — the single unsafe-bearing module).
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: select state table + trust bake integration tests
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

pub mod flow;
pub mod select;
pub mod trust;
