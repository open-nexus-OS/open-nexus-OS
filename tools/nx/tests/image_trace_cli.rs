// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Process-boundary tests for the boot trace's reader (`nx image trace`, RFC-0107,
//! TASK-0327B P1): an image built from the layout carries the `trace` partition; boots kept in it
//! through the loader's own writer read back — the latest by default, every kept boot oldest first
//! with `--all`, one run's boots with `--since`, the loader's text alone with `--loader`, byte for
//! byte with `--out` — from the disk and from a dump of the partition alone; the ring keeps the
//! last eight boots; a damaged header or a forged length drops its boot instead of showing it; and
//! what holds no trace or no boot is refused.
//! OWNERS: @runtime @devx
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 integration tests

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use storage::gpt::parse_gpt;
use storage::trace::{self, Header, LoaderTrace, LOADER_REGION, OS_COMPLETE, SECTOR, SLOT_BYTES};
use storage::{BlockDevice, BlockError};

const OS_SEED_HEX: &str = "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c";

fn run_nx(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("nx process must run")
}

/// A QEMU disk image from the layout, as the harness builds it.
fn build(dir: &Path) {
    std::fs::write(dir.join("os.seed"), OS_SEED_HEX).expect("os seed");
    let kernel: Vec<u8> = (0..200_000u32).map(|i| (i % 241) as u8).collect();
    std::fs::write(dir.join("kernel.bin"), kernel).expect("kernel");
    let out = run_nx(
        &[
            "image",
            "build",
            "--kernel",
            "kernel.bin",
            "--out",
            "disk.img",
            "--sign",
            "os.seed",
            "--build-id",
            "dev-T",
        ],
        dir,
    );
    assert!(out.status.success(), "image build: {}", String::from_utf8_lossy(&out.stdout));
}

/// The image file as the loader's block device.
struct FileDisk(File);

impl FileDisk {
    fn open(path: &Path) -> Self {
        Self(OpenOptions::new().read(true).write(true).open(path).expect("disk"))
    }
}

impl BlockDevice for FileDisk {
    fn block_size(&self) -> usize {
        SECTOR
    }

    fn block_count(&self) -> u64 {
        self.0.metadata().map(|m| m.len() / SECTOR as u64).unwrap_or(0)
    }

    fn read_block(&self, idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let at = idx * SECTOR as u64;
        self.0.read_exact_at(&mut buf[..SECTOR], at).map_err(|_| BlockError::IoError)
    }

