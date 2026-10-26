// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: The two codecs' round trips and reject matrices (ADR-0081). Every refusal the
//! loader, the kernel and init rely on is named here once.

use super::payload::{self, Header};
use super::toc::{self, Builder, Toc};
use super::{Error, PAGE, SECTOR};

fn bootfs_of(files: &[(&str, usize, u32, u64)]) -> Vec<u8> {
    let mut b = Builder::new();
    for (i, (name, len, stack, gp)) in files.iter().enumerate() {
        let bytes = vec![i as u8 + 1; *len];
        b.add(name, &bytes, *stack, *gp).unwrap();
    }
    b.finish().unwrap()
}

#[test]
fn a_bootfs_round_trips_every_file_on_its_own_pages() {
    let fs =
        bootfs_of(&[("init-lite", 5000, 16, 0x1234), ("execd", 1, 8, 0), ("app-host", 4096, 8, 7)]);
    let toc = Toc::parse(&fs).unwrap();
    assert_eq!(toc.len(), 3);
    assert_eq!(toc.total_len(), fs.len() as u64);
    let files: Vec<_> = toc.files().collect();
    assert_eq!(files[0].name, "init-lite");
    assert_eq!((files[0].offset, files[0].len), (PAGE, 5000));
    assert_eq!((files[0].stack_pages, files[0].global_pointer), (16, 0x1234));
    assert_eq!((files[1].offset, files[1].len), (3 * PAGE, 1));
    assert_eq!((files[2].offset, files[2].len), (4 * PAGE, 4096));
    assert_eq!(fs.len() as u64, 5 * PAGE);
    assert_eq!(toc.find("execd").unwrap().offset, 3 * PAGE);
    assert!(toc.find("nobody").is_none());
    // The bytes are where the table says.
    let f = files[0];
    assert!(fs[f.offset as usize..(f.offset + f.len) as usize].iter().all(|&b| b == 1));
}

#[test]
fn test_reject_toc_magic_version_reserved_and_truncation() {
    let fs = bootfs_of(&[("a", 10, 1, 0)]);
    assert_eq!(Toc::parse(&fs[..100]).unwrap_err(), Error::Truncated);
    let mut bad = fs.clone();
    bad[0] = b'X';
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Magic);
    let mut bad = fs.clone();
    bad[8] = 9;
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Version);
    let mut bad = fs.clone();
    bad[12] = 1;
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Reserved);
    let mut bad = fs.clone();
    bad[toc::HEADER_LEN + 52] = 1; // the entry's flags
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Reserved);
}

#[test]
fn test_reject_toc_entries_off_grid_out_of_order_or_beyond_the_end() {
    let fs = bootfs_of(&[("a", 10, 1, 0), ("b", 10, 1, 0)]);
    let e1 = toc::HEADER_LEN + toc::ENTRY_LEN;
    // b's offset not page-aligned.
    let mut bad = fs.clone();
    bad[e1 + 32..e1 + 40].copy_from_slice(&(2 * PAGE + 8).to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Alignment);
    // b where a is (overlap).
    let mut bad = fs.clone();
    bad[e1 + 32..e1 + 40].copy_from_slice(&PAGE.to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Layout);
    // a gap before b.
    let mut bad = fs.clone();
    bad[e1 + 32..e1 + 40].copy_from_slice(&(3 * PAGE).to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Layout);
    // b longer than the bootfs.
    let mut bad = fs.clone();
    bad[e1 + 40..e1 + 48].copy_from_slice(&(2 * PAGE).to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Layout);
    // an empty file.
    let mut bad = fs.clone();
    bad[e1 + 40..e1 + 48].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Empty);
    // total_len not what the files add up to.
    let mut bad = fs.clone();
    bad[16..24].copy_from_slice(&(4 * PAGE).to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Layout);
    // more files than the page holds.
    let mut bad = fs.clone();
    bad[10..12].copy_from_slice(&(toc::MAX_FILES as u16 + 1).to_le_bytes());
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::TooManyFiles);
}

