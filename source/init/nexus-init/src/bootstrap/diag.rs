// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap diagnostics — RFC-0068 fold helpers shared across the
//! orchestrator's phase modules (spawn/endpoints/grants/wiring/finish).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0068-structured-event-observability.md

/// True if `name` is listed in `NEXUS_LOG_EXPAND` (a comma set). The grid GROUP names are themselves
/// the flags: `init_spawn` / `init_caps` expand their whole group (the displayed name IS what you
/// type). A BARE service name (e.g. `keystored`) also matches — that expands ONE service's init lines
/// across spawn+caps, together with its own service markers (cross-process subject debug). No central
/// collector needed; compile-time today, like the per-service expand.
pub(crate) fn expanded(name: &str) -> bool {
    match option_env!("NEXUS_LOG_EXPAND") {
        Some(list) => list.split(',').any(|g| g.trim() == name),
        None => false,
    }
}

/// RFC-0068: fold ONE init cap-wiring DIAGNOSTIC marker into the `init_caps` verdict; return whether
/// its raw trace still prints — in non-folding (proof) boots, OR when the `init_caps` GROUP is
/// expanded, OR when this line's SUBJECT (the bare service, `init:` prefix stripped) is expanded.
/// These `init: <svc> …` traces are NOT harness-grepped and each precedes a real `cap_transfer` (its
/// `err` arm aborts), so the folded count is a real "N wiring steps" tally.
#[inline]
pub(crate) fn iw(wire: &mut nexus_event::SpanTally, fold: bool, subject: &str) -> bool {
    wire.record(nexus_event::Status::Ok, nexus_abi::nsec().unwrap_or(0));
    let svc = subject.strip_prefix("init:").unwrap_or(subject);
    !fold || expanded("init_caps") || expanded(svc)
}

/// Like [`iw`] but for the `lifecycle` group — the boot LIFECYCLE events (entry/timing/deferred-resume/
/// probe/rollback), named for WHAT happens, not the `init` emitter. Expanded by the `lifecycle` group
/// flag OR the bare subject — e.g. `init: deferred resume gpud` carries subject `gpud`, so
/// `NEXUS_LOG_EXPAND=gpud` reveals it together with gpud's bring-up + runtime (one keyword, the whole
/// subject's story). `init: ready` is NEVER folded — it is the harness/launcher stop marker.
#[inline]
pub(crate) fn il(wire: &mut nexus_event::SpanTally, fold: bool, subject: &str) -> bool {
    wire.record(nexus_event::Status::Ok, nexus_abi::nsec().unwrap_or(0));
    let svc = subject.strip_prefix("init:").unwrap_or(subject);
    !fold || expanded("lifecycle") || expanded(svc)
}

/// Emits one CONTRACT marker line atomically (single `debug_println`).
/// The per-byte `debug_write_*` helpers tear against concurrently running
/// services — a freshly resumed instance printed its ready line INSIDE
/// init's `service restarted` marker (smp1, 2026-08-24), which broke the
/// paired exit/restart harness count. `hex` (when given) is appended as
/// 16 lowercase nibbles.
pub(crate) fn emit_marker_atomic(parts: &[&[u8]], hex: Option<u64>) {
    let mut line = [0u8; 112];
    let mut len = 0usize;
    for part in parts {
        let take = part.len().min(line.len() - len);
        line[len..len + take].copy_from_slice(&part[..take]);
        len += take;
    }
    if let Some(value) = hex {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for shift in (0..16).rev() {
            if len < line.len() {
                line[len] = HEX[((value >> (shift * 4)) & 0xf) as usize];
                len += 1;
            }
        }
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        let _ = nexus_abi::debug_println(msg);
    }
}
