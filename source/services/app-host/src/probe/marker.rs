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
    // Bounded at 192 bytes: the longest line is RFC-0108's judged `apphost: ux launch`
    // with its stages (~130) — 96 cut it before its `budget_ms=` (cycles 52–54).
    let mut buf = [0u8; 192];
    let bytes = line.as_bytes();
    let n = bytes.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    if let Ok(text) = core::str::from_utf8(&buf[..n]) {
        let _ = nexus_abi::debug_println(text);
    }
}

/// An event frame no arm took — a bounded marker WITH IDENTITY (magic/op/len): seven anonymous
/// skips cost a debugging session. At most eight per process (`said` counts them).
pub(super) fn odd_frame(frame: &[u8], said: &mut u32) {
    if *said >= 8 {
        return;
    }
    *said += 1;
    let (m0, m1, op) = (
        frame.first().copied().unwrap_or(0),
        frame.get(1).copied().unwrap_or(0),
        frame.get(3).copied().unwrap_or(0),
    );
    let len = frame.len();
    raw_marker(&alloc::format!(
        "apphost: event frame skipped m={m0:02x}{m1:02x} op={op} len={len}"
    ));
}

/// A present the compositor refused (an ack whose status is not OK) — never silent: the window
/// would stop showing without a word. Shares the odd-frame bound (`said`).
pub(super) fn present_nack(status: u8, seq: u32, said: &mut u32) {
    if status == nexus_display_proto::client_surface::SURFACE_STATUS_OK || *said >= 8 {
        return;
    }
    *said += 1;
    raw_marker(&alloc::format!("apphost: present refused (status={status} seq={seq})"));
}