#[test]
fn test_reject_toc_names() {
    let fs = bootfs_of(&[("a", 10, 1, 0), ("b", 10, 1, 0)]);
    let e1 = toc::HEADER_LEN + toc::ENTRY_LEN;
    let mut bad = fs.clone();
    bad[e1] = b'a'; // b renamed a
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::DuplicateName);
    let mut bad = fs.clone();
    bad[e1] = 0; // empty name
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Name);
    let mut bad = fs.clone();
    bad[e1] = b' '; // not plain ASCII
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Name);
    let mut bad = fs.clone();
    bad[e1 + 5] = b'x'; // a byte after the NUL padding began
    assert_eq!(Toc::parse(&bad).unwrap_err(), Error::Name);

    let mut b = Builder::new();
    assert_eq!(b.add("", &[1], 1, 0).unwrap_err(), Error::Name);
    assert_eq!(b.add("has space", &[1], 1, 0).unwrap_err(), Error::Name);
    assert_eq!(b.add(&"x".repeat(33), &[1], 1, 0).unwrap_err(), Error::Name);
    assert_eq!(b.add("empty", &[], 1, 0).unwrap_err(), Error::Empty);
    b.add("twice", &[1], 1, 0).unwrap();
    assert_eq!(b.add("twice", &[1], 1, 0).unwrap_err(), Error::DuplicateName);
}

#[test]
fn a_builder_refuses_the_sixty_fourth_file() {
    let mut b = Builder::new();
    for i in 0..toc::MAX_FILES {
        b.add(&format!("f{i}"), &[1], 1, 0).unwrap();
    }
    assert_eq!(b.add("one-more", &[1], 1, 0).unwrap_err(), Error::TooManyFiles);
}

#[test]
fn a_payload_round_trips_and_places_the_bootfs_past_the_kernel_on_a_sector() {
    let kernel = vec![0xAAu8; 1000];
    let fs = bootfs_of(&[("init-lite", 10, 16, 0)]);
    let bytes = payload::compose(&kernel, 9_000_000, &fs).unwrap();
    let h = payload::decode(&bytes).unwrap();
    assert_eq!(h.kernel_offset, SECTOR);
    assert_eq!(h.kernel_len, 1000);
    assert_eq!(h.kernel_footprint, 9_000_000);
    assert_eq!(h.bootfs_offset, SECTOR + 1024); // 512 + 1000 rounded up to 512
    assert_eq!(h.bootfs_len, fs.len() as u64);
    assert_eq!(bytes.len() as u64, h.total_len().unwrap());
    assert_eq!(bytes.len() % SECTOR as usize, 0);
    assert_eq!(&bytes[SECTOR as usize..SECTOR as usize + 1000], &kernel[..]);
    let b = h.bootfs_offset as usize;
    assert_eq!(&bytes[b..b + fs.len()], &fs[..]);
    // The bootfs inside the payload is a table again.
    assert_eq!(Toc::parse(&bytes[b..]).unwrap().len(), 1);
}

#[test]
fn test_reject_payload_header() {
    let kernel = vec![1u8; 1000];
    let fs = bootfs_of(&[("a", 10, 1, 0)]);
    let good = payload::compose(&kernel, 2000, &fs).unwrap();
    let mut bad = good.clone();
    bad[0] = b'?';
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Magic);
    assert_eq!(payload::decode(&good[..100]).unwrap_err(), Error::Truncated);
    let mut bad = good.clone();
    bad[100] = 1;
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Reserved);
    // The kernel item anywhere but right after the header.
    let mut bad = good.clone();
    bad[8..16].copy_from_slice(&1024u64.to_le_bytes());
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Layout);
    // The bootfs item not where the kernel item ends.
    let mut bad = good.clone();
    bad[32..40].copy_from_slice(&(SECTOR + 2048).to_le_bytes());
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Layout);
    // A bootfs that is not whole pages.
    let mut bad = good.clone();
    bad[40..48].copy_from_slice(&(PAGE + 1).to_le_bytes());
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Alignment);
    // A footprint that cannot hold the kernel's bytes.
    let mut bad = good.clone();
    bad[24..32].copy_from_slice(&10u64.to_le_bytes());
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Footprint);
    // Empty items.
    let mut bad = good.clone();
    bad[16..24].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(payload::decode(&bad).unwrap_err(), Error::Empty);
    assert_eq!(payload::compose(&[], 1, &fs).unwrap_err(), Error::Empty);
    assert_eq!(payload::compose(&kernel, 1, &fs).unwrap_err(), Error::Footprint);
    assert!(Header::for_items(u64::MAX, u64::MAX, 1).is_none());
}
