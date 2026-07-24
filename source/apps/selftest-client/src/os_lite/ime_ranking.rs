// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0204 / RFC-0075 Phase 4 — an in-OS proof that the deterministic
//! ime-ranker runs correctly under the real service allocator (no_std + alloc),
//! not just on host: one commit lifts a table-last candidate to the front, and
//! that learned order survives an NDJSON export→import round trip (the shape the
//! statefs persistence path reloads). No IPC — this exercises the crate itself
//! inside the selftest-client process. The statefs-backed transport + live imed
//! wiring are separate follow-up cuts.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker `SELFTEST: ime ranking ok` via the bringup ladder.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use ime_ranker::{export_ndjson, import_ndjson, rank, train, MemStore, PersonalStore};

/// Proves adaptive ranking + reload in the OS runtime. Returns `Err(())` on any
/// mismatch so the caller emits the honest FAIL marker.
pub(crate) fn ime_ranking_probe() -> Result<(), ()> {
    let cands = [b"aa".as_slice(), b"bb".as_slice(), b"cc".as_slice()];

    // "cc" is LAST in table order; one commit must lift it to the front.
    let mut store = MemStore::default();
    train(&mut store, None, b"cc", 5);
    if rank(&store, None, &cands, 5).first() != Some(&2) {
        return Err(());
    }

    // Serialize → reload into a fresh store: the learned order must survive
    // (this is exactly what the statefs load path will reconstruct).
    let blob = export_ndjson(&store);
    let mut restored = MemStore::default();
    if import_ndjson(&mut restored, &blob).is_err() {
        return Err(());
    }
    if restored.get_dict(b"cc").is_none() {
        return Err(());
    }
    if rank(&restored, None, &cands, 5).first() != Some(&2) {
        return Err(());
    }
    Ok(())
}
