// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `.nxdelta` v1 host proof floor (RFC-0090, TASK-0034): make/
//! apply byte-identity over realistic edit shapes, determinism (make twice
//! ⇒ identical bytes), tamper detection through the digest gates, base
//! substitution reject, and the COPY-coverage sanity that makes deltas
//! worth shipping (an appended-trailer target — the os-B fixture shape —
//! must be almost all COPY).
//! OWNERS: @runtime @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 integration tests
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

use nxdelta::make::{apply, make, BLOCK};

/// Deterministic pseudo-random-ish bytes (no RNG — determinism DoD).
fn pattern(len: usize, seed: u32) -> Vec<u8> {
    (0..len).map(|i| ((i as u32).wrapping_mul(31).wrapping_add(seed) % 251) as u8).collect()
}

#[test]
fn make_apply_roundtrip_over_edit_shapes() {
    let base = pattern(300_003, 7);
    // Shapes: append, prepend, mid-edit, shrink, disjoint blocks.
    let mut appended = base.clone();
    appended.extend_from_slice(&pattern(512, 99));
    let mut prepended = pattern(1000, 5);
    prepended.extend_from_slice(&base);
    let mut edited = base.clone();
    edited[150_000..150_100].copy_from_slice(&pattern(100, 3));
    let shrunk = base[..200_001].to_vec();
    let unrelated = pattern(50_000, 42);
    for (name, target) in [
        ("appended", appended),
        ("prepended", prepended),
        ("edited", edited),
        ("shrunk", shrunk),
        ("unrelated", unrelated),
    ] {
        let delta = make(&base, &target);
        let out = apply(&base, &delta).unwrap_or_else(|e| panic!("{name}: apply failed {e:?}"));
        assert_eq!(out, target, "{name}: reconstruction must be byte-identical");
    }
}

#[test]
fn make_is_deterministic() {
    let base = pattern(123_456, 1);
    let mut target = base.clone();
    target.extend_from_slice(&pattern(4096, 2));
    target[60_000..60_050].copy_from_slice(&pattern(50, 9));
    assert_eq!(make(&base, &target), make(&base, &target), "make twice ⇒ identical bytes");
}

#[test]
fn appended_trailer_is_almost_all_copy() {
    // The os-B fixture shape: kernel + one 512-byte build-id trailer. The
    // delta must be tiny relative to the target, or the format has no
    // reason to exist (this is the honest bandwidth claim, pinned).
    let base = pattern(2 * 1024 * 1024, 11);
    let mut target = base.clone();
    target.extend_from_slice(&pattern(512, 77));
    let delta = make(&base, &target);
    assert!(
        delta.len() < 4096 + BLOCK,
        "append-shape delta must be header+records+trailer-sized, got {} bytes",
        delta.len()
    );
    assert_eq!(apply(&base, &delta).expect("apply"), target);
}

#[test]
fn test_reject_tampered_stream() {
    let base = pattern(100_000, 3);
    let mut target = base.clone();
    target.extend_from_slice(&pattern(600, 4));
    let good = make(&base, &target);
    // Flip one byte in every region: header digests, record area, END.
    for at in [40, 70, nxdelta::HEADER_LEN + 5, good.len() - 4] {
        let mut bad = good.clone();
        bad[at] ^= 0x40;
        assert!(apply(&base, &bad).is_err(), "tamper at byte {at} must reject");
    }
}

#[test]
fn test_reject_wrong_base() {
    let base = pattern(80_000, 6);
    let target = pattern(80_500, 8);
    let delta = make(&base, &target);
    let other = pattern(80_000, 66);
    assert!(apply(&other, &delta).is_err(), "base substitution must reject, never reconstruct");
}

#[test]
fn empty_target_is_a_format_error_not_a_panic() {
    // target_size == 0 is outside the format (a boot image is never
    // empty); the decoder refuses the header rather than emitting nothing.
    let base = pattern(10_000, 2);
    let target: Vec<u8> = Vec::new();
    let delta = make(&base, &target);
    assert!(apply(&base, &delta).is_err(), "zero-size target must reject as Format");
}
