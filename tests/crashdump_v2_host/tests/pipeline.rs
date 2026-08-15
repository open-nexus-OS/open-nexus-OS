// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Crashdump v2a host pipeline proofs (TASK-0048): nxsym indexing of
//! a real fixture binary, Build-ID keyed symbolization of dump frames, `.nxcd`
//! file roundtrips (plain + zst), and GC/budget behavior on a directory tree.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 integration tests
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crashdump_v2_host::{fixture_minidump, symbol_address};

/// Known frame in this test binary; the ladder below indexes the executable
/// and resolves this function's address through `symbols.nxsym`.
#[inline(never)]
#[no_mangle]
pub extern "C" fn crashdump_v2_fixture_frame() -> u64 {
    1234
}

#[test]
fn test_nxsym_indexes_fixture_binary_and_resolves_known_frame() {
    let _ = crashdump_v2_fixture_frame();
    let exe = std::env::current_exe().expect("current exe");
    let pc = symbol_address(&exe, "crashdump_v2_fixture_frame").expect("fixture pc");

    let index = nxsym::build_index(std::slice::from_ref(&exe)).expect("build index");
    assert_eq!(index.binaries.len(), 1);
    let build_id = index.binaries[0].build_id.clone();
    assert!(!build_id.is_empty());
    assert!(!index.binaries[0].entries.is_empty());

    let hit = nxsym::lookup(&index, &build_id, pc).expect("lookup").expect("resolved");
    assert!(hit.function.contains("crashdump_v2_fixture_frame"), "got: {}", hit.function);
    assert_ne!(hit.file, "<unknown>");
    assert_ne!(hit.line, 0);
}

#[test]
fn test_nxsym_index_file_roundtrip_and_deterministic_bytes() {
    let exe = std::env::current_exe().expect("current exe");
    let index = nxsym::build_index(std::slice::from_ref(&exe)).expect("build index");
    let a = nxsym::write_index(&index).expect("write a");
    let b = nxsym::write_index(&index).expect("write b");
    assert_eq!(a, b);
    let back = nxsym::read_index(&a).expect("read");
    assert_eq!(back, index);
}

#[test]
fn test_symbolize_dump_frames_via_minidump_build_id() {
    let _ = crashdump_v2_fixture_frame();
    let exe = std::env::current_exe().expect("current exe");
    let pc = symbol_address(&exe, "crashdump_v2_fixture_frame").expect("fixture pc");
    let index = nxsym::build_index(std::slice::from_ref(&exe)).expect("build index");

    // The dump carries the Build-ID (MinidumpFrame.build_id); symbolization
    // keys on it verbatim — nxsym never re-derives ids for dumps.
    let mut dump = fixture_minidump(1, 7, "demo.sym");
    dump.build_id = index.binaries[0].build_id.clone();
    dump.pcs = vec![pc];
    let container = nxcd::from_minidump(&dump).expect("convert");
    let frames = nxcd::FramesSection::from_section(&container).expect("frames");
    assert_eq!(frames.frames[0].build_id, dump.build_id);

    let hit = nxsym::lookup(&index, &frames.frames[0].build_id, frames.frames[0].pc)
        .expect("lookup")
        .expect("resolved");
    assert!(hit.function.contains("crashdump_v2_fixture_frame"));
}

#[test]
fn test_reject_symbolize_with_unknown_build_id() {
    let exe = std::env::current_exe().expect("current exe");
    let index = nxsym::build_index(std::slice::from_ref(&exe)).expect("build index");
    let dump = fixture_minidump(1, 7, "demo.unknown");
    assert_eq!(
        nxsym::lookup(&index, &dump.build_id, 0x1000),
        Err(nxsym::NxsymError::UnknownBuildId)
    );
}

#[test]
fn test_nxcd_file_roundtrip_plain_and_zst() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dump = fixture_minidump(555, 9, "demo.roundtrip");
    let container = nxcd::from_minidump(&dump).expect("convert");
    let encoded = container.encode().expect("encode");

    let plain = dir.path().join("a.nxcd");
    std::fs::write(&plain, &encoded).expect("write plain");
    let back = nxcd::NxcdContainer::decode(&std::fs::read(&plain).expect("read")).expect("decode");
    assert_eq!(back, container);
    let header = nxcd::CrashHeader::from_section(&back).expect("header");
    assert_eq!(header.timestamp_nsec, 555);
    assert_eq!(header.pid, 9);
    assert_eq!(header.build_id, dump.build_id);

    let zst = dir.path().join("a.nxcd.zst");
    std::fs::write(&zst, nxcd::compress_nxcd(&encoded).expect("compress")).expect("write zst");
    let raw = nxcd::decompress_nxcd(&std::fs::read(&zst).expect("read")).expect("decompress");
    assert_eq!(raw, encoded);
    assert_eq!(nxcd::NxcdContainer::decode(&raw).expect("decode"), container);
}

#[test]
fn test_reject_corrupt_dump_files() {
    // Corrupt container bytes and corrupt zst streams both map to
    // deterministic rejects (untrusted-input contract).
    assert_eq!(nxcd::NxcdContainer::decode(b"garbage-not-nxcd"), Err(nxcd::NxcdError::BadMagic));
    assert_eq!(nxcd::decompress_nxcd(b"garbage-not-zstd"), Err(nxcd::NxcdError::ZstCodec));
    let dump = fixture_minidump(1, 1, "demo.trunc");
    let encoded = nxcd::from_minidump(&dump).expect("convert").encode().expect("encode");
    assert_eq!(
        nxcd::NxcdContainer::decode(&encoded[..encoded.len() - 3]),
        Err(nxcd::NxcdError::LengthMismatch)
    );
}

#[test]
fn test_gc_budget_logic_on_directory_tree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut entries = Vec::new();
    for (ts, pid, name) in [(100u64, 1u32, "demo.a"), (200, 2, "demo.b"), (300, 3, "demo.c")] {
        let dump = fixture_minidump(ts, pid, name);
        let bytes = nxcd::from_minidump(&dump).expect("convert").encode().expect("encode");
        let file = format!("{ts}.{pid}.{name}.nxcd");
        std::fs::write(dir.path().join(&file), &bytes).expect("write dump");
        entries.push(nxcd::GcEntry { id: file, bytes: bytes.len() as u64, timestamp_nsec: ts });
    }
    let plan =
        nxcd::plan_purge(&entries, &nxcd::GcBudget { max_total_bytes: u64::MAX, max_count: 1 });
    assert_eq!(plan.len(), 2);
    for id in &plan {
        std::fs::remove_file(dir.path().join(id)).expect("apply plan");
    }
    let survivors: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(survivors, vec![String::from("300.3.demo.c.nxcd")]);
}

#[test]
fn test_dump_writing_is_deterministic_across_runs() {
    let dump = fixture_minidump(42, 4, "demo.det");
    let a = nxcd::from_minidump(&dump).expect("a").encode().expect("encode a");
    let b = nxcd::from_minidump(&dump).expect("b").encode().expect("encode b");
    assert_eq!(a, b);
    let za = nxcd::compress_nxcd(&a).expect("za");
    let zb = nxcd::compress_nxcd(&b).expect("zb");
    assert_eq!(za, zb);
}
