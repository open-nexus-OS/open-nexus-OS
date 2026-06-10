// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite gpud service entry for the QEMU virtio-gpu proof path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_abi::{debug_println, mmio_map, yield_, AbiError};
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
const RESOURCE_HEIGHT: u32 = 3200; // 4-plane VMO: wallpaper / retained-scene / slot-A / slot-B
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
    KernelServer::new_with_slots(GPUD_RECV_SLOT, GPUD_SEND_SLOT).map_err(|_| nexus_abi::AbiError::InvalidArgument)
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
                        let handoff_id = decode_handoff_id_attach(frame).unwrap_or(active_handoff_id);
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
                                    let _ =
                                        debug_println("gpud: ERROR attach framebuffer resource create failed");
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
                        let status = if frame.len() > 1 {
                            // Reuse scene_cb (reload_from) — no per-frame heap alloc.
                            match scene_cb.reload_from(&frame[1..]) {
                                Ok(_) => {
                                    let damage_rect = damage_rect_from_cb(&scene_cb);
                                    let _ = backend.present_committed(&scene_cb);
                                    present_scanout_damage(&mut backend, damage_rect)
                                }
                                Err(_) => {
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
                        if frame.len() < 9 {
                            (STATUS_MALFORMED, None)
                        } else {
                            let w = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                            let h = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
                            let bgra = &frame[9..];
                            // Store as a software sprite for BlendCursor (no hardware
                            // cursor resource → avoids the QEMU UPDATE_CURSOR quirk).
                            match backend.store_cursor_sprite(bgra, w, h) {
                                Ok(()) => {
                                    let _ = debug_println("gpud: cursor uploaded");
                                    (STATUS_OK, None)
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
            Err(nexus_ipc::IpcError::WouldBlock)
            | Err(nexus_ipc::IpcError::Timeout) => {
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
            Command::BlitSurface { dst_x, dst_y, width, height, .. } => (*dst_x, *dst_y, *width, *height),
            Command::FillSdfRoundedRect { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
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
        Rect { x: min_x, y: min_y, width: max_x.saturating_sub(min_x).max(1), height: max_y.saturating_sub(min_y).max(1) }
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
            if backend.move_hw_cursor(x as u32, y as u32).is_err() {
                return STATUS_DEVICE_ERROR;
            }
            STATUS_OK
        }
        OP_PRESENT_DAMAGE => handle_present_damage(backend, frame),
        _ => STATUS_MALFORMED,
    }
}