    fn write_block(&mut self, idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        let at = idx * SECTOR as u64;
        self.0.write_all_at(&buf[..SECTOR], at).map_err(|_| BlockError::IoError)
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

/// One boot as the loader keeps it: its text at the first milestone, then all of it, complete.
/// Returns the slot's first LBA.
fn keep_boot(dir: &Path, text: &str) -> u64 {
    let mut disk = FileDisk::open(&dir.join("disk.img"));
    let mut t = LoaderTrace::open(&disk).expect("the layout's trace partition");
    let first = text.find('\n').map_or(text.len(), |i| i + 1);
    t.write(&mut disk, &text.as_bytes()[..first], false, false).expect("first milestone");
    t.write(&mut disk, text.as_bytes(), false, true).expect("last milestone");
    t.slot_lba()
}

/// The slot's header as it is on the disk.
fn header_at(disk: &FileDisk, lba: u64) -> Header {
    let mut sector = [0u8; SECTOR];
    disk.read_block(lba, &mut sector).expect("header");
    Header::decode(&sector).expect("a valid header")
}

/// The OS region of a kept boot, as Phase 2's writer appends it: the text, then the header.
fn keep_os(dir: &Path, lba: u64, text: &str) {
    let mut disk = FileDisk::open(&dir.join("disk.img"));
    let mut header = header_at(&disk, lba);
    let mut padded = vec![0u8; text.len().div_ceil(SECTOR) * SECTOR];
    padded[..text.len()].copy_from_slice(text.as_bytes());
    let os_at = lba + 1 + (LOADER_REGION / SECTOR) as u64;
    disk.write_blocks(os_at, &padded).expect("os text");
    header.os_len = text.len() as u32;
    header.flags |= OS_COMPLETE;
    disk.write_block(lba, &header.encode()).expect("os header");
}

fn out_bytes(dir: &Path, args: &[&str]) -> Vec<u8> {
    let mut all = vec!["image", "trace", "--out", "trace.out"];
    all.extend_from_slice(args);
    let out = run_nx(&all, dir);
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
    std::fs::read(dir.join("trace.out")).expect("trace.out")
}

fn meta(dir: &Path, args: &[&str]) -> Value {
    let mut all = vec!["image", "trace", "--json"];
    all.extend_from_slice(args);
    let out = run_nx(&all, dir);
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
    let v: Value = serde_json::from_slice(&out.stdout).expect("json");
    v["data"].clone()
}

fn seqs(meta: &Value) -> Vec<u64> {
    meta["boots"]
        .as_array()
        .expect("boots")
        .iter()
        .map(|b| b["seq"].as_u64().expect("seq"))
        .collect()
}

const ONE: &str =
    "nxboot: trace slot=0 seq=1\nnxboot: bsb ok (slot=a seq=1)\nnxboot: jump slot=a\n";
const TWO: &str = "nxboot: trace slot=1 seq=2\nnxboot: verify FAIL (slot=a digest)\n";
const THREE: &str = "nxboot: trace slot=2 seq=3\nnxboot: jump slot=b base=0x80200000\n";
const THREE_OS: &str = "neuron vers. 0.1.0\ninit: ready\n";

#[test]
fn the_latest_boot_by_default_every_kept_boot_with_all_and_the_loader_alone_with_loader() {
    let dir = tempfile::tempdir().expect("tempdir");
    build(dir.path());
    keep_boot(dir.path(), ONE);
    keep_boot(dir.path(), TWO);
    let lba = keep_boot(dir.path(), THREE);
    keep_os(dir.path(), lba, THREE_OS);
    let img = ["--image", "disk.img"];

    let shown = run_nx(&["image", "trace", "--image", "disk.img"], dir.path());
    assert!(shown.status.success());
    assert_eq!(String::from_utf8_lossy(&shown.stdout), format!("{THREE}{THREE_OS}"));
    assert_eq!(out_bytes(dir.path(), &img), format!("{THREE}{THREE_OS}").as_bytes());
    let all = [img[0], img[1], "--all"];
    assert_eq!(out_bytes(dir.path(), &all), format!("{ONE}{TWO}{THREE}{THREE_OS}").as_bytes());
    let loader = [img[0], img[1], "--loader"];
    assert_eq!(out_bytes(dir.path(), &loader), THREE.as_bytes(), "the OS region left out");
    let both = [img[0], img[1], "--all", "--loader"];
    assert_eq!(out_bytes(dir.path(), &both), format!("{ONE}{TWO}{THREE}").as_bytes());
    let run = [img[0], img[1], "--since", "2", "--loader"];
    assert_eq!(out_bytes(dir.path(), &run), format!("{TWO}{THREE}").as_bytes(), "one run's boots");
    let os = [img[0], img[1], "--seq", "3", "--os"];
    assert_eq!(out_bytes(dir.path(), &os), THREE_OS.as_bytes(), "one boot's OS text");
    let two = [img[0], img[1], "--seq", "2"];
    assert_eq!(out_bytes(dir.path(), &two), TWO.as_bytes(), "one boot, no OS text kept");
    let both = run_nx(&["image", "trace", "--image", "disk.img", "--os", "--loader"], dir.path());
    assert_eq!(both.status.code(), Some(2), "--os and --loader exclude each other");

    let m = meta(dir.path(), &all);
    assert_eq!((m["kept"].as_u64(), seqs(&m)), (Some(3), vec![1, 2, 3]));
    for (b, (slot, os_bytes, os_complete)) in m["boots"].as_array().expect("boots").iter().zip([
        (0, 0, false),
        (1, 0, false),
        (2, 31, true),
    ]) {
        assert_eq!(b["slot"], slot);
        assert_eq!(b["loader_complete"], true, "the loader's last write marks its region");
        assert_eq!(b["loader_overflow"], false);
        assert_eq!(
            (b["os_bytes"].as_u64(), b["os_complete"].as_bool()),
            (Some(os_bytes), Some(os_complete))
        );
    }
}

#[test]
fn a_dump_of_the_partition_alone_reads_like_the_disk_and_both_keep_the_last_eight_boots() {
    let dir = tempfile::tempdir().expect("tempdir");
    build(dir.path());
    for n in 1..=10u32 {
        keep_boot(dir.path(), &format!("nxboot: boot {n}\nnxboot: jump slot=a\n"));
    }
    let disk = FileDisk::open(&dir.path().join("disk.img"));
    let part = trace::partition(&parse_gpt(&disk).expect("gpt")).expect("trace partition");
    assert_eq!((part.last_lba + 1 - part.first_lba) * SECTOR as u64, 8 * SLOT_BYTES as u64);
    let mut dump = vec![0u8; 8 * SLOT_BYTES];
    disk.read_blocks(part.first_lba, &mut dump).expect("partition bytes");
    std::fs::write(dir.path().join("trace.part"), dump).expect("dump");

    let from_disk = out_bytes(dir.path(), &["--image", "disk.img", "--all"]);
    let from_dump = out_bytes(dir.path(), &["--image", "trace.part", "--all"]);
    assert_eq!(from_disk, from_dump, "the same boots, byte for byte");
    let want: String =
        (3..=10).map(|n| format!("nxboot: boot {n}\nnxboot: jump slot=a\n")).collect();
    assert_eq!(String::from_utf8_lossy(&from_disk), want, "the last eight boots, oldest first");
    let m = meta(dir.path(), &["--image", "trace.part", "--all"]);
    assert_eq!((m["kept"].as_u64(), seqs(&m)), (Some(8), (3..=10).collect::<Vec<u64>>()));
    let slots: Vec<u64> = m["boots"]
        .as_array()
        .expect("boots")
        .iter()
        .map(|b| b["slot"].as_u64().expect("slot"))
        .collect();
    assert_eq!(slots, vec![2, 3, 4, 5, 6, 7, 0, 1], "boot 9 took slot 0 over, boot 10 slot 1");
}

#[test]
fn test_reject_a_damaged_header_and_a_forged_length_drop_their_boot() {
    let dir = tempfile::tempdir().expect("tempdir");
    build(dir.path());
    keep_boot(dir.path(), ONE);
    let two = keep_boot(dir.path(), TWO);
    let three = keep_boot(dir.path(), THREE);

    // Boot 3: one bit of its loader length flipped — still in bounds, but the CRC no longer
    // matches.
    let mut disk = FileDisk::open(&dir.path().join("disk.img"));
    let mut sector = [0u8; SECTOR];
    disk.read_block(three, &mut sector).expect("header");
    sector[24] ^= 1;
    disk.write_block(three, &sector).expect("damage");
    // Boot 2: a header with a valid CRC whose length runs past the loader's region.
    let mut forged = header_at(&disk, two);
    forged.loader_len = LOADER_REGION as u32 + 1;
    disk.write_block(two, &forged.encode()).expect("forge");

    let m = meta(dir.path(), &["--image", "disk.img", "--all"]);
    assert_eq!((m["kept"].as_u64(), seqs(&m)), (Some(1), vec![1]), "only the intact boot counts");
    assert_eq!(out_bytes(dir.path(), &["--image", "disk.img"]), ONE.as_bytes());
}

#[test]
fn test_reject_what_holds_no_trace_or_no_boot() {
    let dir = tempfile::tempdir().expect("tempdir");
    build(dir.path());
    let text = |o: &Output| String::from_utf8_lossy(&o.stdout).into_owned();

    let fresh = run_nx(&["image", "trace", "--image", "disk.img"], dir.path());
    assert_eq!(fresh.status.code(), Some(3), "no boot kept yet: {}", text(&fresh));
    assert!(text(&fresh).contains("keeps no boot"));

    std::fs::write(dir.path().join("small.bin"), vec![0u8; 4096]).expect("small");
    let small = run_nx(&["image", "trace", "--image", "small.bin"], dir.path());
    assert_eq!(small.status.code(), Some(3), "neither a disk nor a dump: {}", text(&small));
    assert!(text(&small).contains("neither a disk with a trace partition nor a dump of one"));

    keep_boot(dir.path(), ONE);
    let later = run_nx(&["image", "trace", "--image", "disk.img", "--since", "2"], dir.path());
    assert_eq!(later.status.code(), Some(3), "no boot from seq 2 on: {}", text(&later));
    assert!(text(&later).contains("keeps no boot from seq 2 on (the latest is 1)"));
    let absent = run_nx(&["image", "trace", "--image", "disk.img", "--seq", "9"], dir.path());
    assert_eq!(absent.status.code(), Some(3), "no boot 9: {}", text(&absent));
    assert!(text(&absent).contains("keeps no boot 9 (the latest is 1)"));

    let missing = run_nx(&["image", "trace", "--image", "missing.img"], dir.path());
    assert_eq!(missing.status.code(), Some(4), "{}", text(&missing));
    assert!(!dir.path().join("trace.out").exists(), "nothing written on a refusal");
}
