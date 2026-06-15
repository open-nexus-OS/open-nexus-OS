// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite gpud service entry for the QEMU virtio-gpu proof path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_abi::{debug_println, debug_write, mmio_map, nsec, yield_, AbiError};
use nexus_ipc::{KernelServer, Server as _, Wait};

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use crate::backend::VirtioGpuBackend;
use crate::markers::{
    GPUD_CURSOR_ON, GPUD_DISPLAY_READY, GPUD_MMIO_FAULT, GPUD_NO_DEVICE, GPUD_READY,
    GPUD_SCANOUT_MODE, GPUD_SCANOUT_OK, GPUD_VIRTIO_GPU_PROBED,
};

pub const ROUTE_NAME: &str = "gpud";
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
pub const OP_MOVE_CURSOR: u8 = 2;
pub const OP_SET_FRAMEBUFFER_VMO: u8 = 3;
pub const OP_PRESENT_DAMAGE: u8 = 4;
pub const OP_UPLOAD_CURSOR: u8 = 5;
/// Reply payloads for OP_UPLOAD_CURSOR (magic-tagged — distinguishable from
/// present acks, whose u32 slot carries a small handoff id).
pub const CURSOR_REPLY_HW: u32 = 0xC0DE_0001;
pub const CURSOR_REPLY_SW: u32 = 0xC0DE_0000;
pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_DEVICE_ERROR: u8 = 2;

const GPU_MMIO_CAP_SLOT: u32 = 48;
const GPU_MMIO_VA: usize = 0x2020_0000;
const GPU_MMIO_LEN: usize = 0x1000;
const GPUD_RECV_SLOT: u32 = 3;
const GPUD_SEND_SLOT: u32 = 4;
/// Display framebuffer dimensions matching windowd's VISIBLE_BOOTSTRAP_WIDTH/HEIGHT.
/// On QEMU virtio-gpu with `-display gtk`, the GTK window resizes to match this scanout.
const DISPLAY_WIDTH: u32 = 1280;
const DISPLAY_HEIGHT: u32 = 800;
// 6400 rows: 4 display planes (wallpaper/retained/slot-A/slot-B, 3200) + surface
// atlas (3200) for the retained-surface compositor's cached layers. MUST match
// windowd `crate::atlas::RESOURCE_HEIGHT` (separate crate, no shared dep).
const RESOURCE_HEIGHT: u32 = 6400;
/// Display plane row within the resource (fixed 4-plane layout). Matches
/// `backend::DISPLAY_PLANE_ROW` and windowd's `DISPLAY_ROW_OFFSET`.
const DISPLAY_PLANE_ROW: u32 = 1600;
pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let mut backend = open_backend_once()?;
    if backend.attach_bootstrap_text_scanout(DISPLAY_WIDTH, DISPLAY_HEIGHT).is_ok() {
        let _ = debug_println(GPUD_SCANOUT_OK);
        let _ = debug_println(GPUD_SCANOUT_MODE);
    } else if backend
        .attach_bootstrap_solid_scanout(DISPLAY_WIDTH, DISPLAY_HEIGHT, [0, 0, 0, 255])
        .is_ok()
    {
        let _ = debug_println("gpud: bootstrap text unavailable, fallback solid");
        let _ = debug_println(GPUD_SCANOUT_OK);
        let _ = debug_println(GPUD_SCANOUT_MODE);
    } else {
        let _ = debug_println("gpud: bootstrap scanout skipped");
    }

    // GPU-only architecture: gpud is a pure driver, not a display owner.
    // It probes the device and becomes IPC-ready. The scanout is set only
    // when windowd (the sole display owner) sends a framebuffer VMO via
    // OP_SET_FRAMEBUFFER_VMO. No boot splash, no startup create_resource.
    // Register in the global IPC registry BEFORE emitting the ready marker.
    // Windowd's KernelClient::new_for("gpud") depends on this registration.
    let server = bind_server()?;
    debug_println(GPUD_READY)?;
    service_requests(server, backend)
}

