// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract for the crash-evidence-at-rest core
//! (TASK-0051B): NMD1→`.nxcd` conversion carries reason + redaction-gated
//! previews and nothing else, the intermediate/artifact key derivation
//! fails closed outside `/state/crash/`, the artifact cap holds, and the
//! GC plan enforces the ledger budget newest-first. The container SCHEMA
//! stays pinned by `cargo test -p nxcd` — this file pins the writer's
//! decisions only.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

use execd::crash_store::{
    attach_level, container_key, convert_nmd, gc_plan, AttachLevel, ARTIFACT_MAX_BYTES, GC_BUDGET,
};
use nxcd::{CrashHeader, NxcdContainer, SectionKind};

fn sample_nmd(stack: usize, code: usize) -> Vec<u8> {
    crash::MinidumpFrame {
        timestamp_nsec: 4242,
        pid: 9,
        code: -22,
        name: String::from("demo.fault"),
        build_id: crash::deterministic_build_id("demo.fault"),
        pcs: vec![0x1020, 0x2040],
        stack_preview: vec![0xAA; stack],
        code_preview: vec![0xCC; code],
    }
    .encode()
    .expect("encode")
}

#[test]
fn convert_carries_reason_and_previews_at_stack_level() {
    let bytes = convert_nmd(&sample_nmd(64, 16), Some("fault(load-page)"), AttachLevel::StackOnly)
        .expect("convert");
    assert!(bytes.len() <= ARTIFACT_MAX_BYTES);
    let container = NxcdContainer::decode(&bytes).expect("decode");
    let header = CrashHeader::from_section(&container).expect("header");
    assert_eq!(header.reason.as_deref(), Some("fault(load-page)"));
    assert_eq!(header.pid, 9);
    assert_eq!(container.get(SectionKind::Stack).map(<[u8]>::len), Some(64));
    assert_eq!(container.get(SectionKind::Code).map(<[u8]>::len), Some(16));
}

#[test]
fn test_reject_previews_absent_at_level_none() {
    let bytes = convert_nmd(&sample_nmd(64, 16), None, AttachLevel::None).expect("convert");
    let container = NxcdContainer::decode(&bytes).expect("decode");
    assert!(container.get(SectionKind::Stack).is_none(), "redacted stack must not persist");
    assert!(container.get(SectionKind::Code).is_none(), "redacted code must not persist");
    let header = CrashHeader::from_section(&container).expect("header");
    assert_eq!(header.reason, None);
}

#[test]
fn test_reject_garbage_nmd_maps_stable_reason() {
    assert_eq!(convert_nmd(b"not a minidump", None, AttachLevel::StackOnly), Err("nmd-decode"));
}

#[test]
fn attach_level_cascade_is_deny_by_default() {
    assert_eq!(attach_level(false, false), AttachLevel::None);
    assert_eq!(attach_level(false, true), AttachLevel::StackOnly);
    assert_eq!(attach_level(true, false), AttachLevel::Full);
    assert_eq!(attach_level(true, true), AttachLevel::Full);
}

#[test]
fn container_key_swaps_extension_inside_scope_only() {
    assert_eq!(
        container_key("/state/crash/4242.9.demo.fault.nmd").as_deref(),
        Some("/state/crash/4242.9.demo.fault.nxcd")
    );
    // Fail closed: out-of-scope paths and non-nmd suffixes derive nothing —
    // the writer can never place an artifact outside /state/crash/.
    assert_eq!(container_key("/state/secret/4242.9.demo.fault.nmd"), None);
    assert_eq!(container_key("/state/crash/../secret/x.nmd"), None);
    assert_eq!(container_key("/state/crash/4242.9.demo.fault.nxcd"), None);
}

#[test]
fn gc_plan_keeps_newest_within_budget() {
    // 10 artifacts of 4 KiB: count budget (8) binds before the byte budget.
    let entries: Vec<(String, usize)> = (0..10u64)
        .map(|i| (format!("/state/crash/{}.7.demo.fault.nxcd", 1000 + i), 4096))
        .collect();
    let plan = gc_plan(&entries);
    assert_eq!(
        plan,
        vec![
            "/state/crash/1000.7.demo.fault.nxcd".to_string(),
            "/state/crash/1001.7.demo.fault.nxcd".to_string(),
        ],
        "oldest two fall out of the count budget"
    );

    // Byte budget: 8 × 33 KiB > 256 KiB — only the newest 7 fit.
    let big: Vec<(String, usize)> = (0..8u64)
        .map(|i| (format!("/state/crash/{}.7.demo.fault.nxcd", 2000 + i), 33 * 1024))
        .collect();
    let plan = gc_plan(&big);
    assert_eq!(plan, vec!["/state/crash/2000.7.demo.fault.nxcd".to_string()]);
    assert!(GC_BUDGET.max_total_bytes >= 7 * 33 * 1024);
}

#[test]
fn gc_plan_ages_unparseable_keys_first() {
    let mut entries: Vec<(String, usize)> =
        (0..8u64).map(|i| (format!("/state/crash/{}.7.demo.fault.nxcd", 3000 + i), 1024)).collect();
    entries.push(("/state/crash/stray-file.nmd".to_string(), 1024));
    let plan = gc_plan(&entries);
    assert_eq!(plan, vec!["/state/crash/stray-file.nmd".to_string()]);
}
