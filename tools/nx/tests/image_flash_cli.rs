// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the flash plan of a board image (`nx image flash-plan`,
//! TASK-0260 P2): its raw regions rebuild the disk exactly — the user area from sector 0 to the
//! end of its last partition in chunks no larger than one download, the backup GPT in the disk's
//! last 33 sectors, boot0 on hardware partition 1 — each file's digest the plan's; the same image
//! gives the same plan byte for byte; and what is no board disk is refused.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 integration tests

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};
use storage::gpt::crc32_ieee;

const OS_SEED_HEX: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
/// The fixture board's disk: 1 GiB (sparse); its boot0 1 MiB.
const BOARD_SECTORS: u64 = 2 * 1024 * 1024;
const BOOT0_SECTORS: u64 = 2048;
/// 128 MiB chunks: the fixture's 373 MiB span becomes three.
const CHUNK_SECTORS: u64 = 262_144;

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

fn setup(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    let kernel: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
    std::fs::create_dir_all(dir.join("board/head")).expect("head dir");
    let mut header = vec![0u8; 80];
    header[..4].copy_from_slice(&0xB007_14F0u32.to_le_bytes());
    header[8..12].copy_from_slice(b"eMMC");
    header[0x20..0x24].copy_from_slice(&0x200u32.to_le_bytes());
    header[0x28..0x2C].copy_from_slice(&0x1_0000u32.to_le_bytes());
    let crc = crc32_ieee(&header[..64]);
    header[64..68].copy_from_slice(&crc.to_le_bytes());
    std::fs::write(dir.join("board/head/header.bin"), header).expect("header");
    std::fs::write(dir.join("board/head/spl.bin"), vec![0xF5u8; 60_000]).expect("spl");
    std::fs::write(dir.join("board/head/opensbi.itb"), vec![0x05u8; 130_000]).expect("opensbi");
    let profile = format!(
        "[disk]\nsectors = {BOARD_SECTORS}\n\n[boot0]\nsectors = {BOOT0_SECTORS}\n\
         header = \"head/header.bin\"\nspl = \"head/spl.bin\"\n\n[head]\nopensbi = \"head/opensbi.itb\"\n"
    );
    std::fs::write(dir.join("board/image.toml"), profile).expect("profile");
}

fn build(dir: &Path, out: &str, board: bool) -> Output {
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
        "dev-F",
    ];
    if board {
        args.extend_from_slice(&["--target", "test-board", "--board-profile", "board/image.toml"]);
    }
    run_nx(&args, dir)
}

fn plan(dir: &Path, image: &str, out_dir: &str, chunk: &str) -> Output {
    run_nx(
        &["image", "flash-plan", "--image", image, "--out-dir", out_dir, "--chunk-bytes", chunk],
        dir,
    )
}

