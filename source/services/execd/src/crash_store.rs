// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Crash-evidence-at-rest core (TASK-0051B) — cfg-free logic over
//! the shipped 0048 container SSOT (`nxcd`): NMD1 → `.nxcd` conversion with
//! the ADR-0056 reason and redaction-gated preview sections, the on-device
//! artifact caps, and GC planning over `/state/crash/` keys. The OS glue
//! (`crash_os.rs`) only feeds bytes/policy answers in and applies the plan;
//! every decision lives here under host tests. Placement decision
//! (ledger, 2026-08-24): artifacts are statefs KV records under
//! `/state/crash/` — NMD1 capture is bounded (8 KiB frame), so a container
//! stays far below the 64 KiB KV value cap; bulk-scale captures move to
//! `/data` with TASK-0317. Secrets are designed out: the writer only ever
//! encodes the validated NMD1 frame — no store reads, no `/state/secret/*`
//! surface exists on this path.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/crash_store_contract.rs (conversion + redaction +
//! caps + GC plan + rejects).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use nxcd::{GcBudget, GcEntry, NxcdContainer, SectionKind};

/// Hard cap for one on-device `.nxcd` artifact (KV-value honest: well under
/// the statefs 64 KiB value cap; a legal 8 KiB NMD1 frame converts to
/// ~12 KiB, so hitting this bound means a malformed producer).
pub const ARTIFACT_MAX_BYTES: usize = 32 * 1024;

/// On-device retention budget (ledger decision 2026-08-24): newest-first,
/// both limits apply. Covers `.nxcd` artifacts AND degraded `.nmd`
/// leftovers so a broken conversion path cannot accumulate either.
pub const GC_BUDGET: GcBudget = GcBudget { max_total_bytes: 256 * 1024, max_count: 8 };

/// Redaction level for preview attachments (RFC-0087 §5): resolved from
/// policyd BEFORE conversion, deny-by-default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachLevel {
    /// header/frames/maps only — no raw bytes leave the capture.
    None,
    /// + bounded stack/code previews (the conservative granted default).
    StackOnly,
    /// Today identical to `StackOnly`: NMD1 capture carries no full-memory
    /// section (RFC-0031 stance) — the level exists so the policy shape is
    /// stable when a richer capture arrives.
    Full,
}

/// Deny-by-default cascade over the two delegated capability answers
/// (`crash.attach.full`, `crash.attach.stack`).
pub fn attach_level(full_granted: bool, stack_granted: bool) -> AttachLevel {
    if full_granted {
        AttachLevel::Full
    } else if stack_granted {
        AttachLevel::StackOnly
    } else {
        AttachLevel::None
    }
}

/// Converts validated NMD1 bytes into the on-device `.nxcd` artifact.
/// Deterministic reject reasons feed the degrade marker verbatim.
pub fn convert_nmd(
    nmd: &[u8],
    reason: Option<&str>,
    level: AttachLevel,
) -> Result<Vec<u8>, &'static str> {
    let frame = crash::MinidumpFrame::decode(nmd).map_err(|_| "nmd-decode")?;
    let mut container = nxcd::from_minidump_with_reason(&frame, reason).map_err(|_| "convert")?;
    attach_previews(&mut container, &frame, level)?;
    let bytes = container.encode().map_err(|_| "encode")?;
    if bytes.len() > ARTIFACT_MAX_BYTES {
        return Err("artifact-cap");
    }
    Ok(bytes)
}

/// Preview sections per redaction level — the ONLY place raw capture bytes
/// enter the container.
fn attach_previews(
    container: &mut NxcdContainer,
    frame: &crash::MinidumpFrame,
    level: AttachLevel,
) -> Result<(), &'static str> {
    if level == AttachLevel::None {
        return Ok(());
    }
    if !frame.stack_preview.is_empty() {
        container
            .insert(SectionKind::Stack, frame.stack_preview.clone())
            .map_err(|_| "stack-preview")?;
    }
    if !frame.code_preview.is_empty() {
        container
            .insert(SectionKind::Code, frame.code_preview.clone())
            .map_err(|_| "code-preview")?;
    }
    Ok(())
}

/// `.nxcd` key derived from the intermediate's `.nmd` key; `None` for any
/// path outside the validated `/state/crash/` shape (fail closed).
pub fn container_key(nmd_path: &str) -> Option<String> {
    crash::validate_dump_path(nmd_path).ok()?;
    let stem = nmd_path.strip_suffix(".nmd")?;
    let mut out = String::with_capacity(stem.len() + 5);
    out.push_str(stem);
    out.push_str(".nxcd");
    Some(out)
}

/// GC plan over `/state/crash/` KV entries (`(key, value_len)`): keys carry
/// their capture timestamp (`/state/crash/<ts>.<pid>.<name>.<ext>`), the
/// budget keeps the newest artifacts. Unparseable keys age as ts=0 (deleted
/// first — they are not artifacts this writer produced).
pub fn gc_plan(entries: &[(String, usize)]) -> Vec<String> {
    let candidates: Vec<GcEntry> = entries
        .iter()
        .map(|(key, len)| GcEntry {
            id: key.clone(),
            bytes: *len as u64,
            timestamp_nsec: key_timestamp(key),
        })
        .collect();
    nxcd::plan_purge(&candidates, &GC_BUDGET)
}

fn key_timestamp(key: &str) -> u64 {
    let name = key.rsplit('/').next().unwrap_or("");
    let ts = name.split('.').next().unwrap_or("");
    ts.parse::<u64>().unwrap_or(0)
}
