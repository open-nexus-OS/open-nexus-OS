// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Behavior-first host contract tests for `nexus-vmo`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: deterministic host contract assertions
//! ADR: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md

#![cfg(nexus_env = "host")]

use nexus_vmo::{PeerPid, TransferOutcome, TransferRights, Vmo};

#[test]
fn test_transfer_and_mapping_accounting_is_deterministic_for_fixed_fixture() {
    let mut vmo = Vmo::create(8).expect("create vmo");
    vmo.write(0, b"abcdefgh").expect("write payload");
    vmo.authorize_transfer_to(PeerPid::new(7));

    let transfer = vmo
        .transfer_to(PeerPid::new(7), TransferRights::MAP)
        .expect("authorized transfer must succeed");
    assert_eq!(transfer, TransferOutcome::HostCopyFallback { copied_bytes: 8 });

    let first = vmo.map_ro(0, 8).expect("first mapping");
    assert_eq!(first.as_slice(), b"abcdefgh");
    let second = vmo.map_ro(2, 4).expect("second mapping");
    assert_eq!(second.as_slice(), b"cdef");

    let counters = vmo.counters();
    assert_eq!(counters.copy_fallback_count, 1);
    assert_eq!(counters.control_plane_bytes, 16);
    assert_eq!(counters.bulk_bytes, 16);
    assert_eq!(counters.map_reuse_hits, 1);
    assert_eq!(counters.map_reuse_misses, 1);
}

#[test]
fn test_transfer_without_map_rights_is_rejected_fail_closed() {
    let mut vmo = Vmo::create(8).expect("create vmo");
    vmo.authorize_transfer_to(PeerPid::new(11));
    let err = vmo
        .transfer_to(PeerPid::new(11), TransferRights::SEND)
        .expect_err("missing MAP right must reject transfer");
    assert_eq!(err, nexus_vmo::Error::RightsMismatch);
}

#[test]
fn test_from_bytes_and_slice_preserve_contract_bounds() {
    let vmo = Vmo::from_bytes(b"hello-vmo").expect("from bytes");
    let slice = vmo.slice(1, 4).expect("bounded slice");
    assert_eq!(slice.as_slice(), b"ello");
    assert_eq!(slice.len(), 4);
}

#[test]
fn test_from_file_range_loads_exact_host_range() {
    let mut path = std::env::temp_dir();
    path.push(format!("nexus-vmo-range-{}.bin", std::process::id()));
    std::fs::write(&path, b"0123456789abcdef").expect("write fixture");
    let vmo = Vmo::from_file_range(&path, 4, 6).expect("load file range");
    let mapped = vmo.slice(0, 6).expect("slice loaded range");
    assert_eq!(mapped.as_slice(), b"456789");
    let _ = std::fs::remove_file(path);
}
