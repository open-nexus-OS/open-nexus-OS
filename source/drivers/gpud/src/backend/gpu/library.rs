// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A context's pipeline library on the board (TASK-0215 5c-3 P3, RFC-0101 §7b): the client's
//! archive parsed for this core and its features, each accepted entry checked again for it (its
//! program decodes within its budget, its setup is one this device runs), its programs placed
//! in the context's code heap and its setup programs in its setup heap, each kick naming its
//! program — `GfxDevice::take_library` of the device. The places are the render template's:
//! the code arena's free tail and the head of the PDS arena's free lines
//! (`render_template::LIBRARY_*`), written once and never again, no buffer of their own. The
//! render rungs' context takes the fixture library (`render_library.rs`), windowd's context the
//! library windowd supplies over IPC (`client_context.rs`); gpud holds no program of its own.

use nexus_gfx::backend::gpu::pipelines::{Heaps, LIBRARY_CODE_BYTES, LIBRARY_SETUP_BYTES};
use nexus_gfx::backend::v2::{ClientId, ContextId, DeviceError, GfxDevice, LibraryTaken};
use nexus_gfx::shader::library::feature;

use super::device_v2::Device;
use super::render_template::{
    pds_at, write_bound, CODE_ARENA, CODE_HEAP_BASE, LIBRARY_CODE, LIBRARY_PDS, PDS_ARENA,
};

/// What this core runs beyond the format's baseline: iterate-and-sample (5c-2 cycle 14).
pub(super) const CODE_FEATURES: u32 = feature::FUSED_ITERATE_SAMPLE;
/// Programs and setup programs each start a 64-byte line.
const LINE: u32 = 64;

// The template's places are the room the contract gives a library — the mock's too.
const _: () = assert!(LIBRARY_CODE.1 - LIBRARY_CODE.0 == LIBRARY_CODE_BYTES);
const _: () = assert!(LIBRARY_PDS.1 - LIBRARY_PDS.0 == LIBRARY_SETUP_BYTES);

/// The context's heaps, as far as the library may fill them.
struct Bound<'a> {
    dev: &'a mut Device,
    client: ClientId,
    ctx: ContextId,
    /// The next free code arena offset and setup heap offset.
    code: u32,
    pds: u32,
}

impl Heaps for Bound<'_> {
    fn code(&mut self, bytes: &[u8]) -> Option<u32> {
        let at = self.code.next_multiple_of(LINE);
        let end = at.checked_add(u32::try_from(bytes.len()).ok()?)?;
        if end > LIBRARY_CODE.1 {
            return None;
        }
        write_bound(self.dev, self.client, self.ctx, CODE_ARENA, at, bytes).ok()?;
        self.code = end;
        Some((CODE_ARENA - CODE_HEAP_BASE) as u32 + at)
    }

    fn setup(&mut self, bytes: &[u8]) -> Option<u32> {
        let at = self.pds.next_multiple_of(LINE);
        let end = at.checked_add(u32::try_from(bytes.len()).ok()?)?;
        if end > LIBRARY_PDS.1 {
            return None;
        }
        write_bound(self.dev, self.client, self.ctx, PDS_ARENA, pds_at(PDS_ARENA, at), bytes)
            .ok()?;
        self.pds = end;
        Some(at)
    }
}

impl Device {
    /// Takes `client`'s library into its context `ctx` (refused when the context is lost or
    /// holds one); the table keeps each pipeline's placement and setup, nothing of its bytes.
    pub(super) fn place_library(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        library: &[u8],
    ) -> Result<LibraryTaken, DeviceError> {
        let target = self.capabilities().gpu_code_target.0;
        let mut table = self.take_pipelines(client, ctx)?;
        let loaded = {
            let mut heaps =
                Bound { dev: self, client, ctx, code: LIBRARY_CODE.0, pds: LIBRARY_PDS.0 };
            table.load(library, target, CODE_FEATURES, &mut heaps)
        };
        self.set_pipelines(client, ctx, table)?;
        Ok(LibraryTaken::from(&loaded?))
    }
}
