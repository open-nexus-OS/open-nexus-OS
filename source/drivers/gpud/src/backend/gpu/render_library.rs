// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pipeline library on the device (TASK-0215 5c-2 P4d, RFC-0101 §7b): the compositor's
//! library (`resources/gpu/pipelines/compositor.nxgp`) taken for the rung's context as a client's
//! would be — parsed for this core, each entry checked again (its program decodes within its
//! budget, its setup is one this device runs), its programs placed in the context's code heap
//! and its setup programs in its setup heap, each kick naming its program. The rungs after it
//! draw with its pipelines by id; gpud holds no program of its own, and patches none. The places
//! are written once and never again: the code arena's free tail, the head of the PDS arena's
//! free lines (`render_template::LIBRARY_*`) — no buffer of its own (gpud's address space holds
//! 64 regions). The table keeps each pipeline's placement and setup and nothing else of gpud's
//! heap (`table_bytes=`): the programs' groups are counted as they are decoded. The client's
//! supply over IPC arrives with windowd's own context (5c-3).

use nexus_gfx::backend::gpu::pipelines::{Heaps, Loaded};
use nexus_gfx::backend::v2::{ClientId, GfxDevice};
use nexus_gfx::shader::library::feature;

use super::device_v2::Device;
use super::emit;
use super::render_template::{
    pds_at, write_bound, CODE_ARENA, CODE_HEAP_BASE, LIBRARY_CODE, LIBRARY_PDS, PDS_ARENA,
};

const LIBRARY: &[u8] = include_bytes!("../../../../../../resources/gpu/pipelines/compositor.nxgp");
/// What this core runs beyond the format's baseline: iterate-and-sample (5c-2 cycle 14).
const FEATURES: u32 = feature::FUSED_ITERATE_SAMPLE;
/// Programs and setup programs each start a 64-byte line.
const LINE: u32 = 64;

/// The context's heaps, as far as the library may fill them.
struct Bound<'a> {
    dev: &'a mut Device,
    client: ClientId,
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
        write_bound(self.dev, self.client, CODE_ARENA, at, bytes).ok()?;
        self.code = end;
        Some((CODE_ARENA - CODE_HEAP_BASE) as u32 + at)
    }

    fn setup(&mut self, bytes: &[u8]) -> Option<u32> {
        let at = self.pds.next_multiple_of(LINE);
        let end = at.checked_add(u32::try_from(bytes.len()).ok()?)?;
        if end > LIBRARY_PDS.1 {
            return None;
        }
        write_bound(self.dev, self.client, PDS_ARENA, pds_at(PDS_ARENA, at), bytes).ok()?;
        self.pds = end;
        Some(at)
    }
}

/// The rung: after `gpu render ok`, with the render rung's bindings.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device) {
    let client = ClientId(nexus_abi::service_id_from_name(b"gpud"));
    let target = dev.capabilities().gpu_code_target.0;
    let mut table = core::mem::take(&mut dev.pipelines);
    // What the table keeps of gpud's heap, which never frees: each pipeline's placement and
    // setup, no program decoded into memory.
    let heap_before = nexus_service_entry::os::heap_cursor().1;
    let loaded = {
        let mut heaps = Bound { dev, client, code: LIBRARY_CODE.0, pds: LIBRARY_PDS.0 };
        table.load(LIBRARY, target, FEATURES, &mut heaps)
    };
    let table_bytes = nexus_service_entry::os::heap_cursor().1.saturating_sub(heap_before);
    dev.pipelines = table;
    match loaded {
        Ok(Loaded { accepted, refused, code_bytes, setup_bytes }) if refused.is_empty() => {
            let ids =
                accepted.iter().map(|id| alloc::format!("{id}")).collect::<alloc::vec::Vec<_>>();
            emit(&alloc::format!(
                "gpud: gpu library ok (bytes={} entries={} refused=0 code_bytes={code_bytes} setup_bytes={setup_bytes} table_bytes={table_bytes} ids={})",
                LIBRARY.len(),
                accepted.len(),
                ids.join(",")
            ));
        }
        Ok(Loaded { accepted, refused, .. }) => emit(&alloc::format!(
            "gpud: FAIL gpu library (accepted={accepted:?} refused={refused:?})"
        )),
        Err(e) => emit(&alloc::format!("gpud: FAIL gpu library ({e:?})")),
    }
}
