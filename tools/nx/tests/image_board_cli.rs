// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the disk an image is built for (`nx image build
//! --target`, TASK-0260 P1, RFC-0089 §2). QEMU's image at the size a lane asks for, its GPT's
//! backup at that size's end. A board's image from a fixture profile: exactly the disk's size,
//! the boot-ROM header in sector 0 beside the protective MBR, the head's pieces in their
//! partitions, the volumes byte-identical to QEMU's. And the refusals: a header the boot ROM
//! would refuse, a piece or a disk that does not fit, a board given a size, a profile field
//! the builder does not know.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 integration tests

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use storage::gpt::crc32_ieee;

const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
/// The fixture board's disk: 1 GiB (sparse).
const BOARD_SECTORS: u64 = 2 * 1024 * 1024;
const HEAD: [&str; 4] = ["fsbl", "env", "opensbi", "uboot"];

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

/// A boot-ROM header as the boot ROM checks it: the magic, a CRC-32 of the first 64 bytes.
fn boot_header() -> Vec<u8> {
    let mut header = vec![0u8; 80];
    header[..4].copy_from_slice(&0xB007_14F0u32.to_le_bytes());
    header[8..16].copy_from_slice(b"FIXTURE\0");
    let crc = crc32_ieee(&header[..64]);
    header[64..68].copy_from_slice(&crc.to_le_bytes());
    header
}

fn profile(sectors: u64, extra: &str) -> String {
    format!(
        "[disk]\nsectors = {sectors}\n\n[head]\nboot_header = \"head/header.bin\"\n\
         fsbl = \"head/fsbl.bin\"\nopensbi = \"head/opensbi.itb\"\n{extra}"
    )
}

fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
    std::fs::create_dir_all(dir.join("board/head")).expect("head dir");
    std::fs::write(dir.join("board/head/header.bin"), boot_header()).expect("header");
    std::fs::write(dir.join("board/head/fsbl.bin"), vec![0xF5u8; 200_000]).expect("fsbl");
    std::fs::write(dir.join("board/head/opensbi.itb"), vec![0x05u8; 130_000]).expect("opensbi");
    std::fs::write(dir.join("board/image.toml"), profile(BOARD_SECTORS, "")).expect("profile");
    std::fs::write(dir.join("fit.itb"), vec![0xF1u8; 300_000]).expect("fit");
}

fn build(dir: &Path, out: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "image",
        "build",
        "--kernel",
        "kernel.bin",
        "--out",
        out,
        "--sign",
        "os.seed",
        "--build-id",
        "dev-T",
    ];
    args.extend_from_slice(extra);
    run_nx(&args, dir)
}

const BOARD: [&str; 6] =
    ["--target", "test-board", "--board-profile", "board/image.toml", "--fit", "fit.itb"];

fn read_at(path: &Path, offset: u64, len: usize) -> Vec<u8> {
    let mut f = std::fs::File::open(path).expect("open");
    f.seek(SeekFrom::Start(offset)).expect("seek");
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).expect("read");
    buf
}

/// Streamed digest of `[first_lba, last_lba]` (a volume is up to 128 MiB — never slurped).
fn region_sha(path: &Path, first_lba: u64, last_lba: u64) -> [u8; 32] {
    let mut f = std::fs::File::open(path).expect("open");
    f.seek(SeekFrom::Start(first_lba * 512)).expect("seek");
    let mut left = (last_lba + 1 - first_lba) * 512;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    while left > 0 {
        let n = buf.len().min(left as usize);
        f.read_exact(&mut buf[..n]).expect("read");
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    hasher.finalize().into()
}

fn u64_at(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().expect("8 bytes"))
}

#[test]
fn a_qemu_disk_of_the_size_a_lane_asks_for_carries_its_backup_at_the_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let out = build(dir.path(), "q.img", &["--disk-bytes", "1G"]);
    assert!(out.status.success(), "build: {}", String::from_utf8_lossy(&out.stdout));
    let image = dir.path().join("q.img");
    let sectors = std::fs::metadata(&image).expect("meta").len() / 512;
    assert_eq!(sectors, 2 * 1024 * 1024);
    let primary = read_at(&image, 512, 512);
    assert_eq!(u64_at(&primary, 32), sectors - 1, "alternate = the last sector");
    let backup = read_at(&image, (sectors - 1) * 512, 512);
    assert_eq!((&backup[..8], u64_at(&backup, 24)), (&b"EFI PART"[..], sectors - 1));
    let verify = run_nx(&["image", "verify", "--image", "q.img", "--key", "os.seed"], dir.path());
    assert!(verify.status.success(), "verify: {}", String::from_utf8_lossy(&verify.stdout));
}

