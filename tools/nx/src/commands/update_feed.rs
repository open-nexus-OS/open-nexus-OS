// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx update check/stage` feed half (TASK-0140, RFC-0089 §9) —
//! split from `commands/update.rs` under the structure ratchet. Owns the
//! data-volume view (partition-scoped BlockDevice, nxfs mount/mkfs
//! discipline) and the feed verbs: `check` enumerates `/updates/*.nxs`
//! and verifies each through the REAL device engine, `stage` is the
//! provisioning drop (verify against the baked anchor + floor, THEN
//! write). Status/switch/rollback stay in `update.rs`.
//! OWNERS: @tools-team @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/update_cli.rs
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::cli::{UpdateCheckArgs, UpdateStageArgs};
use crate::commands::image::{part, FileBlockDevice, SECTOR};
use crate::commands::update::{disk_parts, open_disk, read_bsb};
use crate::error::{ExecResult, ExitClass, NxError};

use storage::gpt::{Partition, GUID_NEXUS_DATA};
use storage::{BlockDevice, BlockError};

/// Feed directory on the data volume (RFC-0089 §9 namespace note: the
/// data partition mounts at the VFS root, so the wire path is
/// `/updates/…`, never `/data/updates/…`).
const FEED_DIR: &str = "/updates";
/// Bounded feed walk (device OP_FEED_LIST caps at 8; the host tool may
/// list more, but never unbounded).
const MAX_FEED_ENTRIES: usize = 256;

// --------------------------------------------------------------- devices --

/// Read-side view of one GPT partition as its own BlockDevice (nxfs
/// mounts the data partition without knowing about the disk around it).
struct PartView {
    disk: FileBlockDevice,
    first_lba: u64,
    sectors: u64,
}

impl PartView {
    fn new(disk: FileBlockDevice, p: &Partition) -> Self {
        Self { disk, first_lba: p.first_lba, sectors: p.last_lba - p.first_lba + 1 }
    }

    fn bound(&self, first: u64, len: usize) -> Result<u64, BlockError> {
        let span = (len as u64).div_ceil(SECTOR as u64);
        if first >= self.sectors || span > self.sectors - first {
            return Err(BlockError::OutOfRange);
        }
        Ok(self.first_lba + first)
    }
}

impl BlockDevice for PartView {
    fn block_size(&self) -> usize {
        SECTOR
    }
    fn block_count(&self) -> u64 {
        self.sectors
    }
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let base = self.bound(block_idx, SECTOR)?;
        self.disk.read_blocks(base, &mut buf[..SECTOR])
    }
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        let base = self.bound(block_idx, SECTOR)?;
        self.disk.write_blocks(base, &buf[..SECTOR])
    }
    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let base = self.bound(first_block, buf.len())?;
        self.disk.read_blocks(base, buf)
    }
    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        let base = self.bound(first_block, buf.len())?;
        self.disk.write_blocks(base, buf)
    }
    fn sync(&mut self) -> Result<(), BlockError> {
        self.disk.sync()
    }
}

/// The feed volume for check/stage: a standalone nxfs image (`--data`) or
/// the `data` partition of a built disk. Returns the device + the
/// anti-downgrade floor read from the disk BSB (standalone images carry
/// no BSB, so the floor gate stays at 0 there — stated in the output).
enum FeedTarget {
    Standalone(FileBlockDevice),
    Disk(PartView),
}

impl FeedTarget {
    fn open(image: &Path, data: &Option<PathBuf>) -> Result<(Self, u32, String), NxError> {
        if let Some(data_path) = data {
            let dev = open_disk(data_path)?;
            return Ok((Self::Standalone(dev), 0, data_path.display().to_string()));
        }
        let disk = open_disk(image)?;
        let parts = disk_parts(&disk)?;
        let floor = read_bsb(&disk, &parts)?.0.rollback_min_index;
        let data_part = part(&parts, &GUID_NEXUS_DATA, "data")?;
        Ok((Self::Disk(PartView::new(disk, &data_part)), floor, image.display().to_string()))
    }
}

// ------------------------------------------------------------------ feed --

