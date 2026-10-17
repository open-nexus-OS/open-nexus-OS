// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd's GPU context (TASK-0215 5c-3 P3, RFC-0101 §1, §7, §7b, ADR-0072 §6). At
//! start — after the framebuffer grant, before the attach — windowd asks gpud what its GPU runs
//! and opens its context at the highest priority with its pipeline library for that code
//! target, a VMO moved to gpud. windowd's libraries are built at OS build time (`nxgpu
//! library`): one for the board's GPU, one for QEMU's host renderer; gpud holds none of its own.
//! A display whose GPU runs no code (QEMU's 2D device) is named, and nothing is opened. The waits
//! have no clock: the answer or gpud's death ends them (`nexus_ipc::exchange`).
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wire in `nexus_display_proto::gpu_context` and the pick in
//!   `nexus_gfx::shader::library::header_target` (host, `test_reject_*`); the QEMU display lanes
//!   (`windowd: gpu context ok (`), the board (`gpud: gpu library ok (client=windowd`)

/// windowd's libraries, one per code target (`resources/gpu/pipelines/README.md`).
#[cfg(nexus_env = "os")]
const LIBRARIES: [&[u8]; 2] = [
    include_bytes!("../../../../../resources/gpu/pipelines/compositor.nxgp"),
    include_bytes!("../../../../../resources/gpu/pipelines/compositor-virgl.nxgp"),
];

/// Opens windowd's GPU context with its library (once, at start); a line names the outcome.
#[cfg(nexus_env = "os")]
pub(crate) fn open() {
    use nexus_display_proto::gpu_context::{self as wire, ContextOpen, PRIORITY_REALTIME};
    use nexus_display_proto::{STATUS_OK, STATUS_UNSUPPORTED};
    let route = nexus_service_topology::slots::windowd::GPUD;
    let mut reply = [0u8; wire::REPLY_LEN];
    if let Err(err) =
        nexus_ipc::exchange::send_request(route.send, &wire::encode_capabilities_request())
    {
        return fail(&alloc::format!("gpud unreachable: {err:?}"));
    }
    let caps = match nexus_ipc::exchange::recv_response(route.recv, &mut reply) {
        Ok(n) => wire::decode_capabilities_reply(&reply[..n.min(reply.len())]),
        Err(err) => return fail(&alloc::format!("gpud gone: {err:?}")),
    };
    let caps = match caps {
        Some(c) if c.status == STATUS_OK => c,
        Some(c) if c.status == STATUS_UNSUPPORTED => {
            let _ = nexus_abi::debug_println(
                "windowd: gpu context none (the display's GPU runs no code)",
            );
            return;
        }
        other => return fail(&alloc::format!("capabilities {other:?}")),
    };
    let target = caps.code_target;
    let Some(library) =
        LIBRARIES.iter().find(|l| nexus_gfx::shader::library::header_target(l) == Some(target))
    else {
        return fail(&alloc::format!("no library for code target {target:#x}"));
    };
    let Ok(library_len) = u32::try_from(library.len()) else {
        return fail("a library past the wire's bound");
    };
    let vmo = match nexus_abi::vmo_create(library.len()) {
        Ok(v) => v,
        Err(err) => return fail(&alloc::format!("vmo {err:?}")),
    };
    if let Err(err) = nexus_abi::vmo_write(vmo, 0, library) {
        let _ = nexus_abi::cap_close(vmo);
        return fail(&alloc::format!("vmo write {err:?}"));
    }
    let frame = wire::encode_open(&ContextOpen { priority: PRIORITY_REALTIME, library_len });
    if let Err(err) = nexus_ipc::exchange::send_with_cap(route.send, &frame, vmo) {
        let _ = nexus_abi::cap_close(vmo);
        return fail(&alloc::format!("open unsent: {err:?}"));
    }
    let answer = match nexus_ipc::exchange::recv_response(route.recv, &mut reply) {
        Ok(n) => wire::decode_open_reply(&reply[..n.min(reply.len())]),
        Err(err) => return fail(&alloc::format!("gpud gone: {err:?}")),
    };
    match answer {
        Some(a) if a.status == STATUS_OK && a.refused == 0 => {
            let _ = nexus_abi::debug_println(&alloc::format!(
                "windowd: gpu context ok (ctx={:#x} entries={} bytes={} target={target:#x} features={:#x})",
                a.ctx,
                a.accepted,
                library.len(),
                caps.features
            ));
        }
        other => fail(&alloc::format!("open {other:?}")),
    }
}

#[cfg(nexus_env = "os")]
fn fail(why: &str) {
    let _ = nexus_abi::debug_println(&alloc::format!("windowd: FAIL gpu context ({why})"));
}

/// Host builds have no gpud: nothing to open.
#[cfg(not(nexus_env = "os"))]
pub(crate) fn open() {}
