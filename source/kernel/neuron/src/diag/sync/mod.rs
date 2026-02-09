// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Debug-only sync utilities
//! OWNERS: @kernel-sync-team
//! STATUS: Experimental (debug-only)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (debug-only; exercised via kernel debug builds)
//! PUBLIC API: dbg_mutex (debug builds)
//! DEPENDS_ON: spin (debug), riscv time CSR
//! INVARIANTS: Only compiled in debug; avoid in hot paths in release
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#[cfg(debug_assertions)]
pub mod dbg_mutex;