struct NullSink;
impl updates::component_set::ComponentSink for NullSink {
    fn begin(
        &mut self,
        _meta: &updates::component_set::ComponentMeta,
    ) -> Result<(), updates::component_set::RejectReason> {
        Ok(())
    }
    fn chunk(
        &mut self,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), updates::component_set::RejectReason> {
        Ok(())
    }
    fn finish(
        &mut self,
        _meta: &updates::component_set::ComponentMeta,
    ) -> Result<(), updates::component_set::RejectReason> {
        Ok(())
    }
}

/// The REAL device verify path against the REAL baked anchor — the same
/// call `updated` makes on the device (discarding sink: verify, no apply).
fn engine_verify(
    container: &[u8],
    floor: u32,
) -> Result<updates::component_set::ManifestV2, updates::component_set::RejectReason> {
    updates::component_set::verify_and_apply(
        container,
        &updates::system_set::Ed25519Verifier,
        updates::trust::BAKED_PUBLISHERS,
        floor,
        &mut NullSink,
        &mut || {},
    )
}

fn mount_feed<D: BlockDevice>(dev: D) -> Result<nxfs::Nxfs<D>, NxError> {
    nxfs::Nxfs::mount(dev).map_err(|err| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("update: data volume is not a mountable nxfs ({err:?})"),
        )
    })
}

/// Blank = every byte of the first 8 sectors is zero (a freshly built
/// disk without `--data`). Anything else must MOUNT — never reformat a
/// volume that merely fails to parse.
fn is_blank<D: BlockDevice>(dev: &D) -> Result<bool, NxError> {
    let mut head = [0u8; 8 * SECTOR];
    dev.read_blocks(0, &mut head)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("update: read ({e:?})")))?;
    Ok(head.iter().all(|&b| b == 0))
}

/// Stage-side volume access: mkfs a BLANK data volume (provisioning a
/// fresh disk is the tool's job), mount anything else.
fn mount_or_init_feed<D: BlockDevice>(dev: D) -> Result<nxfs::Nxfs<D>, NxError> {
    if is_blank(&dev)? {
        return nxfs::Nxfs::mkfs(
            dev,
            nxfs::MkfsOptions { uuid: *b"nexus-data-vol01", journal_blocks: 64 },
        )
        .map_err(|err| NxError::new(ExitClass::Internal, format!("update: mkfs ({err:?})")));
    }
    mount_feed(dev)
}

fn feed_names<D: BlockDevice>(fs: &nxfs::Nxfs<D>) -> Result<Vec<String>, NxError> {
    let mut names = Vec::new();
    let mut cursor = 0u32;
    loop {
        let page = fs.read_dir(FEED_DIR, cursor, 64).map_err(|err| {
            NxError::new(ExitClass::ValidationReject, format!("update: readdir ({err:?})"))
        })?;
        for entry in &page.entries {
            if entry.name.ends_with(".nxs") {
                names.push(entry.name.clone());
            }
        }
        if page.eof || names.len() >= MAX_FEED_ENTRIES {
            break;
        }
        cursor = page.next_cursor;
    }
    Ok(names)
}

pub(crate) fn handle_check(args: UpdateCheckArgs) -> ExecResult {
    let (target, floor, source) = FeedTarget::open(&args.image, &args.data)?;
    let candidates = match target {
        FeedTarget::Standalone(dev) => check_or_absent(dev, floor)?,
        FeedTarget::Disk(dev) => check_or_absent(dev, floor)?,
    };
    let available = candidates.iter().filter(|c| c.verdict == "ok").count();
    let mut lines = vec![format!(
        "update: check (feed={source} candidates={} available={available})",
        candidates.len()
    )];
    for c in &candidates {
        lines.push(match &c.manifest {
            Some((build, rbidx)) => format!(
                "update: candidate {} build={build} rbidx={rbidx} verdict={}",
                c.name, c.verdict
            ),
            None => format!("update: candidate {} verdict={}", c.name, c.verdict),
        });
    }
    let data = json!({
        "feed": source,
        "floor": floor,
        "available": available,
        "candidates": candidates.iter().map(|c| json!({
            "name": c.name,
            "bytes": c.bytes,
            "build_id": c.manifest.as_ref().map(|(b, _)| b.clone()),
            "rollback_index": c.manifest.as_ref().map(|(_, r)| *r),
            "verdict": c.verdict,
        })).collect::<Vec<_>>(),
    });
    Ok((ExitClass::Success, lines.join("\n"), args.json, Some(data)))
}

