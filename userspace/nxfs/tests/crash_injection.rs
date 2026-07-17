// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxfs crash-injection proof (RFC-0071 / TASK-0292): every
//! sector-write prefix of every mutating op must remount to EITHER the
//! pre-op state OR the post-op state — never a torn hybrid. Also proves
//! rename exactly-one-name-visible, idempotent replay, checkpoint-flip
//! torn-write recovery, and the fsck outcome matrix.
//! OWNERS: @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this file IS the coverage (deterministic, no randomness)

use nxfs::{fsck, FsckOutcome, MkfsOptions, Nxfs, LOGICAL_BLOCK_SIZE};
use storage::{BlockDevice, BlockError, MemBlockDevice};

const BLOCKS: u64 = 2048;

/// Records every write in order so tests can replay arbitrary prefixes onto
/// a snapshot (sector-granular crash simulation).
struct SpyDevice {
    inner: MemBlockDevice,
    log: Vec<(u64, Vec<u8>)>,
}

impl SpyDevice {
    fn new(inner: MemBlockDevice) -> Self {
        Self { inner, log: Vec::new() }
    }
}

impl BlockDevice for SpyDevice {
    fn block_size(&self) -> usize {
        self.inner.block_size()
    }
    fn block_count(&self) -> u64 {
        self.inner.block_count()
    }
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read_block(block_idx, buf)
    }
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.log.push((block_idx, buf.to_vec()));
        self.inner.write_block(block_idx, buf)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        self.inner.sync()
    }
}

fn snapshot(device: &MemBlockDevice) -> MemBlockDevice {
    let mut copy = MemBlockDevice::new(device.block_size(), device.block_count());
    let mut buf = vec![0u8; device.block_size()];
    for idx in 0..device.block_count() {
        device.read_block(idx, &mut buf).expect("read");
        copy.write_block(idx, &buf).expect("write");
    }
    copy
}

fn listing(fs: &Nxfs<MemBlockDevice>, path: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut cursor = 0;
    loop {
        let page = fs.read_dir(path, cursor, 64).expect("readdir");
        out.extend(page.entries.iter().map(|e| (e.name.clone(), e.size)));
        cursor = page.next_cursor;
        if page.eof {
            break;
        }
    }
    out
}

/// Full observable state fingerprint: root listing + file contents.
fn fingerprint(
    device: MemBlockDevice,
) -> (Vec<(String, u64)>, Vec<(String, Vec<u8>)>, MemBlockDevice) {
    let fs = Nxfs::mount(device).expect("mount");
    let names = listing(&fs, "/");
    let mut contents = Vec::new();
    for (name, _) in &names {
        let path = format!("/{name}");
        if let Ok(bytes) = fs.read(&path, 0, 1 << 20) {
            contents.push((name.clone(), bytes));
        }
    }
    (names, contents, fs.into_device())
}

fn base_image() -> MemBlockDevice {
    let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, BLOCKS);
    let mut fs =
        Nxfs::mkfs(device, MkfsOptions { uuid: [9; 16], journal_blocks: 32 }).expect("mkfs");
    fs.create("/keep.txt").expect("create");
    fs.write("/keep.txt", 0, b"stable content").expect("write");
    fs.mkdir("/dir").expect("mkdir");
    fs.into_device()
}

/// Runs `op` on a spied copy of `base` and checks EVERY write prefix
/// remounts to pre-state or post-state.
fn assert_crash_atomic(op: impl Fn(&mut Nxfs<SpyDevice>)) {
    let base = base_image();
    let (pre_names, pre_contents, base) = fingerprint(base);

    // Run the op fully on a spy to capture the write log + post state.
    let spy = SpyDevice::new(snapshot(&base));
    let mut fs = Nxfs::mount(spy).expect("mount spy");
    op(&mut fs);
    let spy = fs.into_device();
    let log = spy.log;
    let (post_names, post_contents, _) = fingerprint(spy.inner);

    for cut in 0..=log.len() {
        let mut image = snapshot(&base);
        for (idx, data) in &log[..cut] {
            image.write_block(*idx, data).expect("replay write");
        }
        let (names, contents, _) = fingerprint(image);
        let is_pre = names == pre_names && contents == pre_contents;
        let is_post = names == post_names && contents == post_contents;
        assert!(is_pre || is_post, "cut={cut}/{}: torn state visible: {names:?}", log.len());
    }
}

#[test]
fn create_write_is_crash_atomic() {
    assert_crash_atomic(|fs| {
        fs.create("/new.txt").expect("create");
    });
    assert_crash_atomic(|fs| {
        fs.write("/keep.txt", 0, b"REPLACED CONTENT LONGER THAN BEFORE").expect("write");
    });
}

