// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxfs in-memory state — object table, directory tables, and the
//! block-allocation bitmap. The bitmap is DERIVED (reserved regions + live
//! extents), never persisted: checkpoint load and journal replay rebuild it
//! through the same idempotent mark/clear ops the runtime uses, so the
//! allocator can never drift from the metadata.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: allocator + apply-op tests below

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::format::{KIND_DIR, KIND_FILE, ROOT_OBJECT};
use crate::journal::Op;
use crate::{NxfsError, Result};

/// One contiguous extent of logical blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Extent {
    pub lb: u64,
    pub blocks: u32,
}

/// One object (file or directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    pub kind: u8,
    pub size: u64,
    pub extents: Vec<Extent>,
}

/// Directory content: name → (object id, kind). BTreeMap = canonical order.
pub(crate) type DirTable = BTreeMap<String, (u64, u8)>;

/// The mutable filesystem state.
#[derive(Debug)]
pub(crate) struct State {
    pub objects: BTreeMap<u64, Object>,
    pub dirs: BTreeMap<u64, DirTable>,
    pub next_object: u64,
    /// Allocation bitmap over logical blocks (bit set = used).
    bitmap: Vec<u64>,
    /// First allocatable logical block (everything below is reserved).
    data_start: u64,
    total_blocks: u64,
}

impl State {
    /// Fresh state with an empty root directory; all blocks below
    /// `data_start` (superblock/journal/checkpoint regions) plus the mirror
    /// superblock are reserved.
    pub(crate) fn new_empty(total_blocks: u64, data_start: u64) -> Self {
        let words = total_blocks.div_ceil(64) as usize;
        let mut state = Self {
            objects: BTreeMap::new(),
            dirs: BTreeMap::new(),
            next_object: ROOT_OBJECT + 1,
            bitmap: alloc::vec![0u64; words],
            data_start,
            total_blocks,
        };
        for lb in 0..data_start {
            state.mark_used(lb);
        }
        state.mark_used(total_blocks - 1); // superblock mirror
        state.objects.insert(ROOT_OBJECT, Object { kind: KIND_DIR, size: 0, extents: Vec::new() });
        state.dirs.insert(ROOT_OBJECT, DirTable::new());
        state
    }

    fn mark_used(&mut self, lb: u64) {
        // Guarded: callers validate bounds; a stray bit is never a panic.
        if let Some(word) = self.bitmap.get_mut((lb / 64) as usize) {
            *word |= 1u64 << (lb % 64);
        }
    }

    fn mark_free(&mut self, lb: u64) {
        if let Some(word) = self.bitmap.get_mut((lb / 64) as usize) {
            *word &= !(1u64 << (lb % 64));
        }
    }

    fn is_used(&self, lb: u64) -> bool {
        self.bitmap.get((lb / 64) as usize).is_none_or(|word| (word >> (lb % 64)) & 1 == 1)
    }

    /// `Integrity` unless every extent lies inside the container.
    fn validate_extents(&self, extents: &[Extent]) -> Result<()> {
        for extent in extents {
            if extent.lb.saturating_add(u64::from(extent.blocks)) > self.total_blocks {
                return Err(NxfsError::Integrity);
            }
        }
        Ok(())
    }

    /// Marks an extent list used (idempotent — replay and runtime share it).
    pub(crate) fn mark_extents(&mut self, extents: &[Extent]) {
        for extent in extents {
            for i in 0..u64::from(extent.blocks) {
                self.mark_used(extent.lb + i);
            }
        }
    }

    /// Frees an extent list (idempotent).
    pub(crate) fn free_extents(&mut self, extents: &[Extent]) {
        for extent in extents {
            for i in 0..u64::from(extent.blocks) {
                self.mark_free(extent.lb + i);
            }
        }
    }

    /// Allocates `blocks` logical blocks (possibly fragmented), marking them
    /// used. Fails with `NoSpace` leaving the bitmap untouched.
    pub(crate) fn alloc_blocks(&mut self, blocks: u64) -> Result<Vec<Extent>> {
        if blocks == 0 {
            return Ok(Vec::new());
        }
        let mut found: Vec<Extent> = Vec::new();
        let mut need = blocks;
        let mut run_start: Option<u64> = None;
        let mut run_len: u64 = 0;
        let mut lb = self.data_start;
        while lb < self.total_blocks && need > 0 {
            if !self.is_used(lb) {
                if run_start.is_none() {
                    run_start = Some(lb);
                    run_len = 0;
                }
                run_len += 1;
                if run_len == need || run_len == u64::from(u32::MAX) {
                    found.push(Extent { lb: run_start.unwrap_or(lb), blocks: run_len as u32 });
                    need -= run_len;
                    run_start = None;
                    run_len = 0;
                }
            } else if let Some(start) = run_start.take() {
                found.push(Extent { lb: start, blocks: run_len as u32 });
                need -= run_len;
                run_len = 0;
            }
            lb += 1;
        }
        if need > 0 {
            return Err(NxfsError::NoSpace);
        }
        self.mark_extents(&found);
        Ok(found)
    }

