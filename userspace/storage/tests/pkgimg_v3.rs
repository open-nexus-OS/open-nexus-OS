// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `pkgimg` v3 (system volume, RFC-0089 §12.1) host proofs:
//! build/parse roundtrip, determinism, contiguous 4 KiB-aligned bundle
//! windows with launch params, index-only parse, and the reject suite —
//! tampered entry/bundle digests, out-of-bounds bundle table, launch rows
//! without a bundle, v2 images rejected by the v3 parser.
//! OWNERS: @runtime @storage @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 integration tests

use sha2::{Digest, Sha256};
use storage::pkgimg::{PkgImgCaps, PkgImgError, PkgImgFileSpec};
use storage::pkgimg_bundles::{
    build_volume, bundle_window, parse_index, parse_superblock, verify_bundle, verify_entry,
    BundleLaunch, MAGIC_V3,
};

fn specs() -> Vec<PkgImgFileSpec> {
    vec![
        PkgImgFileSpec::new("metricsd", "1.0.0", "payload.elf", &[0x7f; 5000]),
        PkgImgFileSpec::new("metricsd", "1.0.0", "manifest.nxb", b"manifest-m"),
        PkgImgFileSpec::new("timed", "1.0.0", "payload.elf", &[0x11; 100]),
        PkgImgFileSpec::new("apps", "1.0.0", "demo.hello/payload.nxir", b"ui-program"),
    ]
}

fn launch() -> Vec<BundleLaunch> {
    vec![
        BundleLaunch {
            bundle: "metricsd".into(),
            version: "1.0.0".into(),
            stack_pages: 8,
            global_pointer: 0x8000_0800,
        },
        BundleLaunch {
            bundle: "timed".into(),
            version: "1.0.0".into(),
            stack_pages: 4,
            global_pointer: 0x9000_0800,
        },
    ]
}

fn build() -> Vec<u8> {
    build_volume(&specs(), &launch(), &PkgImgCaps::default()).expect("build v3")
}

#[test]
fn roundtrip_windows_and_launch_params() {
    let img = build();
    assert_eq!(&img[..8], MAGIC_V3);
    let idx = parse_index(&img, &PkgImgCaps::default()).expect("parse index");
    assert_eq!(idx.entries.len(), 4);
    assert_eq!(idx.bundles.len(), 3, "one row per (bundle, version)");
    let m = idx.bundle("metricsd", "1.0.0").expect("metricsd row");
    assert_eq!((m.stack_pages, m.global_pointer), (8, 0x8000_0800));
    assert_eq!(m.data_offset % 4096, 0, "windows are 4 KiB aligned");
    // The window covers both metricsd files (contiguous, sorted by path).
    let window = bundle_window(&img, &idx.superblock, m).expect("window");
    assert!(window.starts_with(b"manifest-m"));
    assert_eq!(window[4096..4096 + 5000], [0x7f; 5000]);
    assert_eq!(Sha256::digest(window).as_slice(), m.sha256);
    let apps = idx.bundle("apps", "1.0.0").expect("data-only bundle");
    assert_eq!((apps.stack_pages, apps.global_pointer), (0, 0));
    for b in &idx.bundles {
        verify_bundle(&img, &idx.superblock, b).expect("bundle digest");
    }
    for e in &idx.entries {
        verify_entry(&img, &idx.superblock, e).expect("entry digest");
    }
    assert!(idx.bundle_by_sha(&m.sha256).is_some());
}

#[test]
fn build_is_deterministic_and_index_only_parse_works() {
    let a = build();
    let b = build();
    assert_eq!(a, b);
    let sb = parse_superblock(&a, &PkgImgCaps::default()).expect("superblock");
    // The boot-time read: superblock + index only, no data bytes.
    let head = &a[..sb.index_end()];
    let idx = parse_index(head, &PkgImgCaps::default()).expect("head parse");
    assert_eq!(idx.bundles.len(), 3);
    assert_eq!(idx.superblock, sb);
}