#[test]
fn rename_is_exactly_one_name_visible() {
    assert_crash_atomic(|fs| {
        fs.rename("/keep.txt", "/renamed.txt").expect("rename");
    });
    // Replacing rename: target exists before.
    let base = base_image();
    let spy = SpyDevice::new(snapshot(&base));
    let mut fs = Nxfs::mount(spy).expect("mount");
    fs.create("/victim.txt").expect("create");
    fs.write("/victim.txt", 0, b"victim").expect("write");
    let spy = fs.into_device();
    let prepared = spy.inner;

    let spy = SpyDevice::new(snapshot(&prepared));
    let mut fs = Nxfs::mount(spy).expect("mount");
    fs.rename("/keep.txt", "/victim.txt").expect("rename replace");
    let spy = fs.into_device();
    let log = spy.log;

    for cut in 0..=log.len() {
        let mut image = snapshot(&prepared);
        for (idx, data) in &log[..cut] {
            image.write_block(*idx, data).expect("replay write");
        }
        let fs = Nxfs::mount(image).expect("mount");
        let names: Vec<String> = listing(&fs, "/").into_iter().map(|(name, _)| name).collect();
        let has_old = names.contains(&"keep.txt".to_string());
        let has_target = names.contains(&"victim.txt".to_string());
        // Exactly one of: old state (both names, victim is the victim) or
        // new state (only victim.txt, carrying keep's content).
        assert!(has_target, "cut={cut}: target name vanished");
        let content = fs.read("/victim.txt", 0, 64).expect("read");
        if has_old {
            assert_eq!(content, b"victim", "cut={cut}: pre-state content");
        } else {
            assert_eq!(content, b"stable content", "cut={cut}: post-state content");
        }
    }
}

#[test]
fn checkpoint_flip_survives_every_cut() {
    let base = base_image();
    let spy = SpyDevice::new(snapshot(&base));
    let mut fs = Nxfs::mount(spy).expect("mount");
    fs.write_checkpoint().expect("checkpoint");
    let spy = fs.into_device();
    let log = spy.log;
    let (pre_names, pre_contents, _) = fingerprint(snapshot(&base));

    for cut in 0..=log.len() {
        let mut image = snapshot(&base);
        for (idx, data) in &log[..cut] {
            image.write_block(*idx, data).expect("replay write");
        }
        let (names, contents, _) = fingerprint(image);
        assert_eq!(names, pre_names, "cut={cut}: checkpoint must never change state");
        assert_eq!(contents, pre_contents, "cut={cut}");
    }
}

#[test]
fn replay_is_idempotent() {
    let base = base_image();
    let (names_a, contents_a, device) = fingerprint(base);
    let (names_b, contents_b, _) = fingerprint(device);
    assert_eq!(names_a, names_b);
    assert_eq!(contents_a, contents_b);
}

#[test]
fn fsck_outcome_matrix() {
    // Clean container.
    let base = base_image();
    let (report, device) = fsck(base, false);
    assert_eq!(report.outcome, FsckOutcome::Clean);
    let device = device.expect("device back");

    // Torn journal tail: small txns are sector-atomic (a single journal
    // block write), so run MANY ops with long names — some txn's byte run
    // must cross a journal block boundary, and the cut between its two
    // block writes is a real torn record.
    let spy = SpyDevice::new(snapshot(&device));
    let mut fs = Nxfs::mount(spy).expect("mount");
    for i in 0..30 {
        let name = format!("/{}-{}", "n".repeat(180), i);
        fs.create(&name).expect("create");
    }
    let spy = fs.into_device();
    let log = spy.log;
    // Find a cut that yields an orphan (some prefix that started the journal
    // run but did not finish it). Walk cuts until fsck reports one.
    let mut saw_orphan = false;
    for cut in 0..log.len() {
        let mut image = snapshot(&device);
        for (idx, data) in &log[..cut] {
            image.write_block(*idx, data).expect("replay write");
        }
        let (report, repaired_device) = fsck(image, true);
        if report.orphan_tail {
            saw_orphan = true;
            assert_eq!(report.outcome, FsckOutcome::Repaired);
            assert!(report.repaired);
            // After repair the container is clean.
            let (again, _) = fsck(repaired_device.expect("device"), false);
            assert_eq!(again.outcome, FsckOutcome::Clean);
            break;
        }
    }
    assert!(saw_orphan, "at least one cut must produce an orphan tail");

    // Unrecoverable: destroy both superblocks AND both checkpoint regions.
    let mut image = snapshot(&device);
    let garbage = vec![0xFFu8; LOGICAL_BLOCK_SIZE];
    for lb in 0..64u64 {
        image.write_block(lb, &garbage).expect("wreck");
    }
    image.write_block(BLOCKS - 1, &garbage).expect("wreck mirror");
    let (report, _) = fsck(image, true);
    assert_eq!(report.outcome, FsckOutcome::Unrecoverable);
}