fn open_backend_once() -> Result<VirtioGpuBackend, nexus_abi::AbiError> {
    match mmio_map(GPU_MMIO_CAP_SLOT, GPU_MMIO_VA, 0) {
        Ok(()) => {}
        Err(AbiError::InvalidArgument) => return Err(AbiError::InvalidArgument),
        Err(_) => return Err(nexus_abi::AbiError::InvalidArgument),
    }
    let mut backend = VirtioGpuBackend::new(GPU_MMIO_VA, GPU_MMIO_LEN);
    match backend.probe() {
        Ok(()) => {
            debug_println(GPUD_VIRTIO_GPU_PROBED)?;
            Ok(backend)
        }
        Err(crate::error::GpuDriverError::DeviceNotFound) => {
            let _ = debug_println(GPUD_NO_DEVICE);
            Err(nexus_abi::AbiError::InvalidArgument)
        }
        Err(_) => {
            let _ = debug_println(GPUD_MMIO_FAULT);
            Err(nexus_abi::AbiError::InvalidArgument)
        }
    }
}

fn bind_server() -> Result<KernelServer, nexus_abi::AbiError> {
    if let Ok(server) = KernelServer::new_for(ROUTE_NAME) {
        let _ = debug_println("gpud: route connected");
        return Ok(server);
    }
    let _ = debug_println("gpud: route fallback slots");
    KernelServer::new_with_slots(GPUD_RECV_SLOT, GPUD_SEND_SLOT)
        .map_err(|_| nexus_abi::AbiError::InvalidArgument)
}

