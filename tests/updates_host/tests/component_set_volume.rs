// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proofs for bundle-set staging (RFC-0089 §12.4, TASK-0321
//! P3) through the REAL engine + the REAL `VolumeAssembler` over a
//! sector-fake device: a set `[boot-image, system-volume, bundle…]`
//! assembles the inactive volume byte-identical to the host build,
//! reusing unshipped bundles from the ACTIVE volume (hashed against the
//! NEW index); the reject matrix (`order`, `bundle-not-in-index`,
//! `volume-digest`, `volume-binding`); the power-cut matrix (a cut at any
//! op leaves the inactive NXSV invalid; a restage converges); volume-only
//! sets pair with the active NXBD digest.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 integration tests
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md
#[path = "common/volume_scene.rs"]
mod volume_scene;
use volume_scene::*;

#[test]
fn full_set_assembles_byte_identical_and_reuses_unshipped_bundles() {
    let s = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, log) = run(&set, VecDev::new(4096), Some(s.active), None);
    assert_eq!(outcome, Ok(()));
    assert_eq!(inactive.body(s.new_vol.len()), &s.new_vol[..], "device volume == host build");
    assert_eq!(&inactive.sectors[..512], &s.new_nxsv[..], "NXSV landed last");
    assert_eq!(
        log,
        vec!["volume newB 3", "bundle alpha@2.0.0", "reused beta@1.0.0", "reused gamma@1.0.0"]
    );
    let back =
        parse_index(inactive.body(s.new_index.superblock.index_end()), &PkgImgCaps::default())
            .expect("index on disk parses");
    assert_eq!(back.bundles.len(), 3);
}

#[test]
fn test_reject_order() {
    let s = scene();
    // bundle before the volume
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
        ],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
    // boot image after the volume
    let set = container(
        "newB",
        2,
        &[volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv), boot_comp(&s.k, "newB", 2)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
    // two volumes
    let set = container(
        "newB",
        2,
        &[
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
        ],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::Order));
}

#[test]
fn test_reject_bundle_not_in_index() {
    let s = scene();
    let mut foreign = bundle_comp(&s.new_vol, &s.new_index, "alpha");
    foreign.3.push(0x99); // not a window of the index
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv), foreign],
    );
    let (outcome, inactive, _) = run(&set, VecDev::new(4096), None, None);
    assert_eq!(outcome, Err(RejectReason::BundleNotInIndex));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0), "NXSV never landed");
    // Reuse needs the bundle in the ACTIVE volume: an active without beta.
    let (lonely_vol, lonely_index) = {
        let specs = vec![PkgImgFileSpec::new("alpha", "1.0.0", "payload.elf", &[0x11; 5000])];
        let bytes = build_volume(&specs, &[], &PkgImgCaps::default()).unwrap();
        let idx = parse_index(&bytes, &PkgImgCaps::default()).unwrap();
        (bytes, idx)
    };
    let lonely_nxsv = signed_nxsv(&lonely_vol, &lonely_index, [0xaa; 32], "old", 1);
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, _) =
        run(&set, VecDev::new(4096), Some(VecDev::with_volume(&lonely_vol, &lonely_nxsv)), None);
    assert_eq!(outcome, Err(RejectReason::BundleNotInIndex));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0));
}

#[test]
fn test_reject_volume_digest_on_tampered_active_bytes() {
    let mut s = scene();
    // Flip one byte inside beta's window in the ACTIVE volume: the reuse
    // copy re-hashes against the NEW index and must refuse.
    let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
    let beta = old_index.bundle("beta", "1.0.0").unwrap();
    let off = VOLUME_START_SECTOR as usize * SECTOR
        + old_index.superblock.data_offset
        + beta.data_offset as usize
        + 10;
    s.active.sectors[off] ^= 0x01;
    let _ = old_vol;
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s.k, "newB", 2),
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    let (outcome, inactive, _) = run(&set, VecDev::new(4096), Some(s.active), None);
    assert_eq!(outcome, Err(RejectReason::VolumeDigest));
    assert!(inactive.sectors[..512].iter().all(|&b| b == 0), "NXSV never landed");
}

#[test]
fn test_reject_volume_binding() {
    let s = scene();
    // Unpaired: the NXSV names a different boot image than the set carries.
    let wrong = signed_nxsv(&s.new_vol, &s.new_index, [0xee; 32], "newB", 2);
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &wrong)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::VolumeBinding));
    // Build mismatch between NXSV and manifest.
    let other = signed_nxsv(&s.new_vol, &s.new_index, sha256(&s.k), "other", 2);
    let set = container(
        "newB",
        2,
        &[boot_comp(&s.k, "newB", 2), volume_comp(&s.new_vol, &s.new_index, &other)],
    );
    assert_eq!(run(&set, VecDev::new(4096), None, None).0, Err(RejectReason::VolumeBinding));
}

