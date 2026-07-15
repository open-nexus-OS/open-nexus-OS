// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The nxfs engine — mkfs/mount + the transactional op surface
//! (create/write/read/mkdir/readdir/rename/remove/truncate/stat). Every
//! mutating op is ONE journaled transaction: data blocks are written to
//! freshly allocated space first, then the `BEGIN ops COMMIT` byte run is
//! appended and synced, then the ops apply in memory. A crash before the
//! COMMIT record lands discards the transaction wholesale on replay.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: op-matrix tests below; crash injection in tests/

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use storage::BlockDevice;

use crate::checkpoint;
use crate::dev::Dev;
use crate::format::{
    validate_name, CheckpointSlot, Superblock, Uuid, KIND_DIR, KIND_FILE, LOGICAL_BLOCK_SIZE,
    MAX_DEPTH, ROOT_OBJECT,
};
use crate::journal::{self, Op};
use crate::state::{Extent, State};
use crate::{DirEntry, FileKind, NxfsError, ReadDirPage, Result};

/// Bounded whole-file materialization for the offset-write path (Phase 1;
/// the VMO bulk plane raises this seam in TASK-0295).
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// mkfs parameters. The UUID is injected (no RNG in the engine).
#[derive(Debug, Clone, Copy)]
pub struct MkfsOptions {
    pub uuid: [u8; 16],
    /// Journal region length in logical blocks (bounded replay window).
    pub journal_blocks: u64,
}

impl Default for MkfsOptions {
    fn default() -> Self {
        Self { uuid: [0; 16], journal_blocks: 64 }
    }
}

/// Deterministic checkpoint-region size (both mkfs and mount compute it).
fn checkpoint_blocks(total_blocks: u64) -> u64 {
    (total_blocks / 32).clamp(16, 2048)
}

/// The mounted filesystem.
pub struct Nxfs<D: BlockDevice> {
    dev: Dev<D>,
    sb: Superblock,
    state: State,
    cp_blocks: u64,
    journal_head: usize,
    next_txn: u64,
    /// True when mount discarded a torn/orphaned journal tail (fsck surface).
    pub replay_discarded_tail: bool,
    /// True when this container was just `mkfs`-formatted blank (vs mounted from
    /// existing state) — lets a host seed first-run content exactly once.
    pub formatted_fresh: bool,
}

impl<D: BlockDevice> Nxfs<D> {
    /// Formats a blank container and mounts it. Refuses to format a device
    /// that already carries an nxfs superblock (RFC-0071: no silent reformat).
    pub fn mkfs(device: D, opts: MkfsOptions) -> Result<Self> {
        let dev = Dev::new(device)?;
        let total = dev.logical_blocks();
        let cp_blocks = checkpoint_blocks(total);
        let journal_start = 1u64;
        let min_blocks = 1 + opts.journal_blocks + 2 * cp_blocks + 8;
        if total < min_blocks || opts.journal_blocks == 0 {
            return Err(NxfsError::NoSpace);
        }
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        dev.read(0, &mut block)?;
        if Superblock::decode(&block).is_ok() {
            return Err(NxfsError::Exists);
        }

        let data_start = journal_start + opts.journal_blocks + 2 * cp_blocks;
        let state = State::new_empty(total, data_start);
        let sb = Superblock {
            uuid: Uuid(opts.uuid),
            total_blocks: total,
            journal_start,
            journal_blocks: opts.journal_blocks,
            slots: [CheckpointSlot::default(), CheckpointSlot::default()],
            enc_mode: 0,
        };
        let mut fs = Self {
            dev,
            sb,
            state,
            cp_blocks,
            journal_head: 0,
            next_txn: 1,
            replay_discarded_tail: false,
            formatted_fresh: true,
        };
        fs.write_checkpoint()?;
        debug_assert!(fs.sb.newest_slot().is_some());
        Ok(fs)
    }

    /// Mounts an existing container, or formats a blank device — consuming the
    /// device exactly ONCE (device drivers with fixed MMIO windows cannot be
    /// re-created per attempt). Peeks the superblock to decide, then delegates.
    pub fn open_or_format(device: D, opts: MkfsOptions) -> Result<Self> {
        let dev = Dev::new(device)?;
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        dev.read(0, &mut block)?;
        let device = dev.into_inner();
        if Superblock::decode(&block).is_ok() {
            Self::mount(device)
        } else {
            Self::mkfs(device, opts)
        }
    }