    /// Applies one journal op. This is THE mutation semantic — runtime commit
    /// and crash replay both go through here (no second implementation to
    /// drift). Idempotent on the bitmap by construction.
    pub(crate) fn apply(&mut self, op: &Op) -> Result<()> {
        match op {
            Op::MkNode { parent, id, kind, name } => {
                let dir = self.dirs.get_mut(parent).ok_or(NxfsError::Integrity)?;
                dir.insert(name.clone(), (*id, *kind));
                self.objects.insert(*id, Object { kind: *kind, size: 0, extents: Vec::new() });
                if *kind == KIND_DIR {
                    self.dirs.insert(*id, DirTable::new());
                }
                if *id >= self.next_object {
                    self.next_object = *id + 1;
                }
                Ok(())
            }
            Op::Write { id, size, extents } => {
                self.validate_extents(extents)?;
                let object = self.objects.get_mut(id).ok_or(NxfsError::Integrity)?;
                if object.kind != KIND_FILE {
                    return Err(NxfsError::Integrity);
                }
                let old = core::mem::take(&mut object.extents);
                object.size = *size;
                object.extents = extents.clone();
                let new_extents = extents.clone();
                self.free_extents(&old);
                self.mark_extents(&new_extents);
                Ok(())
            }
            Op::Remove { parent, id, name } => {
                let dir = self.dirs.get_mut(parent).ok_or(NxfsError::Integrity)?;
                dir.remove(name);
                if let Some(object) = self.objects.remove(id) {
                    self.free_extents(&object.extents);
                    if object.kind == KIND_DIR {
                        self.dirs.remove(id);
                    }
                }
                Ok(())
            }
            Op::Rename { from_parent, from_name, to_parent, to_name, replaced } => {
                let entry = {
                    let from = self.dirs.get_mut(from_parent).ok_or(NxfsError::Integrity)?;
                    from.remove(from_name).ok_or(NxfsError::Integrity)?
                };
                if *replaced != 0 {
                    if let Some(object) = self.objects.remove(replaced) {
                        self.free_extents(&object.extents);
                        if object.kind == KIND_DIR {
                            self.dirs.remove(replaced);
                        }
                    }
                }
                let to = self.dirs.get_mut(to_parent).ok_or(NxfsError::Integrity)?;
                to.insert(to_name.clone(), entry);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_respects_reserved_and_frees() {
        let mut state = State::new_empty(128, 10);
        let a = state.alloc_blocks(5).expect("alloc");
        assert!(a.iter().all(|e| e.lb >= 10));
        let b = state.alloc_blocks(3).expect("alloc");
        assert_ne!(a[0].lb, b[0].lb);
        state.free_extents(&a);
        let c = state.alloc_blocks(5).expect("realloc");
        assert_eq!(c[0].lb, a[0].lb, "freed space is reused deterministically");
        // Mirror superblock stays reserved.
        let big = state.alloc_blocks(128);
        assert_eq!(big, Err(NxfsError::NoSpace));
    }

    #[test]
    fn apply_ops_roundtrip() {
        let mut state = State::new_empty(128, 10);
        state
            .apply(&Op::MkNode {
                parent: ROOT_OBJECT,
                id: 2,
                kind: KIND_FILE,
                name: "a.txt".into(),
            })
            .expect("mknode");
        let extents = state.alloc_blocks(2).expect("alloc");
        state.apply(&Op::Write { id: 2, size: 5000, extents: extents.clone() }).expect("write");
        assert_eq!(state.objects[&2].size, 5000);
        state
            .apply(&Op::Rename {
                from_parent: ROOT_OBJECT,
                from_name: "a.txt".into(),
                to_parent: ROOT_OBJECT,
                to_name: "b.txt".into(),
                replaced: 0,
            })
            .expect("rename");
        assert!(state.dirs[&ROOT_OBJECT].contains_key("b.txt"));
        state
            .apply(&Op::Remove { parent: ROOT_OBJECT, id: 2, name: "b.txt".into() })
            .expect("remove");
        assert!(state.objects.get(&2).is_none());
        // Freed blocks are allocatable again.
        let again = state.alloc_blocks(2).expect("alloc");
        assert_eq!(again[0].lb, extents[0].lb);
    }
}