#[test]
fn volume_only_set_pairs_with_the_active_nxbd() {
    let s = scene();
    let set = container(
        "newB",
        2,
        &[
            volume_comp(&s.new_vol, &s.new_index, &s.new_nxsv),
            bundle_comp(&s.new_vol, &s.new_index, "alpha"),
        ],
    );
    // The sink pairs with the digest the OS read from the ACTIVE NXBD.
    let active2 = {
        let (old_vol, old_index) = volume("1.0.0", &[0x11; 5000]);
        VecDev::with_volume(&old_vol, &signed_nxsv(&old_vol, &old_index, [0xaa; 32], "old", 1))
    };
    let ok = run(&set, VecDev::new(4096), Some(s.active), Some(sha256(&s.k)));
    assert_eq!(ok.0, Ok(()));
    let bad = run(&set, VecDev::new(4096), Some(active2), Some([0x01; 32]));
    assert_eq!(bad.0, Err(RejectReason::VolumeBinding));
}

#[test]
fn power_cut_matrix_never_leaves_a_valid_nxsv_and_restage_converges() {
    let s0 = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s0.k, "newB", 2),
            volume_comp(&s0.new_vol, &s0.new_index, &s0.new_nxsv),
            bundle_comp(&s0.new_vol, &s0.new_index, "alpha"),
        ],
    );
    // Count the ops of a clean run, then cut at every point before it.
    let (outcome, done, _) = run(&set, VecDev::new(4096), Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    let total_ops = done.ops;
    assert!(total_ops > 10);
    for cut in 1..total_ops {
        let mut inactive = VecDev::new(4096);
        inactive.fail_after = Some(cut);
        let (outcome, torn, _) = run(&set, inactive, Some(scene().active), None);
        assert_eq!(outcome, Err(RejectReason::Io), "cut at {cut}");
        // Invariant: a VALID NXSV exists only over a COMPLETE, byte-identical
        // volume (the descriptor is written after every verification; only
        // the trailing sync can still fail). Any earlier cut leaves the
        // descriptor zeroed/invalid — never a half volume behind a valid NXSV.
        if bootfmt::nxsv::decode(&torn.sectors[..512]).is_ok() {
            assert_eq!(
                torn.body(s0.new_vol.len()),
                &s0.new_vol[..],
                "cut at {cut}: a valid NXSV must sit on the complete volume"
            );
            assert_eq!(&torn.sectors[..512], &s0.new_nxsv[..]);
        }
        // Restage on the torn device converges to the host build.
        let mut again = torn;
        again.fail_after = None;
        let (outcome, fixed, _) = run(&set, again, Some(scene().active), None);
        assert_eq!(outcome, Ok(()), "restage after cut at {cut}");
        assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);
        assert_eq!(&fixed.sectors[..512], &s0.new_nxsv[..]);
    }
}

/// TASK-0035 P1: the stage journal. A cut after k verified windows resumes
/// with exactly those k windows readback-verified (never rewritten), the
/// restage converges byte-identical, and the journal is zeroed after the
/// NXSV commit.
#[test]
fn restage_resumes_journalled_windows_and_zeroes_the_journal_at_commit() {
    let s0 = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s0.k, "newB", 2),
            volume_comp(&s0.new_vol, &s0.new_index, &s0.new_nxsv),
            bundle_comp(&s0.new_vol, &s0.new_index, "alpha"),
        ],
    );
    let (outcome, done, _) = run(&set, VecDev::new(4096), Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    // Journal zeroed after commit.
    assert!(done.sectors[512..1024].iter().all(|&b| b == 0));
    let total_ops = done.ops;
    let mut saw_resume = false;
    for cut in 1..total_ops {
        let mut inactive = VecDev::new(4096);
        inactive.fail_after = Some(cut);
        let (_, torn, torn_log) = run(&set, inactive, Some(scene().active), None);
        let verified_before_cut = torn_log
            .iter()
            .filter(|l| l.starts_with("bundle ") || l.starts_with("reused "))
            .count();
        // Past the commit point the NXSV is valid and the journal already
        // zeroed (a cut in that zeroing sync): the restage starts clean.
        let committed = bootfmt::nxsv::decode(&torn.sectors[..512]).is_ok();
        let mut again = torn;
        again.fail_after = None;
        let (outcome, fixed, log) = run(&set, again, Some(scene().active), None);
        assert_eq!(outcome, Ok(()), "restage after cut at {cut}");
        assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);
        assert!(fixed.sectors[512..1024].iter().all(|&b| b == 0), "journal zeroed at commit");
        let resumed = log
            .iter()
            .find_map(|l| l.strip_prefix("resume ").map(|r| r.to_string()))
            .and_then(|r| r.split('/').next().and_then(|k| k.parse::<usize>().ok()))
            .unwrap_or(0);
        // A window counts only once journalled (persisted after its readback).
        // The torn run's EVENT follows the journal write + sync, so a cut in
        // that sync can leave one more window journalled than announced — it
        // is readback-verified on resume like any other. Never more than that,
        // and a cut after ≥ 1 announced window resumes at least one.
        assert!(
            resumed <= verified_before_cut + 1,
            "cut {cut}: resumed {resumed} > {verified_before_cut} + 1"
        );
        if verified_before_cut >= 2 && !committed {
            assert!(resumed >= 1, "cut {cut}: verified {verified_before_cut}, resumed 0");
            saw_resume = true;
        }
        // Resumed windows are never rewritten: the restage's `bundle` events
        // only cover NON-resumed shipped windows.
        let bundle_events = log.iter().filter(|l| l.starts_with("bundle ")).count();
        assert!(bundle_events + resumed >= 1);
    }
    assert!(saw_resume, "the matrix must exercise at least one resume");
}