    /// Mounts an existing container: newest valid checkpoint + journal replay.
    pub fn mount(device: D) -> Result<Self> {
        crate::nxfs_trace!("nxfs-trace: mount enter");
        let dev = Dev::new(device)?;
        let total = dev.logical_blocks();
        let sb = Self::read_superblock(&dev, total)?;
        crate::nxfs_trace!("nxfs-trace: superblock read ok");
        let cp_blocks = checkpoint_blocks(total);
        let data_start = sb.journal_start + sb.journal_blocks + 2 * cp_blocks;

        // Newest slot first; fall back to the other on integrity failure
        // (torn checkpoint write — the older slot must still mount).
        let order = match sb.newest_slot() {
            Some(first) => [first, 1 - first],
            None => return Err(NxfsError::Integrity),
        };
        let mut loaded: Option<(State, u64)> = None;
        for idx in order {
            let slot = &sb.slots[idx];
            if slot.generation == 0 {
                continue;
            }
            crate::nxfs_trace!("nxfs-trace: reading checkpoint blob");
            let blob = match dev.read_bytes(slot.root_lb, slot.len_bytes as usize) {
                Ok(blob) => blob,
                Err(_) => continue,
            };
            crate::nxfs_trace!("nxfs-trace: checkpoint blob read ok");
            if crate::format::crc32c(&blob) != slot.crc {
                continue;
            }
            if let Ok(result) = checkpoint::decode(&blob, total, data_start) {
                loaded = Some(result);
                break;
            }
        }
        let (mut state, cp_next_txn) = loaded.ok_or(NxfsError::Integrity)?;
        crate::nxfs_trace!("nxfs-trace: checkpoint decoded, reading journal");

        // Journal replay (committed-only, stale txns skipped by watermark).
        // Read the journal region INCREMENTALLY and stop at the first fully-
        // unused (all-zero) block: the used portion is tiny versus the reserved
        // region, so reading all `journal_blocks` both wastes time and — on the
        // real virtio-blk driver — deadlocks on a long sequential read run
        // (TASK-0293). No committed record can follow an all-zero block (the
        // journal is written contiguously from offset 0, tail is zeros).
        let mut region: Vec<u8> = Vec::with_capacity(2 * LOGICAL_BLOCK_SIZE);
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        for i in 0..sb.journal_blocks {
            dev.read(sb.journal_start + i, &mut block)?;
            let all_zero = block.iter().all(|&byte| byte == 0);
            region.extend_from_slice(&block);
            if all_zero {
                break;
            }
        }
        crate::nxfs_trace!("nxfs-trace: journal read ok, replaying");
        let replayed = journal::replay(&region, cp_next_txn);
        crate::nxfs_trace!("nxfs-trace: replay done");
        for op in &replayed.ops {
            state.apply(op)?;
        }
        Ok(Self {
            dev,
            sb,
            state,
            cp_blocks,
            journal_head: replayed.write_head,
            next_txn: replayed.next_txn,
            replay_discarded_tail: replayed.orphan,
            formatted_fresh: false,
        })
    }

    fn read_superblock(dev: &Dev<D>, total: u64) -> Result<Superblock> {
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        dev.read(0, &mut block)?;
        if let Ok(sb) = Superblock::decode(&block) {
            return Ok(sb);
        }
        // Primary torn: the mirror in the last logical block still mounts.
        dev.read(total - 1, &mut block)?;
        Superblock::decode(&block)
    }

    /// Consumes the filesystem, returning the underlying device (tests).
    pub fn into_device(self) -> D {
        self.dev.into_inner()
    }

    // ---- read surface -----------------------------------------------------