fn service_requests(
    server: KernelServer,
    mut backend: VirtioGpuBackend,
) -> Result<(), nexus_abi::AbiError> {
    // 8192 bytes: large enough for full cursor upload (32×32×4 = 4096B BGRA + 9B header).
    let mut recv_frame = [0u8; 8192];
    let mut active_handoff_id: u32 = 0;
    // Persistent present buffer: reused (reload_from) for every frame so gpud
    // does NOT allocate a fresh Vec<Command> per present. gpud runs on a
    // non-freeing bump allocator; a per-frame deserialize Vec would leak and
    // exhaust the 384KB heap after a few hundred animation frames (`alloc-fail
    // svc=gpud`), which is exactly what crashed the GPU pipeline mid-animation.
    let mut scene_cb = CommittedBuffer::with_capacity(32);
    // Present-time telemetry (frame budget for 120Hz = 8333us). Accumulated over
    // a window and emitted as a no-alloc marker every PRESENT_STATS_WINDOW
    // presents — gpud runs on a non-freeing bump allocator, so no per-frame
    // format!/heap. Lets us measure where the glass/compositor frame cost goes.
    const PRESENT_STATS_WINDOW: u32 = 120;
    let mut present_count: u32 = 0;
    let mut present_ns_sum: u64 = 0;
    let mut present_ns_max: u64 = 0;
    // Debug pipeline-bisection: dump an ASCII thumbnail of our actual output (the
    // windowd source plane and the GPU scanout readback) over the serial console.
    // Headless — no host display. Fires once at an early settled frame, then
    // periodically, so we can SEE what we render and where the frame breaks.
    let mut total_presents: u32 = 0;
    const THUMBNAIL_EVERY: u32 = 240;
    // Present-chain hop trace (graphical-output bisection): emit the per-frame
    // hops once a frame gets all the way through, but keep re-tracing every frame
    // while the chain is broken so a headless run shows exactly HOW FAR we get.
    let mut chain_trace_done = false;
    loop {
        // Reactive: block until windowd sends a command (framebuffer VMO, present damage,
        // or animation submit). No polling, no busy-wait. The kernel wakes us on message arrival.
        match server.recv_request_with_meta_into(Wait::Blocking, &mut recv_frame) {
            Ok((frame_len, _sid, mut moved_cap)) => {
                let frame = &recv_frame[..frame_len];
                let op = match frame.first().copied() {
                    Some(op) => op,
                    None => {
                        let _ = server.send(&[STATUS_MALFORMED], Wait::Blocking);
                        continue;
                    }
                };
                let (status, response_handoff_id) = match op {
                    OP_SET_FRAMEBUFFER_VMO => {
                        let _ = debug_println("gpud: recv OP_SET_FRAMEBUFFER_VMO");
                        let handoff_id =
                            decode_handoff_id_attach(frame).unwrap_or(active_handoff_id);
                        match moved_cap.take() {
                            Some(cap) => match backend.attach_external_framebuffer(
                                cap.slot(),
                                DISPLAY_WIDTH,
                                RESOURCE_HEIGHT,
                            ) {
                                Ok(()) => {
                                    active_handoff_id = handoff_id;
                                    let _ = backend.move_cursor(0, 0);
                                    let _ = debug_println("gpud: handoff attach ack");
                                    let _ = debug_println(GPUD_CURSOR_ON);
                                    let _ = debug_println(GPUD_DISPLAY_READY);
                                    (STATUS_OK, Some(active_handoff_id))
                                }
                                Err(e) => {
                                    let _ = debug_println("gpud: ERROR attach framebuffer failed");
                                    let _ = debug_println(
                                        "gpud: ERROR attach framebuffer resource create failed",
                                    );
                                    let _ = e;
                                    (STATUS_DEVICE_ERROR, Some(handoff_id))
                                }
                            },
                            None => {
                                let _ = debug_println("gpud: ERROR no cap in VMO message");
                                (STATUS_MALFORMED, Some(handoff_id))
                            }
                        }
                    }
                    OP_PRESENT_DAMAGE => {
                        // Phase 6c: carries a serialized CommittedBuffer with batched
                        // BlitSurface commands describing all damage regions.
                        let handoff_id =
                            decode_handoff_id_present(frame).unwrap_or(active_handoff_id);
                        let trace = !chain_trace_done;
                        if trace {
                            let _ = debug_println(crate::markers::GPUD_CHAIN_RECV);
                        }
                        let status = if frame.len() > 1 {
                            // Reuse scene_cb (reload_from) — no per-frame heap alloc.
                            match scene_cb.reload_from(&frame[1..]) {
                                Ok(_) => {
                                    if trace {
                                        let _ = debug_println(crate::markers::GPUD_CHAIN_PARSE_OK);
                                    }
                                    let damage_rect = damage_rect_from_cb(&scene_cb);
                                    // Lift the save-under cursor so scene blits land on
                                    // a cursor-free plane, present, then re-apply it on
                                    // top so the pointer always stays visible.
                                    let t0 = nsec().unwrap_or(0);
                                    backend.cursor_before_present();
                                    // present_committed's result was previously discarded;
                                    // capture it so a failed composite is no longer silent.
                                    match backend.present_committed(&scene_cb) {
                                        Ok(_) => {
                                            if trace {
                                                let _ =
                                                    debug_println(crate::markers::GPUD_CHAIN_EXEC_OK);
                                            }
                                        }
                                        Err(e) => {
                                            let _ =
                                                debug_println(crate::markers::GPUD_CHAIN_EXEC_FAIL);
                                            let _ = debug_println(gfx_error_label(e));
                                        }
                                    }
                                    let st = present_scanout_damage(&mut backend, damage_rect);
                                    backend.cursor_after_present();
                                    if trace {
                                        if st == STATUS_OK {
                                            let _ = debug_println(
                                                crate::markers::GPUD_CHAIN_SCANOUT_OK,
                                            );
                                            // Whole chain reached the end: stop tracing.
                                            chain_trace_done = true;
                                        } else {
                                            let _ = debug_println(
                                                crate::markers::GPUD_CHAIN_SCANOUT_FAIL,
                                            );
                                        }
                                    }
                                    let dt = nsec().unwrap_or(t0).saturating_sub(t0);
                                    present_ns_sum += dt;
                                    present_ns_max = present_ns_max.max(dt);
                                    present_count += 1;
                                    if present_count >= PRESENT_STATS_WINDOW {
                                        emit_present_stats(
                                            (present_ns_sum / present_count as u64 / 1000) as u32,
                                            (present_ns_max / 1000) as u32,
                                            present_count,
                                        );
                                        present_count = 0;
                                        present_ns_sum = 0;
                                        present_ns_max = 0;
                                    }
                                    total_presents += 1;
                                    // NOTE: the debug thumbnail did
                                    // `transfer_from_host(GL_SCANOUT_RES)` (reading the
                                    // scanout texture back) which desyncs QEMU's GL
                                    // present → black display. It must NOT run in the
                                    // live present path. Kept off; re-enable only for
                                    // offline RT inspection that doesn't also present.
                                    let _ = total_presents;
                                    st
                                }
                                Err(_) => {
                                    if trace {
                                        let _ =
                                            debug_println(crate::markers::GPUD_CHAIN_PARSE_FAIL);
                                    }
                                    // Only fall back to legacy damage-rect format when the
                                    // frame is exactly 17 bytes (opcode + 16-byte rect).
                                    if frame.len() == 17 {
                                        handle_present_damage(&mut backend, frame)
                                    } else {
                                        STATUS_MALFORMED
                                    }
                                }
                            }
                        } else {
                            handle_present_damage(&mut backend, frame)
                        };
                        if status == STATUS_OK {
                            active_handoff_id = handoff_id;
                        }
                        (status, Some(handoff_id))
                    }
                    OP_UPLOAD_CURSOR => {
                        let _ = debug_println("gpud: recv OP_UPLOAD_CURSOR");
                        // Frame: [op, w(4), h(4), hot_x(4), hot_y(4), bgra]. The reply's
                        // u32 payload reports the active cursor path: 1 = hardware
                        // overlay (cursor queue), 0 = software BlendCursor fallback.
                        if frame.len() < 17 {
                            (STATUS_MALFORMED, None)
                        } else {
                            let w = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                            let h = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
                            let hot_x =
                                u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
                            let hot_y =
                                u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                            let bgra = &frame[17..];
                            // Store the sprite for the scene-CB BlendCursor path.
                            // windowd composites the cursor into the per-present CB
                            // (reactive: one present per move, no racing). The gpud
                            // save-under path raced windowd's presents (flicker) and
                            // flushed per move (UART/loop storm), so it is disabled
                            // — reply SW so windowd keeps the BlendCursor path.
                            // (`hot_x`/`hot_y` are applied by windowd's BlendCursor.)
                            let _ = (hot_x, hot_y);
                            match backend.store_cursor_sprite(bgra, w, h) {
                                Ok(()) => {
                                    let _ = debug_println("gpud: cursor uploaded");
                                    (STATUS_OK, Some(CURSOR_REPLY_SW))
                                }
                                Err(_) => (STATUS_DEVICE_ERROR, None),
                            }
                        }
                    }
                    _ => (handle_frame(&mut backend, frame), None),
                };
                drop(moved_cap);
                if let Some(handoff_id) = response_handoff_id {
                    let mut response = [0u8; 5];
                    response[0] = status;
                    response[1..5].copy_from_slice(&handoff_id.to_le_bytes());
                    let _ = server.send(&response, Wait::Blocking);
                } else {
                    let response = [status];
                    let _ = server.send(&response, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                // WouldBlock/Timeout are unexpected in Blocking mode; yield and retry.
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint))
            | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)) => {
                // Route disappeared — yield and wait for re-registration.
                let _ = yield_();
            }
            Err(_) => return Err(nexus_abi::AbiError::InvalidArgument),
        }
    }
}

