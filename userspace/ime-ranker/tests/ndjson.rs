// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0203 NDJSON goldens + security reject matrix: export/import is a
//! byte-identical round trip (export bytes = the contract), and a hostile
//! profile cannot pick a bad version, smuggle an oversized/malformed line, or
//! bust the quota.

use ime_ranker::{
    export_ndjson, import_ndjson, train, DictStat, ImportError, MemStore, PersonalStore,
};

fn trained_store() -> MemStore {
    let mut s = MemStore::default();
    train(&mut s, None, b"aa", 1);
    train(&mut s, Some(b"aa"), b"bb", 2);
    // A multibyte (CJK) candidate exercises the hex path: あ = E3 81 82.
    train(&mut s, None, "\u{3042}".as_bytes(), 3);
    s
}

#[test]
fn export_import_round_trip_is_byte_identical() {
    let original = trained_store();
    let bytes1 = export_ndjson(&original);
    let mut restored = MemStore::default();
    let report = import_ndjson(&mut restored, &bytes1).expect("import ok");
    assert_eq!(report.skipped, 0, "clean export imports with no skips");
    // Restored state re-exports byte-for-byte — the round-trip contract.
    assert_eq!(bytes1, export_ndjson(&restored));
}

#[test]
fn export_is_deterministic() {
    assert_eq!(export_ndjson(&trained_store()), export_ndjson(&trained_store()));
}

#[test]
fn reject_import_empty() {
    let mut s = MemStore::default();
    assert_eq!(import_ndjson(&mut s, ""), Err(ImportError::Empty));
}

#[test]
fn reject_import_bad_version() {
    let mut s = MemStore::default();
    // Version 2 header — the whole file is rejected, nothing loaded.
    let data =
        "{\"v\":2,\"kind\":\"ime-personal\"}\n{\"t\":\"d\",\"c\":\"6161\",\"f\":1,\"b\":1}\n";
    assert_eq!(import_ndjson(&mut s, data), Err(ImportError::BadHeader));
    assert!(s.dict_entries().is_empty());
}

#[test]
fn reject_import_oversize_line() {
    let mut s = MemStore::default();
    let header = export_ndjson(&MemStore::default()); // just the header line
    let oversize = "x".repeat(300); // > NDJSON_LINE_MAX (256)
    let good = "{\"t\":\"d\",\"c\":\"6161\",\"f\":7,\"b\":9}";
    let data = format!("{header}{oversize}\n{good}\n");
    let report = import_ndjson(&mut s, &data).expect("header ok");
    assert_eq!(report.skipped, 1, "oversized line skipped before parsing");
    assert_eq!(report.dict_loaded, 1, "the good line still loads");
    assert_eq!(s.get_dict(b"aa").map(|d| d.freq), Some(7));
}

#[test]
fn reject_import_malformed_lines() {
    let mut s = MemStore::default();
    let header = export_ndjson(&MemStore::default());
    // Bad hex, unknown field, non-numeric count, missing braces — all skipped.
    let data = format!(
        "{header}{}\n{}\n{}\n{}\n",
        "{\"t\":\"d\",\"c\":\"zz\",\"f\":1,\"b\":1}", // bad hex
        "{\"t\":\"d\",\"c\":\"6161\",\"q\":1,\"b\":1}", // unknown field
        "{\"t\":\"g\",\"p\":\"6161\",\"c\":\"6262\",\"n\":\"x\"}", // non-numeric
        "not-json",                                   // no braces
    );
    let report = import_ndjson(&mut s, &data).expect("header ok");
    assert_eq!(report.skipped, 4);
    assert_eq!(report.dict_loaded, 0);
    assert!(s.dict_entries().is_empty());
}

#[test]
fn import_quota_enforced() {
    // Export a store holding more entries than the target's quota, then import
    // into a small-quota store: it must stay bounded (eviction), never grow.
    let mut big = MemStore::default();
    for i in 0..10u16 {
        big.upsert_dict(&[b'a', b'0' + i as u8], DictStat { freq: i + 1, last_seen_bucket: i });
    }
    let data = export_ndjson(&big);
    let mut small = MemStore::with_quota(3);
    let report = import_ndjson(&mut small, &data).expect("import ok");
    assert_eq!(report.dict_loaded, 10, "every line is offered to the store");
    assert!(small.dict_len() <= 3, "quota is enforced on import (eviction)");
}
