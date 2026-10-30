// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0333 6f-3 host proofs: `DebugLine` builds one console line in place — the bytes, the
//! numbers (zero, the extremes, negative) — and cuts an overlong line at the cap with room for the
//! newline (`test_reject_*`), never past it. The write itself (`emit`, one `debug_write`) is the
//! OS's: every lane's ladder matches the lines whole.

use nexus_abi::{DebugLine, DEBUG_LINE_CAP};

#[test]
fn a_line_is_built_from_bytes_strings_and_numbers() {
    let line = DebugLine::new()
        .put(b"abilitymgr: kill (inst=")
        .u64(3)
        .str(" reason=memory)")
        .put(b" zero=")
        .u64(0)
        .put(b" max=")
        .u64(u64::MAX)
        .put(b" code=")
        .i64(-9)
        .put(b" min=")
        .i64(i64::MIN);
    assert_eq!(
        line.as_bytes(),
        b"abilitymgr: kill (inst=3 reason=memory) zero=0 max=18446744073709551615 code=-9 \
min=-9223372036854775808"
    );
    assert!(DebugLine::default().as_bytes().is_empty());
}

#[test]
fn test_reject_a_line_past_the_cap() {
    let long = [b'x'; DEBUG_LINE_CAP * 2];
    let line = DebugLine::new().put(&long).put(b"tail").u64(12345);
    assert_eq!(line.as_bytes().len(), DEBUG_LINE_CAP - 1, "cut, one byte left for the newline");
    assert!(line.as_bytes().iter().all(|b| *b == b'x'), "nothing past the cap");
}