/// Emit `gpud: present us avg=A max=M n=N` without heap allocation (gpud's
/// bump allocator never frees). 120Hz budget = 8333us; this surfaces the
/// per-present compositor cost so glass/layer optimisations can be measured.
/// Human-readable reason for a present-chain hop failure (G3 exec). Static
/// strings only — no alloc on gpud's bump heap.
fn gfx_error_label(e: GfxError) -> &'static str {
    match e {
        GfxError::DeviceNotFound => "gpud: chain reason: device not found",
        GfxError::MmioFault => "gpud: chain reason: mmio fault",
        GfxError::CommandRejected => "gpud: chain reason: command rejected",
        GfxError::ResourceExhausted => "gpud: chain reason: resource exhausted (bump heap?)",
        GfxError::Unsupported => "gpud: chain reason: unsupported command",
        GfxError::InvalidArgument => "gpud: chain reason: invalid argument",
    }
}

fn emit_present_stats(avg_us: u32, max_us: u32, n: u32) {
    let mut buf = [0u8; 64];
    let mut p = 0usize;
    let put = |buf: &mut [u8; 64], p: &mut usize, s: &[u8]| {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    };
    let put_dec = |buf: &mut [u8; 64], p: &mut usize, mut v: u32| {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            if *p < buf.len() {
                buf[*p] = tmp[n];
                *p += 1;
            }
        }
    };
    put(&mut buf, &mut p, b"gpud: present us avg=");
    put_dec(&mut buf, &mut p, avg_us);
    put(&mut buf, &mut p, b" max=");
    put_dec(&mut buf, &mut p, max_us);
    put(&mut buf, &mut p, b" n=");
    put_dec(&mut buf, &mut p, n);
    put(&mut buf, &mut p, b"\n");
    let _ = debug_write(&buf[..p]);
}