#[test]
fn test_reject_pkgimg_v3_entry_digest_mismatch() {
    let mut img = build();
    let idx = parse_index(&img, &PkgImgCaps::default()).expect("parse");
    let e = idx.entries.iter().find(|e| e.path == "payload.elf" && e.bundle == "timed").unwrap();
    let off = idx.superblock.data_offset + e.data_offset as usize;
    img[off] ^= 0xFF;
    // Index still parses (data untouched), lazy verification catches it.
    let idx2 = parse_index(&img, &PkgImgCaps::default()).expect("index intact");
    let e2 = idx2.entries.iter().find(|x| x.bundle == "timed").unwrap();
    assert_eq!(verify_entry(&img, &idx2.superblock, e2), Err(PkgImgError::EntryDigestMismatch));
}

#[test]
fn test_reject_pkgimg_v3_bundle_digest_mismatch() {
    let mut img = build();
    let idx = parse_index(&img, &PkgImgCaps::default()).expect("parse");
    let b = idx.bundle("metricsd", "1.0.0").unwrap();
    // Flip a PADDING byte inside the window: no entry digest changes, but
    // the window digest (the reuse unit) must.
    let off = idx.superblock.data_offset + b.data_offset as usize + 20;
    img[off] ^= 0x01;
    let idx2 = parse_index(&img, &PkgImgCaps::default()).expect("index intact");
    let b2 = idx2.bundle("metricsd", "1.0.0").unwrap();
    assert_eq!(verify_bundle(&img, &idx2.superblock, b2), Err(PkgImgError::BundleDigestMismatch));
    for e in &idx2.entries {
        verify_entry(&img, &idx2.superblock, e).expect("entries untouched");
    }
}

#[test]
fn test_reject_pkgimg_v3_bundle_table_out_of_bounds() {
    let img = build();
    let sb = parse_superblock(&img, &PkgImgCaps::default()).expect("sb");
    // Rebuild the index with a bundle row pointing past the data section,
    // re-hash it so only the bounds check can fire.
    let mut head = img[..sb.index_end()].to_vec();
    let index = &mut head[sb.index_offset..];
    // Walk to the bundle table: entry_count + entries.
    let mut off = 4usize;
    let n = u32::from_le_bytes([index[0], index[1], index[2], index[3]]) as usize;
    for _ in 0..n {
        let bl = u16::from_le_bytes([index[off], index[off + 1]]) as usize;
        let vl = u16::from_le_bytes([index[off + 2], index[off + 3]]) as usize;
        let pl = u16::from_le_bytes([index[off + 4], index[off + 5]]) as usize;
        off += 8 + 16 + bl + vl + pl + 32;
    }
    off += 4; // bundle_count
    let data_len_off = off + 2 + 2 + 4 + 8;
    index[data_len_off..data_len_off + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let digest = Sha256::digest(&head[sb.index_offset..sb.index_end()]);
    head[44..76].copy_from_slice(&digest);
    let err = parse_index(&head, &PkgImgCaps::default()).expect_err("must reject");
    assert_eq!(err, PkgImgError::EntryOutOfBounds);
}

#[test]
fn test_reject_pkgimg_v3_launch_row_without_bundle() {
    let extra = vec![BundleLaunch {
        bundle: "ghost".into(),
        version: "1.0.0".into(),
        stack_pages: 8,
        global_pointer: 1,
    }];
    let err = build_volume(&specs(), &extra, &PkgImgCaps::default()).expect_err("must reject");
    assert_eq!(err, PkgImgError::Malformed("launch row without bundle"));
}

#[test]
fn test_reject_pkgimg_v3_parser_on_v2_image() {
    let v2 = storage::pkgimg::build_pkgimg(&specs(), PkgImgCaps::default()).expect("v2");
    let sb = parse_superblock(&v2, &PkgImgCaps::default()).expect("v2 superblock parses");
    assert_eq!(sb.version, 2);
    assert_eq!(
        parse_index(&v2, &PkgImgCaps::default()).expect_err("v2 has no bundle table"),
        PkgImgError::BadMagicOrVersion
    );
}
