// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The streaming file-IO surface of the nxfs engine (TASK-0179;
//! split from `fs.rs` under the structure ratchet). Windowed reads and
//! windowed copy-on-write writes: every operation touches only the blocks
//! under its window, so memory is bounded by the CALLER's buffer and never
//! by the file size. New content always lands in FRESH blocks and the
//! journaled extent swap is the commit point, so the crash story is
//! unchanged from the whole-file era it replaces.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0179)
//! PUBLIC API: Nxfs::{read, read_into, write, truncate}
//! TEST_COVERAGE: fs.rs streaming integration tests + stream.rs unit tests
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

use alloc::vec::Vec;

use storage::BlockDevice;

use crate::format::{validate_name, KIND_DIR, KIND_FILE, LOGICAL_BLOCK_SIZE};
use crate::fs::MAX_FILE_BYTES;
use crate::journal::{self, Op};
use crate::{DirEntry, FileKind, Nxfs, NxfsError, ReadDirPage, Result};

impl<D: BlockDevice> Nxfs<D> {
    /// ALLOCATION-FREE windowed read: fills `buf` from `offset` and returns
    /// the byte count (short at EOF). This is the streaming surface every
    /// bump-heap service must use — `read()` is the convenience wrapper for
    /// host tooling and small reads.
    pub fn read_into(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind == KIND_DIR {
            return Err(NxfsError::IsDir);
        }
        let start = offset.min(object.size);
        let end = offset.saturating_add(buf.len() as u64).min(object.size);
        if end <= start {
            return Ok(0);
        }
        let bs = LOGICAL_BLOCK_SIZE as u64;
        let first_block = start / bs;
        let end_block = end.div_ceil(bs);
        let mut block = [0u8; LOGICAL_BLOCK_SIZE];
        for run in
            crate::stream::runs_for_window(&object.extents, first_block, end_block - first_block)
        {
            for i in 0..run.blocks {
                let file_block = run.file_block + i;
                let block_start = file_block * bs;
                let from = start.max(block_start);
                let to = end.min(block_start + bs);
                if from >= to {
                    continue;
                }
                self.dev.read_into(run.lb + i, &mut block)?;
                let dst = (from - start) as usize;
                let src = (from - block_start) as usize;
                let take = (to - from) as usize;
                buf[dst..dst + take].copy_from_slice(&block[src..src + take]);
            }
        }
        Ok((end - start) as usize)
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
        if data.is_empty() {
            return Ok(());
        }
        // Windowed CoW (TASK-0179): fresh blocks for exactly the touched
        // range (plus the zero gap of a sparse extend), head/tail partials
        // copied from the old blocks, extent list spliced, one journaled
        // commit. Memory is bounded by the window, never the file.
        let old_size = object.size;
        let bs = LOGICAL_BLOCK_SIZE as u64;
        // A write past EOF zero-fills the gap; the fresh window starts at
        // the earlier of the write offset and the old tail.
        let window_start_byte = offset.min(old_size);
        let first_block = window_start_byte / bs;
        let touched_end = (offset + data.len() as u64).div_ceil(bs);
        self.splice_window(id, first_block, touched_end, new_size, Some((offset, data)))
    }