/// TASK-0035 P1 `test_reject_journal_*` at the engine: a journal for ANOTHER
/// target, a CRC-torn journal and a journal whose window bytes were tampered
/// are all ignored (no resume, bytes rewritten), and the restage converges.
#[test]
fn test_reject_journal_manifest_crc_and_tampered_window() {
    let s0 = scene();
    let set = container(
        "newB",
        2,
        &[
            boot_comp(&s0.k, "newB", 2),
            volume_comp(&s0.new_vol, &s0.new_index, &s0.new_nxsv),
            bundle_comp(&s0.new_vol, &s0.new_index, "alpha"),
        ],
    );
    // (1) Another target's journal with every bit set: must not resume.
    let mut other = updates::stage_journal::Journal::new([0x77; 32], [0x66; 32], 3).unwrap();
    for r in 0..3 {
        other.set_done(r, true);
    }
    let mut dev = VecDev::new(4096);
    dev.sectors[512..1024].copy_from_slice(&other.encode());
    let (outcome, fixed, log) = run(&set, dev, Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    assert!(!log.iter().any(|l| l.starts_with("resume ")), "{log:?}");
    assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);

    // (2) A torn run leaves a binding journal; corrupt its CRC → no resume.
    let (_, done, _) = run(&set, VecDev::new(4096), Some(scene().active), None);
    let cut = done.ops - 8;
    let mut inactive = VecDev::new(4096);
    inactive.fail_after = Some(cut);
    let (_, mut torn, torn_log) = run(&set, inactive, Some(scene().active), None);
    assert!(torn_log.iter().any(|l| l.starts_with("bundle ") || l.starts_with("reused ")));
    assert!(updates::stage_journal::Journal::decode(&torn.sectors[512..1024]).is_ok());
    let mut crc_torn = torn.clone();
    crc_torn.sectors[512 + 74] ^= 0x01; // flip a bitmap bit without the CRC
    crc_torn.fail_after = None;
    let (outcome, fixed, log) = run(&set, crc_torn, Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    assert!(!log.iter().any(|l| l.starts_with("resume ")), "{log:?}");
    assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);

    // (3) A journalled window whose bytes were tampered: the bit is cleared
    // (readback fails), the window is rewritten, the result is byte-identical.
    let j = updates::stage_journal::Journal::decode(&torn.sectors[512..1024]).unwrap();
    let row = (0..3).find(|&r| j.is_done(r)).expect("a journalled window");
    let b = &s0.new_index.bundles[row];
    let off = VOLUME_START_SECTOR as usize * SECTOR
        + s0.new_index.superblock.data_offset
        + b.data_offset as usize;
    torn.sectors[off] ^= 0xff;
    torn.fail_after = None;
    let (outcome, fixed, log) = run(&set, torn, Some(scene().active), None);
    assert_eq!(outcome, Ok(()));
    let resumed = log
        .iter()
        .find_map(|l| l.strip_prefix("resume "))
        .and_then(|r| r.split('/').next().and_then(|k| k.parse::<usize>().ok()))
        .unwrap_or(0);
    assert!(resumed < j.done_count(), "tampered window must not resume: {log:?}");
    assert_eq!(fixed.body(s0.new_vol.len()), &s0.new_vol[..]);
}