/// Streamed digest of `len` bytes at `offset` of each (file, offset, len), in order.
fn digest_of(parts: &[(&Path, u64, u64)]) -> String {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    for (path, offset, len) in parts {
        let mut f = std::fs::File::open(path).expect("open");
        f.seek(SeekFrom::Start(*offset)).expect("seek");
        let mut left = *len;
        while left > 0 {
            let n = buf.len().min(left as usize);
            f.read_exact(&mut buf[..n]).expect("read");
            hasher.update(&buf[..n]);
            left -= n as u64;
        }
    }
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_regions_rebuild_the_disk_exactly() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "b.img", true).status.success(), "board image");
    let out = plan(dir.path(), "b.img", "flash", "128M");
    assert!(out.status.success(), "plan: {}", String::from_utf8_lossy(&out.stdout));
    let text = std::fs::read_to_string(dir.path().join("flash/plan.json")).expect("plan.json");
    let plan: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(plan["disk_sectors"], BOARD_SECTORS);
    let regions = plan["regions"].as_array().expect("regions");
    let field = |r: &serde_json::Value, k: &str| r[k].as_u64().expect(k);
    // The user area: chunks from sector 0, back to back, to the end of the last partition.
    let span =
        storage::layout::plan().expect("layout").iter().map(|p| p.last_lba + 1).max().unwrap();
    let disk: Vec<&serde_json::Value> =
        regions.iter().filter(|r| r["name"] != "nxgpt" && r["name"] != "nxboot0").collect();
    let mut next = 0;
    for r in &disk {
        assert_eq!((field(r, "hwpart"), field(r, "start_lba")), (0, next), "{}", r["name"]);
        assert!(field(r, "sectors") <= CHUNK_SECTORS && field(r, "sectors") > 0);
        next += field(r, "sectors");
    }
    assert_eq!((next, disk.len()), (span, 3), "chunks up to the end of the last partition");
    let image = dir.path().join("b.img");
    let chunk_files: Vec<std::path::PathBuf> =
        disk.iter().map(|r| dir.path().join("flash").join(r["file"].as_str().unwrap())).collect();
    let chunks: Vec<(&Path, u64, u64)> = chunk_files
        .iter()
        .zip(&disk)
        .map(|(f, r)| (f.as_path(), 0, field(r, "sectors") * 512))
        .collect();
    assert_eq!(
        digest_of(&chunks),
        digest_of(&[(image.as_path(), 0, span * 512)]),
        "the chunks rebuild it"
    );
    // The backup GPT: the disk's last 33 sectors; boot0: hardware partition 1, whole.
    let gpt = regions.iter().find(|r| r["name"] == "nxgpt").expect("nxgpt");
    assert_eq!(
        (field(gpt, "hwpart"), field(gpt, "start_lba"), field(gpt, "sectors")),
        (0, BOARD_SECTORS - 33, 33)
    );
    let boot0 = regions.iter().find(|r| r["name"] == "nxboot0").expect("nxboot0");
    assert_eq!(
        (field(boot0, "hwpart"), field(boot0, "start_lba"), field(boot0, "sectors")),
        (1, 0, BOOT0_SECTORS)
    );
    assert_eq!(
        std::fs::read(dir.path().join("flash/nxboot0.img")).expect("boot0 region"),
        std::fs::read(dir.path().join("b.img.boot0")).expect("boot0")
    );
    // Every file is what the plan says it is.
    for r in regions {
        let file = dir.path().join("flash").join(r["file"].as_str().unwrap());
        let len = std::fs::metadata(&file).expect("meta").len();
        assert_eq!(len, field(r, "sectors") * 512, "{}", r["name"]);
        assert_eq!(
            digest_of(&[(file.as_path(), 0, len)]),
            r["sha256"].as_str().unwrap(),
            "{}",
            r["name"]
        );
    }
    let backup = digest_of(&[(image.as_path(), (BOARD_SECTORS - 33) * 512, 33 * 512)]);
    assert_eq!(gpt["sha256"].as_str().unwrap(), backup);
}

#[test]
fn the_same_image_gives_the_same_plan() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "b.img", true).status.success(), "board image");
    assert!(plan(dir.path(), "b.img", "one", "128M").status.success(), "plan one");
    assert!(plan(dir.path(), "b.img", "two", "128M").status.success(), "plan two");
    let read = |d: &str| std::fs::read(dir.path().join(d).join("plan.json")).expect("plan");
    assert_eq!(read("one"), read("two"), "byte-identical plans (the files' digests included)");
}

#[test]
fn test_reject_what_is_no_board_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    setup(dir.path());
    assert!(build(dir.path(), "q.img", false).status.success(), "QEMU image");
    let out = plan(dir.path(), "q.img", "q", "128M");
    assert_eq!(out.status.code(), Some(4), "no boot0: {}", String::from_utf8_lossy(&out.stdout));
    assert!(build(dir.path(), "b.img", true).status.success(), "board image");
    let out = plan(dir.path(), "b.img", "c", "100K");
    assert_eq!(out.status.code(), Some(3), "a chunk below 1 MiB");
    // A disk grown past its GPT's backup is not the disk the GPT names.
    let grown =
        std::fs::OpenOptions::new().write(true).open(dir.path().join("b.img")).expect("open");
    grown.set_len((BOARD_SECTORS + 2048) * 512).expect("grow");
    let out = plan(dir.path(), "b.img", "g", "128M");
    assert_eq!(
        out.status.code(),
        Some(3),
        "backup not at the end: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    std::fs::write(dir.path().join("z.img"), vec![0u8; 1 << 20]).expect("zeros");
    assert_eq!(plan(dir.path(), "z.img", "z", "128M").status.code(), Some(3), "no GPT");
}
