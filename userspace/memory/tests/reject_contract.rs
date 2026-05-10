// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deny-by-default reject-path proofs for `nexus-vmo` TASK-0031 scope.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: reject-path integration tests
//! ADR: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md

#![cfg(nexus_env = "host")]

use nexus_vmo::{Error, PeerPid, TransferRights, Vmo};

#[test]
fn test_reject_unauthorized_transfer() {
    let mut vmo = Vmo::create(4).expect("create vmo");
    let err = vmo
        .transfer_to(PeerPid::new(99), TransferRights::MAP)
        .expect_err("unauthorized transfer must fail");
    assert_eq!(err, Error::UnauthorizedTransfer);
}

#[test]
fn test_reject_oversized_mapping() {
    let mut vmo = Vmo::create(16).expect("create vmo");
    let err = vmo.map_ro(8, 9).expect_err("map past end must fail");
    assert_eq!(err, Error::OutOfBounds);
}

#[test]
fn test_ro_mapping_enforced() {
    let mut vmo = Vmo::create(8).expect("create vmo");
    vmo.write(0, b"nexusvmo").expect("seed payload");
    vmo.seal_ro();

    let mapped = vmo.map_ro(0, 8).expect("ro mapping after seal");
    assert_eq!(mapped.as_slice(), b"nexusvmo");

    let err = vmo.write(0, b"reject!!").expect_err("sealed vmo must reject write");
    assert_eq!(err, Error::ReadOnlyViolation);
}

#[test]
fn test_reject_file_range_short_read() {
    let mut path = std::env::temp_dir();
    path.push(format!("nexus-vmo-short-{}.bin", std::process::id()));
    std::fs::write(&path, b"abc").expect("write short fixture");
    let err = match Vmo::from_file_range(&path, 0, 8) {
        Ok(_) => panic!("short read must fail"),
        Err(err) => err,
    };
    assert_eq!(err, Error::IoFailure);
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_reject_transfer_to_slot_on_host_backend() {
    let mut vmo = Vmo::create(16).expect("create vmo");
    vmo.authorize_transfer_to(PeerPid::new(1));
    let err = vmo
        .transfer_to_slot(PeerPid::new(1), TransferRights::MAP, 23)
        .expect_err("slot transfer is unsupported on host backend");
    assert_eq!(err, Error::Unsupported);
}
