// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Shared fixtures for the crashdump v2a host test suite (TASK-0048):
//! deterministic minidump fixtures and symbol discovery in the running test
//! executable (host-only, no QEMU).
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Consumed by `tests/` in this crate
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

#![forbid(unsafe_code)]

use object::{Object, ObjectSymbol};
use std::path::Path;

/// Deterministic minidump-v1 fixture.
pub fn fixture_minidump(ts: u64, pid: u32, name: &str) -> crash::MinidumpFrame {
    crash::MinidumpFrame {
        timestamp_nsec: ts,
        pid,
        code: 42,
        name: String::from(name),
        build_id: crash::deterministic_build_id(name),
        pcs: vec![0x1000, 0x2000, 0x3000],
        stack_preview: vec![0xAA; 32],
        code_preview: vec![0xCC; 8],
    }
}

/// Address of a named symbol in an ELF on disk (test executables carry their
/// own known frames).
pub fn symbol_address(elf: &Path, needle: &str) -> Option<u64> {
    let bytes = std::fs::read(elf).ok()?;
    let obj = object::File::parse(&*bytes).ok()?;
    for sym in obj.symbols() {
        if let Ok(name) = sym.name() {
            if name.contains(needle) {
                return Some(sym.address());
            }
        }
    }
    None
}