struct Candidate {
    name: String,
    bytes: u64,
    manifest: Option<(String, u32)>,
    verdict: String,
}

/// A blank data volume is an honest empty feed, not an error.
fn check_or_absent<D: BlockDevice>(dev: D, floor: u32) -> Result<Vec<Candidate>, NxError> {
    if is_blank(&dev)? {
        return Ok(Vec::new());
    }
    check_feed(mount_feed(dev)?, floor)
}

fn check_feed<D: BlockDevice>(fs: nxfs::Nxfs<D>, floor: u32) -> Result<Vec<Candidate>, NxError> {
    let mut out = Vec::new();
    for name in feed_names(&fs)? {
        let path = format!("{FEED_DIR}/{name}");
        let (bytes, container) = match fs.stat(&path) {
            Ok((_, size)) if size as usize <= updates::component_set::MAX_NXS_ARCHIVE_BYTES => {
                match fs.read(&path, 0, size as usize) {
                    Ok(data) => (size, Some(data)),
                    Err(_) => (size, None),
                }
            }
            Ok((_, size)) => (size, None),
            Err(_) => (0, None),
        };
        let (manifest, verdict) = match container {
            Some(data) => match engine_verify(&data, floor) {
                Ok(m) => (Some((m.build_id, m.rollback_index)), "ok".to_string()),
                Err(reason) => (None, reason.label().to_string()),
            },
            None => (None, "io".to_string()),
        };
        out.push(Candidate { name, bytes, manifest, verdict });
    }
    Ok(out)
}

// ----------------------------------------------------------------- stage --

pub(crate) fn handle_stage(args: UpdateStageArgs) -> ExecResult {
    let name = args
        .container
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| {
            n.len() <= 64
                && n.ends_with(".nxs")
                && n.bytes().all(|b| b.is_ascii_graphic() && b != b'/')
        })
        .ok_or_else(|| {
            NxError::new(
                ExitClass::Usage,
                "update: container must be named `<ascii ≤64>.nxs` (device path rules)",
            )
        })?
        .to_string();
    let container = std::fs::read(&args.container).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("update: read {}: {err}", args.container.display()),
        )
    })?;
    let (target, floor, source) = FeedTarget::open(&args.image, &args.data)?;
    // Verify BEFORE any write — the drop never lands bytes the device
    // engine would reject (same anchor, same floor gate).
    let manifest = engine_verify(&container, floor).map_err(|reason| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("update: stage rejected ({})", reason.label()),
        )
    })?;
    let path = format!("{FEED_DIR}/{name}");
    match target {
        FeedTarget::Standalone(dev) => write_feed(mount_or_init_feed(dev)?, &path, &container)?,
        FeedTarget::Disk(dev) => write_feed(mount_or_init_feed(dev)?, &path, &container)?,
    }
    let message = format!(
        "update: staged {path} (build={} rbidx={}) into {source}",
        manifest.build_id, manifest.rollback_index
    );
    let data = json!({
        "path": path,
        "target": source,
        "bytes": container.len(),
        "build_id": manifest.build_id,
        "rollback_index": manifest.rollback_index,
        "floor": floor,
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

fn write_feed<D: BlockDevice>(
    mut fs: nxfs::Nxfs<D>,
    path: &str,
    bytes: &[u8],
) -> Result<(), NxError> {
    fn io(what: &'static str) -> impl Fn(nxfs::NxfsError) -> NxError {
        move |err| NxError::new(ExitClass::Internal, format!("update: {what} ({err:?})"))
    }
    if fs.stat(FEED_DIR).is_err() {
        fs.mkdir(FEED_DIR).map_err(io("mkdir /updates"))?;
    }
    // Remove-then-create: a shorter re-stage must not leave stale tail
    // bytes behind the new container.
    if fs.stat(path).is_ok() {
        fs.remove(path).map_err(io("remove old"))?;
    }
    fs.create(path).map_err(io("create"))?;
    fs.write(path, 0, bytes).map_err(io("write"))?;
    fs.sync().map_err(io("sync"))?;
    fs.write_checkpoint().map_err(io("checkpoint"))?;
    Ok(())
}
