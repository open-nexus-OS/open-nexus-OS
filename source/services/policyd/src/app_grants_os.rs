// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The OS half of the app-grants authority (TASK-0333 6d-3, ADR-0084): the pure
//! handler decides, this prints the proof. `policyd: app grants (app= n=)` once per published
//! app — the lines that replace abilitymgr's build-time `caps ok` self-check — and the
//! refusals by name (`policyd: app grants reject (app= cap=)`, `policyd: app grants denied
//! (op= sender=)`), bounded so a hostile sender cannot write a UART line per request.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the decision in `app_grants.rs` (host); the markers on the headless lane
//!   (`SELFTEST: app grants ok (`, `SELFTEST: app grants put deny ok`)

use core::sync::atomic::{AtomicU32, Ordering};

use crate::app_grants::{AppGrants, Outcome};
use crate::os_lite::emit_line;
use nexus_sel::Policy;

/// Refusal lines printed per boot before they go quiet (the counters stay in the audit).
const REFUSAL_LINES: u32 = 16;
static REFUSALS: AtomicU32 = AtomicU32::new(0);

/// Handles an app-grants frame and prints its marker; `None` when the frame is not one.
pub(crate) fn handle(
    policy: &Policy<'_>,
    table: &mut AppGrants,
    frame: &[u8],
    sender_service_id: u64,
    out: &mut [u8],
) -> Option<usize> {
    let (len, outcome) = crate::app_grants::handle(policy, table, frame, sender_service_id, out)?;
    match outcome {
        Outcome::Stored { app, n, .. } => {
            emit_line(&alloc::format!("policyd: app grants (app={} n={n})", text(app)))
        }
        Outcome::Rejected { app, cap } => emit_line(&alloc::format!(
            "policyd: app grants reject (app={} cap={})",
            text(app),
            text(cap)
        )),
        Outcome::Full { app } => {
            emit_line(&alloc::format!("policyd: FAIL app grants (table full; app={})", text(app)))
        }
        Outcome::Denied { op } | Outcome::Malformed { op } => {
            if REFUSALS.fetch_add(1, Ordering::Relaxed) < REFUSAL_LINES {
                let what =
                    if matches!(outcome, Outcome::Denied { .. }) { "denied" } else { "malformed" };
                emit_line(&alloc::format!(
                    "policyd: app grants {what} (op={op} sender=0x{sender_service_id:016x})"
                ));
            }
        }
        // Reads are the hot path of every launch: no line (the reader prints its own).
        Outcome::Read { .. } => {}
    }
    Some(len)
}

fn text(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap_or("?")
}