    /// Metadata for `path`: `(kind, size)`.
    pub fn stat(&self, path: &str) -> Result<(FileKind, u64)> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        let kind = if object.kind == KIND_DIR { FileKind::Dir } else { FileKind::File };
        Ok((kind, object.size))
    }

    /// Reads up to `len` bytes at `offset`.
    pub fn read(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind == KIND_DIR {
            return Err(NxfsError::IsDir);
        }
        let content = self.materialize(&object.extents, object.size)?;
        let start = (offset.min(object.size)) as usize;
        let end = start.saturating_add(len).min(content.len());
        Ok(content[start..end].to_vec())
    }

    /// One bounded readdir page in canonical (byte) order.
    pub fn read_dir(&self, path: &str, cursor: u32, limit: u16) -> Result<ReadDirPage> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind != KIND_DIR {
            return Err(NxfsError::NotDir);
        }
        let table = self.state.dirs.get(&id).ok_or(NxfsError::Integrity)?;
        let entries: Vec<DirEntry> = table
            .iter()
            .map(|(name, (child, kind))| DirEntry {
                name: name.clone(),
                kind: if *kind == KIND_DIR { FileKind::Dir } else { FileKind::File },
                size: self.state.objects.get(child).map_or(0, |o| o.size),
            })
            .collect();
        let limit = limit.clamp(1, nexus_vfs_types::MAX_ENTRIES_PER_PAGE) as usize;
        let start = (cursor as usize).min(entries.len());
        let take = (entries.len() - start).min(limit);
        let eof = start + take >= entries.len();
        Ok(ReadDirPage {
            entries: entries[start..start + take].to_vec(),
            next_cursor: (start + take) as u32,
            eof,
        })
    }

    // ---- write surface (one txn per op) ------------------------------------

    /// Creates an empty file (exclusive).
    pub fn create(&mut self, path: &str) -> Result<()> {
        self.mknode(path, KIND_FILE)
    }

    /// Creates a directory (exclusive).
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        self.mknode(path, KIND_DIR)
    }

    fn mknode(&mut self, path: &str, kind: u8) -> Result<()> {
        let (parent, name) = self.resolve_parent(path)?;
        validate_name(&name)?;
        let table = self.state.dirs.get(&parent).ok_or(NxfsError::NotDir)?;
        if table.contains_key(&name) {
            return Err(NxfsError::Exists);
        }
        let id = self.state.next_object;
        let ops = alloc::vec![Op::MkNode { parent, id, kind, name }];
        self.run_txn(ops, &[])
    }

    /// Writes `data` at `offset`, extending the file as needed (bounded by
    /// [`MAX_FILE_BYTES`]). Whole-content copy-on-write: fresh extents carry
    /// the new content; old blocks free on commit.
    pub fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<()> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind == KIND_DIR {
            return Err(NxfsError::IsDir);
        }
        let new_size = core::cmp::max(object.size, offset.saturating_add(data.len() as u64));
        if new_size > MAX_FILE_BYTES {
            return Err(NxfsError::TooBig);
        }
        let mut content = self.materialize(&object.extents, object.size)?;
        content.resize(new_size as usize, 0);
        content[offset as usize..offset as usize + data.len()].copy_from_slice(data);
        self.rewrite(id, &content)
    }

    /// Truncates (or zero-extends) the file to `size`.
    pub fn truncate(&mut self, path: &str, size: u64) -> Result<()> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind == KIND_DIR {
            return Err(NxfsError::IsDir);
        }
        if size > MAX_FILE_BYTES {
            return Err(NxfsError::TooBig);
        }
        let mut content = self.materialize(&object.extents, object.size)?;
        content.resize(size as usize, 0);
        self.rewrite(id, &content)
    }

    fn rewrite(&mut self, id: u64, content: &[u8]) -> Result<()> {
        let blocks = (content.len().div_ceil(LOGICAL_BLOCK_SIZE)) as u64;
        let extents = self.state.alloc_blocks(blocks)?;
        if extents.len() > journal::MAX_EXTENTS_PER_WRITE {
            self.state.free_extents(&extents);
            return Err(NxfsError::NoSpace);
        }
        // Data first (unreferenced until commit), then the journaled commit.
        let mut written = 0usize;
        for extent in &extents {
            let extent_bytes = (extent.blocks as usize) * LOGICAL_BLOCK_SIZE;
            let end = (written + extent_bytes).min(content.len());
            if self.dev.write_bytes(extent.lb, &content[written..end]).is_err() {
                self.state.free_extents(&extents);
                return Err(NxfsError::Io);
            }
            written = end;
        }
        let ops =
            alloc::vec![Op::Write { id, size: content.len() as u64, extents: extents.clone() }];
        self.run_txn(ops, &extents)
    }

    /// Removes a file or an EMPTY directory (`Busy` otherwise).
    pub fn remove(&mut self, path: &str) -> Result<()> {
        let (parent, name) = self.resolve_parent(path)?;
        let table = self.state.dirs.get(&parent).ok_or(NxfsError::NotDir)?;
        let (id, kind) = *table.get(&name).ok_or(NxfsError::NotFound)?;
        if kind == KIND_DIR
            && self.state.dirs.get(&id).is_some_and(|t| !t.is_empty())
        {
            return Err(NxfsError::Busy);
        }
        let ops = alloc::vec![Op::Remove { parent, id, name }];
        self.run_txn(ops, &[])
    }

    /// Atomically renames `from` to `to` (same container; an existing target
    /// file or empty dir is replaced — exactly one name is visible after any
    /// crash).
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let (from_parent, from_name) = self.resolve_parent(from)?;
        let (to_parent, to_name) = self.resolve_parent(to)?;
        validate_name(&to_name)?;
        let from_table = self.state.dirs.get(&from_parent).ok_or(NxfsError::NotDir)?;
        let (moving_id, _) = *from_table.get(&from_name).ok_or(NxfsError::NotFound)?;
        let to_table = self.state.dirs.get(&to_parent).ok_or(NxfsError::NotDir)?;
        let replaced = match to_table.get(&to_name) {
            None => 0,
            Some((existing, kind)) => {
                if *existing == moving_id {
                    return Ok(()); // rename onto itself
                }
                if *kind == KIND_DIR
                    && self.state.dirs.get(existing).is_some_and(|t| !t.is_empty())
                {
                    return Err(NxfsError::Busy);
                }
                *existing
            }
        };
        let ops = alloc::vec![Op::Rename {
            from_parent,
            from_name,
            to_parent,
            to_name,
            replaced,
        }];
        self.run_txn(ops, &[])
    }

    /// Flushes the device.
    pub fn sync(&mut self) -> Result<()> {
        self.dev.sync()
    }

    // ---- transaction machinery ---------------------------------------------

    fn run_txn(&mut self, ops: Vec<Op>, rollback_extents: &[Extent]) -> Result<()> {
        let bytes = match journal::encode_txn(self.next_txn, &ops) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.state.free_extents(rollback_extents);
                return Err(err);
            }
        };
        let capacity = (self.sb.journal_blocks as usize) * LOGICAL_BLOCK_SIZE;
        if self.journal_head + bytes.len() > capacity {
            // Journal full: checkpoint compacts it (head resets to 0).
            if let Err(err) = self.write_checkpoint() {
                self.state.free_extents(rollback_extents);
                return Err(err);
            }
            if bytes.len() > capacity {
                self.state.free_extents(rollback_extents);
                return Err(NxfsError::NoSpace);
            }
        }
        if let Err(err) = self.append_journal(&bytes).and_then(|()| self.dev.sync()) {
            self.state.free_extents(rollback_extents);
            return Err(err);
        }
        self.journal_head += bytes.len();
        self.next_txn += 1;
        for op in &ops {
            self.state.apply(op)?;
        }
        Ok(())
    }

    /// Appends a byte run at the journal head (read-modify-write on the
    /// boundary block; the run itself is whole-block-aligned afterwards).
    fn append_journal(&mut self, bytes: &[u8]) -> Result<()> {
        let base = self.sb.journal_start;
        let mut head = self.journal_head;
        let mut remaining = bytes;
        while !remaining.is_empty() {
            let lb = base + (head / LOGICAL_BLOCK_SIZE) as u64;
            let in_block = head % LOGICAL_BLOCK_SIZE;
            let space = LOGICAL_BLOCK_SIZE - in_block;
            let take = remaining.len().min(space);
            let mut block = [0u8; LOGICAL_BLOCK_SIZE];
            if in_block != 0 {
                self.dev.read(lb, &mut block)?;
            }
            block[in_block..in_block + take].copy_from_slice(&remaining[..take]);
            self.dev.write(lb, &block)?;
            head += take;
            remaining = &remaining[take..];
        }
        Ok(())
    }

    /// Writes a checkpoint into the OLDER fixed region, flips the superblock
    /// slot (primary + mirror), then resets the journal head. Crash-safe in
    /// every prefix: the newest valid slot + txn watermark always reconstruct
    /// a committed state.
    pub fn write_checkpoint(&mut self) -> Result<()> {
        let blob = checkpoint::encode(&self.state, self.next_txn);
        let region_bytes = (self.cp_blocks as usize) * LOGICAL_BLOCK_SIZE;
        if blob.len() > region_bytes {
            return Err(NxfsError::NoSpace);
        }
        let slot_idx = self.sb.older_slot();
        let region_start = self.sb.journal_start
            + self.sb.journal_blocks
            + (slot_idx as u64) * self.cp_blocks;
        self.dev.write_bytes(region_start, &blob)?;
        self.dev.sync()?;

        let generation =
            self.sb.slots[0].generation.max(self.sb.slots[1].generation) + 1;
        self.sb.slots[slot_idx] = CheckpointSlot {
            generation,
            root_lb: region_start,
            len_bytes: blob.len() as u64,
            crc: crate::format::crc32c(&blob),
        };
        let encoded = self.sb.encode();
        self.dev.write(0, &encoded)?;
        self.dev.write(self.sb.total_blocks - 1, &encoded)?;
        self.dev.sync()?;
        self.journal_head = 0;
        Ok(())
    }

    // ---- path helpers -------------------------------------------------------

    fn split(path: &str) -> Result<Vec<&str>> {
        let mut parts = Vec::new();
        for segment in path.split('/') {
            if segment.is_empty() {
                continue;
            }
            if segment == "." || segment == ".." {
                return Err(NxfsError::Invalid);
            }
            parts.push(segment);
        }
        if parts.len() > MAX_DEPTH {
            return Err(NxfsError::TooBig);
        }
        Ok(parts)
    }

    fn resolve(&self, path: &str) -> Result<u64> {
        let mut current = ROOT_OBJECT;
        for segment in Self::split(path)? {
            let table = self.state.dirs.get(&current).ok_or(NxfsError::NotDir)?;
            let (child, _) = *table.get(segment).ok_or(NxfsError::NotFound)?;
            current = child;
        }
        Ok(current)
    }

    fn resolve_parent(&self, path: &str) -> Result<(u64, String)> {
        let parts = Self::split(path)?;
        let (name, dirs) = parts.split_last().ok_or(NxfsError::Invalid)?;
        let mut current = ROOT_OBJECT;
        for segment in dirs {
            let table = self.state.dirs.get(&current).ok_or(NxfsError::NotDir)?;
            let (child, kind) = *table.get(*segment).ok_or(NxfsError::NotFound)?;
            if kind != KIND_DIR {
                return Err(NxfsError::NotDir);
            }
            current = child;
        }
        Ok((current, name.to_string()))
    }

    fn materialize(&self, extents: &[Extent], size: u64) -> Result<Vec<u8>> {
        if size > MAX_FILE_BYTES {
            return Err(NxfsError::TooBig);
        }
        let mut out = Vec::with_capacity(size as usize);
        for extent in extents {
            let extent_bytes = (extent.blocks as usize) * LOGICAL_BLOCK_SIZE;
            let remaining = (size as usize).saturating_sub(out.len());
            if remaining == 0 {
                break;
            }
            let take = remaining.min(extent_bytes);
            out.extend_from_slice(&self.dev.read_bytes(extent.lb, take)?);
        }
        if out.len() != size as usize {
            return Err(NxfsError::Integrity);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::MemBlockDevice;

    fn fresh() -> Nxfs<MemBlockDevice> {
        let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 4096);
        Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs")
    }

    #[test]
    fn op_matrix_roundtrip() {
        let mut fs = fresh();
        fs.mkdir("/docs").expect("mkdir");
        fs.create("/docs/a.txt").expect("create");
        fs.write("/docs/a.txt", 0, b"hello nxfs").expect("write");
        assert_eq!(fs.read("/docs/a.txt", 0, 64).expect("read"), b"hello nxfs");
        // Offset write with extension
        fs.write("/docs/a.txt", 6, b"world!").expect("write offset");
        assert_eq!(fs.read("/docs/a.txt", 0, 64).expect("read"), b"hello world!");
        assert_eq!(fs.stat("/docs/a.txt").expect("stat"), (FileKind::File, 12));
        fs.truncate("/docs/a.txt", 5).expect("truncate");
        assert_eq!(fs.read("/docs/a.txt", 0, 64).expect("read"), b"hello");
        fs.rename("/docs/a.txt", "/docs/b.txt").expect("rename");
        let page = fs.read_dir("/docs", 0, 64).expect("readdir");
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].name, "b.txt");
        fs.remove("/docs/b.txt").expect("remove file");
        fs.remove("/docs").expect("remove empty dir");
        assert!(fs.read_dir("/", 0, 64).expect("root").entries.is_empty());
    }

    #[test]
    fn mount_replays_committed_state() {
        let mut fs = fresh();
        fs.mkdir("/d").expect("mkdir");
        fs.create("/d/f").expect("create");
        fs.write("/d/f", 0, b"persist me").expect("write");
        let device = fs.into_device();
        let fs = Nxfs::mount(device).expect("mount");
        assert_eq!(fs.read("/d/f", 0, 64).expect("read"), b"persist me");
        assert!(!fs.replay_discarded_tail);
    }

    #[test]
    fn checkpoint_then_more_txns_survive_remount() {
        let mut fs = fresh();
        fs.mkdir("/a").expect("mkdir");
        fs.write_checkpoint().expect("checkpoint");
        fs.create("/a/post-cp.txt").expect("create");
        fs.write("/a/post-cp.txt", 0, b"x").expect("write");
        let fs = Nxfs::mount(fs.into_device()).expect("mount");
        assert_eq!(fs.stat("/a/post-cp.txt").expect("stat").0, FileKind::File);
    }

    #[test]
    fn journal_full_triggers_checkpoint_and_keeps_going() {
        let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 4096);
        let mut fs =
            Nxfs::mkfs(device, MkfsOptions { uuid: [1; 16], journal_blocks: 1 }).expect("mkfs");
        for i in 0..200 {
            fs.create(&alloc::format!("/f{i}")).expect("create");
        }
        let fs = Nxfs::mount(fs.into_device()).expect("mount");
        assert_eq!(fs.read_dir("/", 0, 64).expect("page").entries.len(), 64);
        let page = fs.read_dir("/", 128, 64).expect("page");
        assert_eq!(page.entries.len(), 64);
    }

    #[test]
    fn test_reject_error_paths() {
        let mut fs = fresh();
        fs.mkdir("/d").expect("mkdir");
        fs.create("/d/f").expect("create");
        assert_eq!(fs.create("/d/f"), Err(NxfsError::Exists));
        assert_eq!(fs.read("/nope", 0, 1), Err(NxfsError::NotFound));
        assert_eq!(fs.read("/d", 0, 1), Err(NxfsError::IsDir));
        assert_eq!(fs.read_dir("/d/f", 0, 64), Err(NxfsError::NotDir));
        assert_eq!(fs.remove("/d"), Err(NxfsError::Busy));
        assert_eq!(fs.create("/d/f/x"), Err(NxfsError::NotDir));
        assert_eq!(fs.create("/d/../x"), Err(NxfsError::Invalid));
        assert_eq!(fs.write("/d/f", MAX_FILE_BYTES, b"x"), Err(NxfsError::TooBig));
        // No silent reformat of an existing container.
        let device = fs.into_device();
        assert_eq!(
            Nxfs::mkfs(device, MkfsOptions::default()).map(|_| ()),
            Err(NxfsError::Exists)
        );
    }

    #[test]
    fn no_space_is_clean() {
        let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 256);
        let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
        fs.create("/big").expect("create");
        let huge = alloc::vec![0xABu8; 3 * 1024 * 1024];
        assert_eq!(fs.write("/big", 0, &huge), Err(NxfsError::NoSpace));
        // The failed txn must not leak blocks: a small write still works.
        fs.write("/big", 0, b"small").expect("small write");
        assert_eq!(fs.read("/big", 0, 16).expect("read"), b"small");
    }
}
