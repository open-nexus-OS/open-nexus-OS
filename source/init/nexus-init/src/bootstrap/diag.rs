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

/// RFC-0068 for the post-bootstrap helpers that carry no tally: a routine
/// `init: <subject> …` trace prints raw in proof (non-folding) boots and
/// when the subject is expanded; in an interactive boot it folds away
/// (the harness never greps these lines).
#[inline]
pub(crate) fn raw_or_expanded(subject: &str) -> bool {
    !nexus_abi::boot_should_fold_verdicts() || expanded(subject)
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

/// One marker line built with `core::fmt` and written with one call (a line written in pieces
/// can be torn by a service printing at the same time); longer than [`emit_marker_atomic`]'s,
/// for lines that carry a device path.
pub(crate) struct Line {
    buf: [u8; 192],
    len: usize,
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let take = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

impl Line {
    pub(crate) fn new() -> Self {
        Self { buf: [0; 192], len: 0 }
    }

    pub(crate) fn emit(mut self) {
        use core::fmt::Write as _;
        let _ = self.write_str("\n");
        crate::bootstrap::helpers::debug_write_bytes(&self.buf[..self.len]);
    }
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

/// A control frame that is neither a route ask, an exec check nor a control verb: NAMED with
/// its sender, length and first bytes, in one atomic line, instead of vanishing (TASK-0327B P4
/// H0d — on the board the platform floor's `@ready` frames never reached the ready table, and
/// a silent skip was the only possible witness). Bounded: eight head bytes, one line.
pub(crate) fn emit_ctrl_frame_unknown(svc: &str, frame: &[u8]) {
    let mut head = 0u64;
    for (i, b) in frame.iter().take(8).enumerate() {
        head |= u64::from(*b) << (8 * (7 - i));
    }
    // Frames are at most 64 bytes (`IPC_SYS_TRUNCATE` into a 64-byte buffer): two digits.
    let len = frame.len().min(99);
    let digits = [b'0' + (len / 10) as u8, b'0' + (len % 10) as u8];
    let digits = if len < 10 { &digits[1..] } else { &digits[..] };
    emit_marker_atomic(
        &[b"init: ctrl frame unknown svc=", svc.as_bytes(), b" len=", digits, b" head="],
        Some(head),
    );
}
