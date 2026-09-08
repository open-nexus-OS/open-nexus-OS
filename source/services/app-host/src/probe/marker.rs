// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app-host marker funnel (`apphost:` / `APPHOST:` lines):
//! bounded, allocation-free, and RFC-0068 verdict-folding aware.
//! OWNERS: @runtime
//! STATUS: Experimental

pub(crate) fn raw_marker(line: &str) {
    // RFC-0068 verdict folding: in an interactive boot the routine `apphost:`
    // / `APPHOST:` markers fold into one `app-host N/N` grid line (failures
    // — `FAIL`/`err` — still print); proof boots print every marker raw.
    let mut buf = [0u8; 96];
    let bytes = line.as_bytes();
    let n = bytes.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    if let Ok(text) = core::str::from_utf8(&buf[..n]) {
        let _ = nexus_abi::debug_println(text);
    }
}
