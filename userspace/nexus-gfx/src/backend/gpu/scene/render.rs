// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render trailer of a version-2 bundle (TASK-0215 5a, RFC-0101 §8d): the dataset's
//! arguments, the geometry's call stack, the two free lists and the render context's static
//! state, as the open driver asked for them. Part of [`super`] (`scene`).

use super::{align8, get32, get64, FreelistArgs, HwrtArgs, SceneError};

/// The kernel side's objects of a render pass, as the open driver asked for them (version 2
/// bundles): the dataset's 136-byte arguments, the free lists `[local, global]`, the render
/// context's 192-byte static state stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderSetup<'a> {
    pub hwrt_args: &'a [u8],
    /// The geometry's call stack: a bound buffer the open driver named at context creation.
    pub callstack_addr: u64,
    pub freelists: [FreelistArgs; 2],
    pub static_state: &'a [u8],
}

/// Bytes of one free list record in the trailer.
const FREELIST_RECORD: usize = 24;

pub(super) fn read_render(b: &[u8], off: usize) -> Result<(RenderSetup<'_>, usize), SceneError> {
    let hwrt_args = b.get(off..off + HwrtArgs::SIZE).ok_or(SceneError::Truncated)?;
    let mut o = off + HwrtArgs::SIZE;
    let callstack_addr = get64(b, o)?;
    o += 8;
    let n = get32(b, o)?;
    o += 4;
    if n != 2 {
        return Err(SceneError::Freelists(n));
    }
    let mut freelists = [FreelistArgs::default(); 2];
    for fl in freelists.iter_mut() {
        *fl = FreelistArgs {
            gpu_addr: get64(b, o)?,
            initial_pages: get32(b, o + 8)?,
            max_pages: get32(b, o + 12)?,
            grow_pages: get32(b, o + 16)?,
            grow_threshold_percent: get32(b, o + 20)?,
        };
        o += FREELIST_RECORD;
    }
    let len = get32(b, o)? as usize;
    o += 4;
    let static_state = b.get(o..o + len).ok_or(SceneError::Truncated)?;
    if len != STATIC_STATE_STREAM {
        return Err(SceneError::StaticState(len as u32));
    }
    Ok((RenderSetup { hwrt_args, callstack_addr, freelists, static_state }, o + align8(len)))
}

/// The static render-context state stream's length (the 23 registers behind an 8-byte header).
const STATIC_STATE_STREAM: usize = 192;