    /// Core of the windowed CoW: allocates fresh blocks for file blocks
    /// `[first_block, touched_end)`, fills them from the OLD content where
    /// the window overlaps it (head/tail partial blocks), overlays the
    /// `payload` bytes at their absolute offset (None = zero-fill, the
    /// truncate shape), splices the extent list and commits. New bytes
    /// land ONLY in fresh blocks — the crash story is the journaled
    /// extent swap, unchanged.
    fn splice_window(
        &mut self,
        id: u64,
        first_block: u64,
        touched_end: u64,
        new_size: u64,
        payload: Option<(u64, &[u8])>,
    ) -> Result<()> {
        let bs = LOGICAL_BLOCK_SIZE as u64;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        let old_size = object.size;
        let old_extents = object.extents.clone();

        let fresh_blocks = touched_end - first_block;
        let fresh = self.state.alloc_blocks(fresh_blocks)?;

        // Fill the fresh window run by run, in bounded chunks.
        const CHUNK_BLOCKS: u64 = 16; // 64 KiB working buffer
        let mut buf = alloc::vec![0u8; (CHUNK_BLOCKS * bs) as usize];
        let mut window_block = first_block;
        'fill: for extent in &fresh {
            let mut extent_off = 0u64;
            while extent_off < u64::from(extent.blocks) {
                let chunk_blocks = CHUNK_BLOCKS.min(u64::from(extent.blocks) - extent_off);
                let chunk = &mut buf[..(chunk_blocks * bs) as usize];
                chunk.fill(0);
                // Old content under this chunk (head/tail partial coverage).
                let chunk_start_byte = window_block * bs;
                let old_overlap_end = old_size.min(chunk_start_byte + chunk_blocks * bs);
                if chunk_start_byte < old_overlap_end {
                    let old_first = chunk_start_byte / bs;
                    let old_span = (old_overlap_end - chunk_start_byte).div_ceil(bs);
                    for run in crate::stream::runs_for_window(&old_extents, old_first, old_span) {
                        let dst = ((run.file_block - old_first) * bs) as usize;
                        let run_bytes = (run.blocks as usize) * LOGICAL_BLOCK_SIZE;
                        let take = run_bytes.min((old_overlap_end - run.file_block * bs) as usize);
                        // Allocation-free: straight into the working chunk.
                        self.dev.read_into(run.lb, &mut chunk[dst..dst + take])?;
                    }
                }
                // Payload overlay: plain range intersection with the chunk.
                if let Some((payload_offset, data)) = payload {
                    let chunk_end = chunk_start_byte + chunk_blocks * bs;
                    let from = payload_offset.max(chunk_start_byte);
                    let to = (payload_offset + data.len() as u64).min(chunk_end);
                    if from < to {
                        let dst = (from - chunk_start_byte) as usize;
                        let src = (from - payload_offset) as usize;
                        let take = (to - from) as usize;
                        chunk[dst..dst + take].copy_from_slice(&data[src..src + take]);
                    }
                }
                if self.dev.write_bytes(extent.lb + extent_off, chunk).is_err() {
                    self.state.free_extents(&fresh);
                    return Err(NxfsError::Io);
                }
                extent_off += chunk_blocks;
                window_block += chunk_blocks;
                if window_block >= touched_end {
                    break 'fill;
                }
            }
        }

        let new_extents = crate::stream::splice(&old_extents, first_block, touched_end, &fresh);
        if new_extents.len() > journal::MAX_EXTENTS_PER_WRITE {
            self.state.free_extents(&fresh);
            return Err(NxfsError::NoSpace);
        }
        let ops = alloc::vec![Op::Write { id, size: new_size, extents: new_extents.clone() }];
        self.run_txn(ops, &fresh)
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
        let old_size = object.size;
        if size == old_size {
            return Ok(());
        }
        let bs = LOGICAL_BLOCK_SIZE as u64;
        if size < old_size {
            // Shrink: keep whole blocks, CoW the new partial tail block so
            // its stale suffix reads as zero after a later extend.
            let keep_full = size / bs;
            let touched_end = size.div_ceil(bs);
            if touched_end == keep_full {
                // Block-aligned: pure extent trim, no data moves.
                let old_extents =
                    self.state.objects.get(&id).ok_or(NxfsError::NotFound)?.extents.clone();
                let (prefix, _) = crate::stream::split_at_block(&old_extents, keep_full);
                let ops = alloc::vec![Op::Write { id, size, extents: prefix.clone() }];
                return self.run_txn(ops, &[]);
            }
            self.splice_window(id, keep_full, touched_end, size, None)
        } else {
            // Grow: zero-extend — fresh zero blocks plus a CoW of the old
            // partial tail (its stale suffix becomes readable and must be
            // zero, exactly like the old resize semantics).
            let first_block = old_size / bs;
            let touched_end = size.div_ceil(bs);
            self.splice_window(id, first_block, touched_end, size, None)
        }
    }
}
