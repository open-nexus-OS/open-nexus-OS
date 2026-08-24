// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: Crash-evidence-at-rest OS glue (TASK-0051B, RFC-0087 §5) — the
//! execd-side `.nxcd` writer over the cfg-free `crash_store` core. Flow:
//! resolve the attachment level ONCE per boot from policyd
//! (`crash.attach.full` → `crash.attach.stack` → none, deny-by-default),
//! convert the bounded NMD1 frame, PUT the container under
//! `/state/crash/…​.nxcd`, delete the `.nmd` intermediate (kept only when
//! conversion degrades — loudly). Retention GC runs once per boot on the
//! first publish: LIST + bounded GETs feed `crash_store::gc_plan`
//! (`nxcd::plan_purge` SSOT), deletions are audited as an evidence-class
//! record (`execd.audit` scope). Best-effort by contract: the dying
//! process is already reaped — a failed publish degrades, never blocks.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: decision core host-tested (tests/crash_store_contract.rs);
//! this glue is QEMU-proven (`crash: dump written` + gc markers).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use statefs::client::StatefsClient;

use crate::crash_store::{self, AttachLevel};

/// Cached policy answer (0 = unresolved; 1 = none, 2 = stack-only, 3 = full).
static ATTACH_LEVEL: AtomicU8 = AtomicU8::new(0);
/// Retention GC runs once per boot, on the first publish.
static GC_DONE: AtomicBool = AtomicBool::new(false);

/// Publishes one crash artifact from validated NMD1 bytes. `nmd_key` is the
/// at-rest intermediate to delete on success (`None` when the frame only
/// ever existed in RAM). Returns the published `.nxcd` key.
pub(crate) fn publish_container(
    artifact_key: &str,
    nmd_bytes: &[u8],
    nmd_key: Option<&str>,
    reason: &str,
) -> Option<String> {
    let Ok(client) = StatefsClient::new() else {
        emit_degrade("statefs-route");
        return None;
    };
    gc_once(&client);
    let bytes = match crash_store::convert_nmd(nmd_bytes, Some(reason), resolved_attach_level()) {
        Ok(bytes) => bytes,
        Err(why) => {
            emit_degrade(why);
            return None;
        }
    };
    if client.put(artifact_key, &bytes).is_err() || client.sync().is_err() {
        emit_degrade("statefs-put");
        return None;
    }
    if let Some(nmd_key) = nmd_key {
        // Intermediate seam closed: the container now carries everything
        // (previews included per redaction level). Best-effort delete — a
        // leftover is bounded by the GC budget either way.
        let _ = client.delete(nmd_key);
    }
    emit_written(artifact_key, bytes.len());
    Some(String::from(artifact_key))
}

/// Attachment level, resolved from policyd once per boot (deny-by-default
/// cascade; Unreachable = deny — a crash writer must never wait on policy).
fn resolved_attach_level() -> AttachLevel {
    match ATTACH_LEVEL.load(Ordering::Relaxed) {
        1 => return AttachLevel::None,
        2 => return AttachLevel::StackOnly,
        3 => return AttachLevel::Full,
        _ => {}
    }
    let sid = nexus_abi::service_id_from_name(b"execd");
    let allows = |cap: &[u8]| {
        matches!(
            nexus_ipc::policyd::check_cap_delegated(sid, cap),
            nexus_ipc::policyd::CapDecision::Allow
        )
    };
    let level =
        crash_store::attach_level(allows(b"crash.attach.full"), allows(b"crash.attach.stack"));
    let cached = match level {
        AttachLevel::None => 1,
        AttachLevel::StackOnly => 2,
        AttachLevel::Full => 3,
    };
    ATTACH_LEVEL.store(cached, Ordering::Relaxed);
    level
}

/// Once-per-boot retention pass: budget marker, bounded inventory, plan,
/// delete, audit. Runs BEFORE the first publish so a full store from prior
/// boots cannot starve the newest artifact.
fn gc_once(client: &StatefsClient) {
    if GC_DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::os_lite::emit_line("crash: retention gc on (budget=256KiB)");
    let Ok(keys) = client.list("/state/crash/", 64) else { return };
    let mut entries: Vec<(String, usize)> = Vec::with_capacity(keys.len());
    for key in keys {
        let len = client.get(&key).map(|v| v.len()).unwrap_or(0);
        entries.push((key, len));
    }
    let plan = crash_store::gc_plan(&entries);
    if plan.is_empty() {
        return;
    }
    let mut deleted = 0usize;
    for key in &plan {
        if client.delete(key).is_ok() {
            deleted += 1;
        }
    }
    emit_gc_deleted(deleted);
    // Evidence-class audit (0049C classifier: `.audit` scope suffix) — GC
    // actions must be reconstructable post-mortem like any other mutation.
    nexus_log::info("execd.audit", |line| {
        line.text("crash gc deleted=");
        line.dec(deleted as u64);
        line.text(" planned=");
        line.dec(plan.len() as u64);
    });
}

fn emit_written(key: &str, bytes: usize) {
    // One atomic line — this marker is harness-gated (torn-marker rule).
    let mut buf = LineBuf::new();
    buf.push_str("crash: dump written (id=");
    buf.push_str(key.rsplit('/').next().unwrap_or(key));
    buf.push_str(" bytes=");
    buf.push_usize(bytes);
    buf.push_str(")");
    buf.emit();
}

fn emit_degrade(reason: &str) {
    let mut buf = LineBuf::new();
    buf.push_str("crash: container write degraded (reason=");
    buf.push_str(reason);
    buf.push_str(")");
    buf.emit();
}

fn emit_gc_deleted(count: usize) {
    let mut buf = LineBuf::new();
    buf.push_str("crash: retention gc deleted (n=");
    buf.push_usize(count);
    buf.push_str(")");
    buf.emit();
}

/// Bounded single-line builder: markers gated by the harness must hit the
/// UART as ONE `debug_println` (per-byte writes tear under SMP).
struct LineBuf {
    buf: [u8; 160],
    len: usize,
}

impl LineBuf {
    fn new() -> Self {
        Self { buf: [0u8; 160], len: 0 }
    }

    fn push_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn push_usize(&mut self, value: usize) {
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        let mut v = value;
        if v == 0 {
            self.push_str("0");
            return;
        }
        while v != 0 && i != 0 {
            i -= 1;
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        for &b in &tmp[i..] {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn emit(&self) {
        if let Ok(line) = core::str::from_utf8(&self.buf[..self.len]) {
            crate::os_lite::emit_line(line);
        }
    }
}
