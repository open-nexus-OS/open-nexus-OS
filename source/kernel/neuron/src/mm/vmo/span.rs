// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: An object's bytes as physical pieces (split out of `vmo.rs` under the structure
//! ratchet, TASK-0348): `span` answers `[offset, offset + len)` of any kind as `(pa, len)` pieces
//! — a lazy page absent is `None`, committed first when the caller writes or a device is told —
//! and `named_runs` merges a lazy range into the runs a device is given (ADR-0078).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! TEST_COVERAGE: the VMO selftests (`vmo runs ok`, `vmo lazy device ok`) and every write path
//! ADR: docs/adr/0078-gpu-memory-is-the-one-pools-committed-when-the-gpu-is-told-its-pages-its-share-a-fraction-of-ram.md

use alloc::vec::Vec;

use super::{commit, runs, with, VmoError, VmoId};
use crate::lazy::{PageState, PAGE};

/// Lazy page `index` of `id` for a read: its frame, `None` when absent (zero) — a squeezed or
/// swapped page is content, never zero: it is brought back first (ADR-0087, ADR-0088).
fn resident(id: VmoId, index: usize) -> Result<Option<usize>, VmoError> {
    match with(id, |o| o.lazy.as_ref().and_then(|p| p.state(index)))
        .ok_or(VmoError::NoSuchObject)?
    {
        Some(PageState::Frame(pa)) => Ok(Some(pa as usize)),
        Some(PageState::Absent) => Ok(None),
        Some(PageState::Record(_) | PageState::Swapped) => commit(id, index).map(Some),
        None => Err(VmoError::BadLength),
    }
}

/// The bytes `[offset, offset + len)` of `id` as pieces `(pa, len)` in order: the runs of an
/// eager object, the pages of a lazy one — `None` where a lazy page is absent. With
/// `commit_pages` every lazy page of the span is committed first (a write, a device's runs), so
/// no piece is `None`; a squeezed page is brought back either way.
pub fn span(
    id: VmoId,
    offset: usize,
    len: usize,
    commit_pages: bool,
) -> Result<Vec<(Option<usize>, usize)>, VmoError> {
    let end = offset.checked_add(len).ok_or(VmoError::BadLength)?;
    let (lazy, object_len) =
        with(id, |o| (o.lazy.is_some(), o.len)).ok_or(VmoError::NoSuchObject)?;
    if end > object_len {
        return Err(VmoError::BadLength);
    }
    let mut out = Vec::new();
    if !lazy {
        let mut at = 0usize;
        for (pa, run_len) in runs(id).ok_or(VmoError::NoSuchObject)? {
            let (lo, hi) = (offset.max(at), end.min(at + run_len));
            if lo < hi {
                out.push((Some(pa + (lo - at)), hi - lo));
            }
            at += run_len;
        }
        return Ok(out);
    }
    let mut at = offset;
    while at < end {
        let piece = (end - at).min(PAGE - at % PAGE);
        let base =
            if commit_pages { Some(commit(id, at / PAGE)?) } else { resident(id, at / PAGE)? };
        out.push((base.map(|pa| pa + at % PAGE), piece));
        at += piece;
    }
    Ok(out)
}

/// `[offset, offset + len)` of a lazy object as a device is told it (RFC-0100 §2, ADR-0078):
/// every page committed first — a device cannot fault — then the pieces merged into runs, in
/// range order. At most `MAX_NAMED_PAGES` pages at once: the commits run under the kernel lock.
pub fn named_runs(id: VmoId, offset: usize, len: usize) -> Result<Vec<(usize, usize)>, VmoError> {
    if !crate::lazy::nameable(offset, len) {
        return Err(VmoError::BadLength);
    }
    let pieces = span(id, offset, len, true)?;
    Ok(crate::lazy::merge_runs(pieces.into_iter().filter_map(|(pa, len)| Some((pa?, len)))))
}
