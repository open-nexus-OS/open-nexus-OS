// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Architecture specific support code
//! OWNERS: @kernel-arch-team
//! PUBLIC API: arch backends under `arch::<isa>`
//! DEPENDS_ON: per-ISA modules (e.g., riscv)
//! INVARIANTS: Keep per-arch code isolated behind module boundaries
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

pub mod riscv;
