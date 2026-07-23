// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the RFC-0080 read-only-VMO map policy as a pure bit function, kept
//! OUT of the target-gated `mm`/`syscall` modules so its security invariant is
//! host-unit-tested (like `waitset`/`fence`/`image_allocs`/`ipc_eof`). `sys_map`
//! runs a `VmoRo` cap's requested page flags through [`force_readonly`] before
//! installing the mapping, so the holder can NEVER map the shared pages
//! writable or executable — even if it asks. Bit values mirror
//! `mm::page_table::PageFlags` (VALID=1, READ=2, WRITE=4, EXECUTE=8, USER=16).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the invariant truth table below (host)

/// Page-flag bits (mirror of `mm::page_table::PageFlags`).
const VALID: usize = 1 << 0;
const READ: usize = 1 << 1;
const WRITE: usize = 1 << 2;
const EXECUTE: usize = 1 << 3;
const USER: usize = 1 << 4;

/// Forces a `VmoRo` mapping's page flags to read-only user memory: WRITE and
/// EXECUTE are always cleared; VALID, READ and USER are always set. The result
/// therefore NEVER carries WRITE or EXECUTE regardless of what the caller
/// requested — the RFC-0080 shared-atlas anti-corruption invariant.
#[must_use]
pub fn force_readonly(requested: usize) -> usize {
    (requested & !(WRITE | EXECUTE)) | VALID | READ | USER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_execute_are_always_cleared() {
        // Whatever the caller asks, the result is read-only user memory.
        for req in [
            0,
            WRITE,
            EXECUTE,
            WRITE | EXECUTE,
            VALID | READ | WRITE | EXECUTE | USER,
            usize::MAX, // a caller that sets every bit still cannot get WRITE/EXEC.
        ] {
            let got = force_readonly(req);
            assert_eq!(got & WRITE, 0, "WRITE must be stripped (req=0x{req:x})");
            assert_eq!(got & EXECUTE, 0, "EXECUTE must be stripped (req=0x{req:x})");
            assert_eq!(got & VALID, VALID, "VALID must be set");
            assert_eq!(got & READ, READ, "READ must be set");
            assert_eq!(got & USER, USER, "USER must be set");
        }
    }

    #[test]
    fn a_plain_read_request_is_unchanged_in_meaning() {
        // A already-RO request maps to exactly RO user memory (no surprises).
        assert_eq!(force_readonly(VALID | READ | USER), VALID | READ | USER);
    }
}
