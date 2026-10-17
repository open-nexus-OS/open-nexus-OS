// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pipeline library on the device (TASK-0215 5c-2 P4d, RFC-0101 §7b): the render rungs'
//! context takes the compositor's library as a client's context would (`GfxDevice::
//! take_library`, `library.rs`) — this rung's fixture (`resources/gpu/pipelines/
//! compositor.nxgp`), with whose pipelines the rungs after it draw by id. windowd's context takes
//! the library windowd supplies over IPC (5c-3 P3, `client_context.rs`); the fixture serves the
//! boot rungs alone, which run before any client is there. The table keeps each pipeline's
//! placement and setup and nothing else of gpud's heap (`table_bytes=`): the programs' groups
//! are counted as they are decoded.

use nexus_gfx::backend::v2::GfxDevice;

use super::device_v2::Device;
use super::emit;
use super::render_template::RungContext;

const LIBRARY: &[u8] = include_bytes!("../../../../../../resources/gpu/pipelines/compositor.nxgp");

/// The rung: after `gpu render ok`, in the render rung's context.
#[inline(never)]
pub(crate) fn rung(dev: &mut Device, rc: &RungContext) {
    let (client, ctx) = (rc.client, rc.ctx);
    // What the table keeps of gpud's heap, which never frees: each pipeline's placement and
    // setup, no program decoded into memory.
    let heap_before = nexus_service_entry::os::heap_cursor().1;
    let taken = dev.take_library(client, ctx, LIBRARY);
    let table_bytes = nexus_service_entry::os::heap_cursor().1.saturating_sub(heap_before);
    match taken {
        Ok(t) if t.refused == 0 => {
            let ids = dev
                .pipeline_ids(client, ctx)
                .iter()
                .map(|id| alloc::format!("{id}"))
                .collect::<alloc::vec::Vec<_>>();
            emit(&alloc::format!(
                "gpud: gpu library ok (client=gpud bytes={} entries={} refused=0 code_bytes={} setup_bytes={} table_bytes={table_bytes} ids={})",
                LIBRARY.len(),
                t.accepted,
                t.code_bytes,
                t.setup_bytes,
                ids.join(",")
            ));
        }
        Ok(t) => emit(&alloc::format!(
            "gpud: FAIL gpu library (client=gpud accepted={} refused={} first_refused={})",
            t.accepted,
            t.refused,
            t.first_refused
        )),
        Err(e) => emit(&alloc::format!("gpud: FAIL gpu library (client=gpud {e:?})")),
    }
}
