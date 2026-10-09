// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The streaming file-IO surface of the nxfs engine (TASK-0179;
//! split from `fs.rs` under the structure ratchet). Windowed reads and
//! windowed copy-on-write writes: every operation touches only the blocks
//! under its window, so memory is bounded by the CALLER's buffer and never
//! by the file size. New content always lands in FRESH blocks and the
//! journaled extent swap is the commit point, so the crash story is
//! unchanged from the whole-file era it replaces. The fresh blocks are
//! assembled in ONE working buffer per mount (TASK-0068): a buffer per
//! write leaked its full 64 KiB on every write on the never-freeing
//! service bump heaps.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0179)
//! PUBLIC API: Nxfs::{read, read_into, write, write_from, truncate}
//! TEST_COVERAGE: streaming + working-buffer tests below; tests/write_from.rs
//!   (pulled-write contract); stream.rs unit tests
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

use alloc::vec::Vec;

use storage::BlockDevice;

use crate::format::{validate_name, KIND_DIR, KIND_FILE, LOGICAL_BLOCK_SIZE};
use crate::fs::MAX_FILE_BYTES;
use crate::journal::{self, Op};
use crate::state::Extent;
use crate::{DirEntry, FileKind, Nxfs, NxfsError, ReadDirPage, Result};

/// Logical blocks per fill chunk of the windowed CoW: the size of the
/// engine's working buffer (`Nxfs::work_buf`, 64 KiB).
const WORK_BLOCKS: u64 = 16;
const WORK_BYTES: usize = WORK_BLOCKS as usize * LOGICAL_BLOCK_SIZE;