fn decode_handoff_id_attach(frame: &[u8]) -> Option<u32> {
    if frame.len() < 5 {
        return None;
    }
    Some(u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

fn decode_handoff_id_present(frame: &[u8]) -> Option<u32> {
    if frame.len() < 21 {
        return None;
    }
    Some(u32::from_le_bytes([frame[17], frame[18], frame[19], frame[20]]))
}

/// Extract bounding damage rect from ALL command types.
fn damage_rect_from_cb(cb: &CommittedBuffer) -> Rect {
    let mut min_x = DISPLAY_WIDTH;
    let mut min_y = DISPLAY_HEIGHT;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for cmd in cb.commands() {
        let (x, y, w, h) = match cmd {
            Command::BlitSurface { dst_x, dst_y, width, height, .. } => {
                (*dst_x, *dst_y, *width, *height)
            }
            // Absolute blits that target the display plane (e.g. the chat layer
            // composite, sidebar/button blur-cache restores) MUST contribute to
            // the present damage, or their region is written to the backing but
            // never transferred/flushed to the host. Convert the absolute dst row
            // back to screen-relative; ignore blits aimed elsewhere (atlas/cache).
            Command::BlitAbsolute { dst_x, dst_y_abs, width, height, .. } => {
                if *dst_y_abs >= DISPLAY_PLANE_ROW
                    && *dst_y_abs < DISPLAY_PLANE_ROW + DISPLAY_HEIGHT
                {
                    (*dst_x, dst_y_abs - DISPLAY_PLANE_ROW, *width, *height)
                } else {
                    continue;
                }
            }
            Command::FillSdfRoundedRect { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::FillSdfGradient { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::CompositeLayer {
                width,
                height,
                dst_x,
                dst_y,
                shadow_blur,
                shadow_offset_y,
                ..
            } => {
                // Damage the layer rect plus its shadow halo (blur + offset).
                let pad = *shadow_blur + shadow_offset_y.unsigned_abs();
                let x0 = dst_x.saturating_sub(pad);
                let y0 = dst_y.saturating_sub(pad);
                let x1 = (dst_x + width).saturating_add(pad);
                let y1 = (dst_y + height).saturating_add(pad);
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::DropShadow { rect, blur, offset_x, offset_y, .. } => {
                // The painted halo extends past the casting rect by blur,
                // shifted by the offset — damage the full extent (clamped).
                let pad = *blur as i32;
                let x0 = (rect.x as i32 + offset_x - pad).max(0) as u32;
                let y0 = (rect.y as i32 + offset_y - pad).max(0) as u32;
                let x1 = ((rect.x + rect.width) as i32 + offset_x + pad).max(0) as u32;
                let y1 = ((rect.y + rect.height) as i32 + offset_y + pad).max(0) as u32;
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::BlurBackdrop { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::BlendCursor { x, y, width, height } => (*x, *y, *width, *height),
            _ => continue,
        };
        let ex = x.saturating_add(w);
        let ey = y.saturating_add(h);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(ex);
        max_y = max_y.max(ey);
        found = true;
    }
    if found {
        // Clamp to the display plane — halo-style commands (DropShadow) may
        // extend past the screen edges.
        let min_x = min_x.min(DISPLAY_WIDTH);
        let min_y = min_y.min(DISPLAY_HEIGHT);
        let max_x = max_x.min(DISPLAY_WIDTH);
        let max_y = max_y.min(DISPLAY_HEIGHT);
        Rect {
            x: min_x,
            y: min_y,
            width: max_x.saturating_sub(min_x).max(1),
            height: max_y.saturating_sub(min_y).max(1),
        }
    } else {
        Rect { x: 0, y: 0, width: DISPLAY_WIDTH, height: DISPLAY_HEIGHT }
    }
}

fn present_scanout_damage(backend: &mut VirtioGpuBackend, rect: Rect) -> u8 {
    match backend.present_scanout_damage(rect) {
        Ok(()) => STATUS_OK,
        Err(e) => {
            let _ = debug_println("gpud: present scanout damage FAIL");
            match e {
                GfxError::InvalidArgument => {
                    let _ = debug_println("gpud: scanout InvalidArgument (no scanout resource?)");
                }
                GfxError::ResourceExhausted => {
                    let _ = debug_println("gpud: scanout ResourceExhausted");
                }
                _ => {}
            }
            STATUS_DEVICE_ERROR
        }
    }
}

fn handle_present_damage(backend: &mut VirtioGpuBackend, frame: &[u8]) -> u8 {
    if frame.len() < 17 {
        return STATUS_MALFORMED;
    }
    let x = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
    let y = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
    let width = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
    let height = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
    present_scanout_damage(backend, Rect { x, y, width, height })
}

fn handle_frame(backend: &mut VirtioGpuBackend, frame: &[u8]) -> u8 {
    let Some(op) = frame.first().copied() else {
        return STATUS_MALFORMED;
    };
    match op {
        OP_SUBMIT_ANIMATION_FRAME => {
            // Animation frames carry a serialized CommittedBuffer after the opcode.
            // Deserialize and submit to the GPU backend for execution.
            if frame.len() <= 1 {
                return STATUS_MALFORMED;
            }
            match CommittedBuffer::deserialize_from(&frame[1..]) {
                Ok((cmd, _consumed)) => {
                    let _ = backend.submit(cmd);
                    STATUS_OK
                }
                Err(_) => STATUS_MALFORMED,
            }
        }
        OP_MOVE_CURSOR => {
            if frame.len() < 9 {
                return STATUS_MALFORMED;
            }
            let x = i32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let y = i32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            // Record the pointer position for the GL-scanout fallback cursor (the
            // Stage-4 build-up draws a procedural arrow at cursor_ox/oy each present
            // — no transfer_to_host, so it is safe on the virgl GL scanout, unlike
            // the hardware-cursor overlay whose resource transfer blanks the GL
            // present). windowd also sends OP_PRESENT_DAMAGE on move, re-rendering.
            backend.set_pointer_pos(x, y);
            // Legacy save-under SW path (no-op while cursor ownership is unclaimed).
            if backend.cursor_move(x, y).is_err() {
                return STATUS_DEVICE_ERROR;
            }
            STATUS_OK
        }
        OP_PRESENT_DAMAGE => handle_present_damage(backend, frame),
        _ => STATUS_MALFORMED,
    }
}