#[test]
fn a_board_image_is_its_disk_byte_for_byte() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let out = build(dir.path(), "b.img", &BOARD);
    assert!(out.status.success(), "build: {}", String::from_utf8_lossy(&out.stdout));
    assert!(build(dir.path(), "q.img", &[]).status.success(), "the same inputs for QEMU");
    let (board, qemu) = (dir.path().join("b.img"), dir.path().join("q.img"));
    assert_eq!(std::fs::metadata(&board).expect("meta").len(), BOARD_SECTORS * 512);
    // Sector 0: the boot-ROM header, then the protective MBR over the board's disk.
    let sector0 = read_at(&board, 0, 512);
    assert_eq!(&sector0[..80], boot_header().as_slice());
    assert!(sector0[80..446].iter().all(|b| *b == 0));
    assert_eq!(sector0[450], 0xEE);
    assert_eq!(&sector0[454..462], &[1, 0, 0, 0, 0xFF, 0xFF, 0x1F, 0x00], "LBA 1, disk − 1");
    assert_eq!(&sector0[510..512], &[0x55, 0xAA]);
    let backup = read_at(&board, (BOARD_SECTORS - 1) * 512, 512);
    assert_eq!(u64_at(&backup, 24), BOARD_SECTORS - 1, "the backup at the disk's end");
    // The head: each piece in its partition, `env` zero; QEMU's head all zero.
    let layout = storage::layout::plan().expect("layout");
    let pieces = [
        ("fsbl", "board/head/fsbl.bin"),
        ("opensbi", "board/head/opensbi.itb"),
        ("uboot", "fit.itb"),
    ];
    for (name, file) in pieces {
        let p = layout.iter().find(|p| p.name == name).expect("head partition");
        let want = std::fs::read(dir.path().join(file)).expect("piece");
        assert_eq!(read_at(&board, p.first_lba * 512, want.len()), want, "{name}");
    }
    for p in layout.iter().filter(|p| HEAD.contains(&p.name.as_str())) {
        let bytes =
            read_at(&qemu, p.first_lba * 512, ((p.last_lba + 1 - p.first_lba) * 512) as usize);
        assert!(bytes.iter().all(|b| *b == 0), "QEMU's {} is zero", p.name);
        if p.name == "env" {
            let env = read_at(&board, p.first_lba * 512, bytes.len());
            assert!(env.iter().all(|b| *b == 0), "the board's env is zero");
        }
    }
    // The volumes do not depend on the disk they land on.
    for p in layout.iter().filter(|p| !HEAD.contains(&p.name.as_str())) {
        let (b, q) = (
            region_sha(&board, p.first_lba, p.last_lba),
            region_sha(&qemu, p.first_lba, p.last_lba),
        );
        assert_eq!(b, q, "{} identical on both disks", p.name);
    }
    let verify = run_nx(&["image", "verify", "--image", "b.img", "--key", "os.seed"], dir.path());
    assert!(verify.status.success(), "verify: {}", String::from_utf8_lossy(&verify.stdout));
}

#[test]
fn test_reject_a_boot_header_the_boot_rom_would_refuse() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let header = dir.path().join("board/head/header.bin");
    let mut bad_crc = boot_header();
    bad_crc[20] ^= 0x01;
    let mut bad_magic = boot_header();
    bad_magic[0] ^= 0x01;
    let crc = crc32_ieee(&bad_magic[..64]);
    bad_magic[64..68].copy_from_slice(&crc.to_le_bytes());
    for (what, bytes) in
        [("crc", bad_crc), ("magic", bad_magic), ("length", boot_header()[..79].to_vec())]
    {
        std::fs::write(&header, &bytes).expect("header");
        let out = build(dir.path(), "b.img", &BOARD);
        assert_eq!(out.status.code(), Some(3), "{what}: {}", String::from_utf8_lossy(&out.stdout));
        assert!(!dir.path().join("b.img").exists(), "{what}: nothing built");
    }
}

#[test]
fn test_reject_a_disk_or_a_piece_that_does_not_fit() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    let refused = |extra: &[&str], what: &str| {
        let out = build(dir.path(), "x.img", extra);
        assert_eq!(out.status.code(), Some(3), "{what}: {}", String::from_utf8_lossy(&out.stdout));
    };
    refused(&["--disk-bytes", "100M"], "below the layout");
    refused(&["--disk-bytes", "402653185"], "not whole sectors");
    refused(&["--fit", "fit.itb"], "a FIT without a board");
    refused(&[&BOARD[..], &["--disk-bytes", "1G"]].concat(), "a board given a size");
    refused(&["--target", "../x", "--board-profile", "board/image.toml"], "not a board name");
    let profile_at = dir.path().join("board/image.toml");
    std::fs::write(&profile_at, profile(1000, "")).expect("profile");
    refused(&BOARD, "a disk smaller than the layout");
    std::fs::write(&profile_at, profile(BOARD_SECTORS, "env = \"head/fsbl.bin\"\n")).expect("p");
    refused(&BOARD, "a field the builder does not know");
    std::fs::write(&profile_at, profile(BOARD_SECTORS, "")).expect("profile");
    std::fs::write(dir.path().join("board/head/fsbl.bin"), vec![0xF5u8; 256 * 1024 + 1])
        .expect("f");
    refused(&BOARD, "an SPL larger than fsbl");
}