/// What a windowed CoW write lays over its fresh blocks: `len` bytes at file
/// offset `offset`, pulled from `source` one chunk's piece at a time.
struct Payload<'a> {
    offset: u64,
    len: u64,
    source: &'a mut dyn FnMut(u64, &mut [u8]) -> Result<()>,
}

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
    /// the new content; old blocks free on commit. [`Self::write_from`] over
    /// a slice — one write path.
    pub fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<()> {
        self.write_from(path, offset, data.len() as u64, &mut |at, out| {
            // The engine only asks for pieces inside `[0, data.len())`.
            let at = at as usize;
            out.copy_from_slice(data.get(at..at + out.len()).ok_or(NxfsError::Invalid)?);
            Ok(())
        })
    }

    /// Writes `len` bytes at `offset` like [`Self::write`], for content the
    /// caller does not hold in one slice: `source(at, out)` fills `out` with
    /// payload bytes `[at, at + out.len())`. It is asked for every byte
    /// exactly once, in order, each piece straight into the engine's working
    /// buffer — so the write is still ONE journaled transaction and memory
    /// stays bounded whatever `len` is (vfsd pulls a client's VMO this way,
    /// TASK-0068). Bounds are checked before the source is asked for a byte.
    /// A source error aborts the write before anything commits: the file is
    /// unchanged, every fresh block returns to the pool, and the error comes
    /// back as it was.
    pub fn write_from(
        &mut self,
        path: &str,
        offset: u64,
        len: u64,
        source: &mut dyn FnMut(u64, &mut [u8]) -> Result<()>,
    ) -> Result<()> {
        let id = self.resolve(path)?;
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        if object.kind == KIND_DIR {
            return Err(NxfsError::IsDir);
        }
        let new_size = core::cmp::max(object.size, offset.saturating_add(len));
        if new_size > MAX_FILE_BYTES {
            return Err(NxfsError::TooBig);
        }
        if len == 0 {
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
        let touched_end = (offset + len).div_ceil(bs);
        let payload = Payload { offset, len, source };
        self.splice_window(id, first_block, touched_end, new_size, Some(payload))
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
        payload: Option<Payload<'_>>,
    ) -> Result<()> {
        let object = self.state.objects.get(&id).ok_or(NxfsError::NotFound)?;
        let old_size = object.size;
        let old_extents = object.extents.clone();

        let fresh_blocks = touched_end - first_block;
        let fresh = self.state.alloc_blocks(fresh_blocks)?;

        // Fill the fresh window through the engine's ONE working buffer. It
        // leaves `self` for the fill (which borrows `self.dev`) and comes
        // back on every path — a lost buffer would be re-allocated, and
        // leaked, by the next write.
        let mut buf = core::mem::take(&mut self.work_buf);
        if buf.len() < WORK_BYTES {
            buf = alloc::vec![0u8; WORK_BYTES]; // the first write of this mount
        }
        let filled = self.fill_fresh(
            &mut buf,
            &fresh,
            first_block..touched_end,
            old_size,
            &old_extents,
            payload,
        );
        self.work_buf = buf;
        if let Err(err) = filled {
            self.state.free_extents(&fresh);
            return Err(err);
        }

        let new_extents = crate::stream::splice(&old_extents, first_block, touched_end, &fresh);
        if new_extents.len() > journal::MAX_EXTENTS_PER_WRITE {
            self.state.free_extents(&fresh);
            return Err(NxfsError::NoSpace);
        }
        let ops = alloc::vec![Op::Write { id, size: new_size, extents: new_extents }];
        self.run_txn(ops, &fresh)
    }

    /// Writes the `fresh` extents backing file blocks `blocks`, run by run in
    /// bounded chunks of `buf`: each chunk is zeroed, gets the OLD content
    /// where the window overlaps it (head/tail partial blocks), then the
    /// `payload` bytes at their absolute offset, pulled straight into the
    /// chunk. The caller owns rollback.
    fn fill_fresh(
        &mut self,
        buf: &mut [u8],
        fresh: &[Extent],
        blocks: core::ops::Range<u64>,
        old_size: u64,
        old_extents: &[Extent],
        mut payload: Option<Payload<'_>>,
    ) -> Result<()> {
        let bs = LOGICAL_BLOCK_SIZE as u64;
        let mut window_block = blocks.start;
        for extent in fresh {
            let mut extent_off = 0u64;
            while extent_off < u64::from(extent.blocks) {
                let chunk_blocks = WORK_BLOCKS.min(u64::from(extent.blocks) - extent_off);
                let chunk = &mut buf[..(chunk_blocks * bs) as usize];
                chunk.fill(0);
                // Old content under this chunk (head/tail partial coverage).
                let chunk_start_byte = window_block * bs;
                let old_overlap_end = old_size.min(chunk_start_byte + chunk_blocks * bs);
                if chunk_start_byte < old_overlap_end {
                    let old_first = chunk_start_byte / bs;
                    let old_span = (old_overlap_end - chunk_start_byte).div_ceil(bs);
                    for run in crate::stream::runs_for_window(old_extents, old_first, old_span) {
                        let dst = ((run.file_block - old_first) * bs) as usize;
                        let run_bytes = (run.blocks as usize) * LOGICAL_BLOCK_SIZE;
                        let take = run_bytes.min((old_overlap_end - run.file_block * bs) as usize);
                        // Allocation-free: straight into the working chunk.
                        self.dev.read_into(run.lb, &mut chunk[dst..dst + take])?;
                    }
                }
                // Payload overlay: plain range intersection with the chunk,
                // pulled from the source straight into the working chunk.
                if let Some(payload) = payload.as_mut() {
                    let chunk_end = chunk_start_byte + chunk_blocks * bs;
                    let from = payload.offset.max(chunk_start_byte);
                    let to = (payload.offset + payload.len).min(chunk_end);
                    if from < to {
                        let dst = (from - chunk_start_byte) as usize;
                        let take = (to - from) as usize;
                        (payload.source)(from - payload.offset, &mut chunk[dst..dst + take])?;
                    }
                }
                self.dev.write_bytes(extent.lb + extent_off, chunk).map_err(|_| NxfsError::Io)?;
                extent_off += chunk_blocks;
                window_block += chunk_blocks;
                if window_block >= blocks.end {
                    return Ok(());
                }
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MkfsOptions;
    use storage::MemBlockDevice;

    /// TASK-0179 streaming recut: a container-sized file (past the old
    /// 4 MiB materialize cap) round-trips through windowed CoW writes and
    /// window reads; interior overwrites, sparse extension, truncate
    /// shrink/grow and the stale-tail-zero rule all hold.
    #[test]
    fn streaming_large_file_windows() {
        // 19 MB file on a 24k-block (94 MB) volume.
        let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 24 * 1024);
        let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
        fs.create("/big.bin").expect("create");
        let total: usize = 19 * 1024 * 1024 + 137; // deliberately unaligned
        let pattern = |i: usize| (i % 251) as u8;

        // Sequential 64 KiB append stream (the staging shape).
        let chunk = 64 * 1024;
        let mut buf = alloc::vec![0u8; chunk];
        let mut off = 0usize;
        while off < total {
            let take = chunk.min(total - off);
            for (j, slot) in buf[..take].iter_mut().enumerate() {
                *slot = pattern(off + j);
            }
            fs.write("/big.bin", off as u64, &buf[..take]).expect("append");
            off += take;
        }
        let (_kind, size) = fs.stat("/big.bin").expect("stat");
        assert_eq!(size, total as u64);

        // Window reads at unaligned interior offsets.
        for (read_off, read_len) in
            [(0usize, 4096usize), (5 * 1024 * 1024 + 13, 100_000), (total - 137, 137)]
        {
            let bytes = fs.read("/big.bin", read_off as u64, read_len).expect("window read");
            assert_eq!(bytes.len(), read_len.min(total - read_off));
            assert!(
                bytes.iter().enumerate().all(|(j, b)| *b == pattern(read_off + j)),
                "window {read_off}+{read_len} content"
            );
        }

        // Interior overwrite (unaligned, crossing block edges) touches
        // ONLY its window.
        let overwrite_off = 7 * 1024 * 1024 + 777;
        fs.write("/big.bin", overwrite_off as u64, &[0xEE; 10_000]).expect("overwrite");
        let bytes = fs.read("/big.bin", overwrite_off as u64 - 8, 10_016).expect("read back");
        assert!(bytes[..8].iter().enumerate().all(|(j, b)| *b == pattern(overwrite_off - 8 + j)));
        assert!(bytes[8..10_008].iter().all(|b| *b == 0xEE));
        assert!(bytes[10_008..]
            .iter()
            .enumerate()
            .all(|(j, b)| *b == pattern(overwrite_off + 10_000 + j)));

        // Truncate shrink to an unaligned size, then grow: the stale tail
        // beyond the shrink point must read back as ZERO (old resize
        // semantics preserved by the tail-block CoW).
        let shrink_to = 3 * 1024 * 1024 + 55;
        fs.truncate("/big.bin", shrink_to as u64).expect("shrink");
        fs.truncate("/big.bin", (shrink_to + 9_000) as u64).expect("grow");
        let tail = fs.read("/big.bin", shrink_to as u64 - 5, 9_005).expect("tail read");
        assert!(tail[..5].iter().enumerate().all(|(j, b)| *b == pattern(shrink_to - 5 + j)));
        assert!(tail[5..].iter().all(|b| *b == 0), "grown region must be zero");

        // Sparse extension: a write past EOF zero-fills the gap.
        fs.create("/sparse.bin").expect("create sparse");
        fs.write("/sparse.bin", 0, b"head").expect("head");
        fs.write("/sparse.bin", 10_000, b"tail").expect("sparse write");
        let gap = fs.read("/sparse.bin", 4, 9_996).expect("gap read");
        assert!(gap.iter().all(|b| *b == 0), "gap reads zero");
        assert_eq!(fs.read("/sparse.bin", 10_000, 4).expect("tail"), b"tail");

        // Remount: the spliced extent lists survive the checkpoint cycle.
        fs.write_checkpoint().expect("checkpoint");
        let device = fs.into_device();
        let fs = Nxfs::mount(device).expect("remount");
        let bytes = fs.read("/big.bin", 0, 64).expect("post-mount read");
        assert!(bytes.iter().enumerate().all(|(j, b)| *b == pattern(j)));
    }

    /// The `OP_WRITE_VMO` wire bound lives in vfs-types, below this crate in
    /// the graph, so it cannot name the engine's cap — this pins the match.
    #[test]
    fn write_vmo_wire_cap_is_the_file_cap() {
        assert_eq!(u64::from(nexus_vfs_types::fileops::MAX_WRITE_VMO_BYTES), MAX_FILE_BYTES);
    }

    /// TASK-0068: every write of a mount fills through the SAME working
    /// buffer — nothing before the first write (a read-only mount never pays),
    /// one allocation after it, reused by every write shape. Its stale bytes
    /// never reach a file: the grown tail below reads zero although the
    /// buffer last carried other content.
    #[test]
    fn writes_reuse_one_work_buffer() {
        let device = MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 4096);
        let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
        assert!(fs.work_buf.is_empty(), "nothing is allocated before the first write");
        fs.create("/a").expect("create");
        fs.write("/a", 0, &[1u8; 100_000]).expect("first write"); // two chunks
        assert_eq!(fs.work_buf.len(), WORK_BYTES);
        let buffer = (fs.work_buf.as_ptr(), fs.work_buf.capacity());

        fs.write("/a", 100_000, &[2u8; 5_000]).expect("append");
        fs.write("/a", 777, &[3u8; 9_000]).expect("interior overwrite");
        fs.truncate("/a", 50_001).expect("shrink (tail-block CoW)");
        fs.truncate("/a", 300_000).expect("grow (zero fill)");
        assert_eq!((fs.work_buf.as_ptr(), fs.work_buf.capacity()), buffer, "reused, never re-made");

        let bytes = fs.read("/a", 0, 300_000).expect("read back");
        assert_eq!(bytes.len(), 300_000);
        assert!(bytes[..777].iter().all(|b| *b == 1));
        assert!(bytes[777..9_777].iter().all(|b| *b == 3));
        assert!(bytes[9_777..50_001].iter().all(|b| *b == 1));
        assert!(bytes[50_001..].iter().all(|b| *b == 0), "grown tail reads zero");
    }

    /// A fill that fails on the device — reading the old head/tail or
    /// writing a fresh chunk — hands the working buffer back (a lost one
    /// would be re-allocated, and leaked, by the next write) and returns
    /// every fresh block to the pool. The volume is sized so ONE leaked
    /// rewrite would make the next one `NoSpace`.
    #[test]
    fn test_reject_failed_fill_keeps_the_buffer_and_the_blocks() {
        use std::{cell::Cell, rc::Rc};
        use storage::BlockError;

        /// Fails every sector read (mode 1) or write (mode 2) while armed.
        struct Flaky {
            inner: MemBlockDevice,
            mode: Rc<Cell<u8>>,
        }
        impl BlockDevice for Flaky {
            fn block_size(&self) -> usize {
                self.inner.block_size()
            }
            fn block_count(&self) -> u64 {
                self.inner.block_count()
            }
            fn read_block(&self, i: u64, b: &mut [u8]) -> core::result::Result<(), BlockError> {
                if self.mode.get() == 1 {
                    return Err(BlockError::IoError);
                }
                self.inner.read_block(i, b)
            }
            fn write_block(&mut self, i: u64, b: &[u8]) -> core::result::Result<(), BlockError> {
                if self.mode.get() == 2 {
                    return Err(BlockError::IoError);
                }
                self.inner.write_block(i, b)
            }
            fn sync(&mut self) -> core::result::Result<(), BlockError> {
                self.inner.sync()
            }
        }

        // 256 blocks leave 158 data blocks: a 60-block file plus ONE
        // copy-on-write rewrite of it fits, a leaked rewrite on top does not.
        let mode = Rc::new(Cell::new(0u8));
        let device =
            Flaky { inner: MemBlockDevice::new(LOGICAL_BLOCK_SIZE, 256), mode: Rc::clone(&mode) };
        let mut fs = Nxfs::mkfs(device, MkfsOptions::default()).expect("mkfs");
        let size = 60 * LOGICAL_BLOCK_SIZE;
        fs.create("/f").expect("create");
        fs.write("/f", 0, &alloc::vec![1u8; size]).expect("seed");
        assert_eq!(fs.work_buf.len(), WORK_BYTES, "the seed write made the buffer");
        let buffer = (fs.work_buf.as_ptr(), fs.work_buf.capacity());

        // An unaligned rewrite reads the old head block, then writes.
        let rewrite = alloc::vec![2u8; size - 2];
        for armed in [1u8, 2] {
            mode.set(armed);
            assert_eq!(fs.write("/f", 1, &rewrite), Err(NxfsError::Io), "mode {armed}");
            mode.set(0);
            let now = (fs.work_buf.as_ptr(), fs.work_buf.capacity());
            assert_eq!(now, buffer, "mode {armed}: the buffer came back");
        }
        fs.write("/f", 1, &rewrite).expect("no fresh block leaked by the failed fills");
        let bytes = fs.read("/f", 0, size).expect("read back");
        assert_eq!((bytes[0], bytes[size - 1]), (1, 1));
        assert!(bytes[1..size - 1].iter().all(|b| *b == 2));
    }
}
