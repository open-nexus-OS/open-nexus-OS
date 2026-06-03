// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite gpud service entry for the QEMU virtio-gpu proof path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_abi::{debug_println, mmio_map, yield_, AbiError};
use nexus_ipc::{KernelServer, Server as _, Wait};

use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::Rect;

use crate::backend::VirtioGpuBackend;
use crate::markers::{
    GPUD_CURSOR_ON, GPUD_DISPLAY_READY, GPUD_MMIO_FAULT, GPUD_NO_DEVICE, GPUD_READY,
    GPUD_VIRTIO_GPU_PROBED,
};

pub const ROUTE_NAME: &str = "gpud";
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
pub const OP_MOVE_CURSOR: u8 = 2;
pub const OP_SET_FRAMEBUFFER_VMO: u8 = 3;
pub const OP_PRESENT_DAMAGE: u8 = 4;
pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_DEVICE_ERROR: u8 = 2;

const GPU_MMIO_CAP_SLOT: u32 = 48;
const GPU_MMIO_VA: usize = 0x2020_0000;
const GPU_MMIO_LEN: usize = 0x1000;
const GPUD_RECV_SLOT: u32 = 6;
const GPUD_SEND_SLOT: u32 = 7;
/// Display framebuffer dimensions matching windowd's VISIBLE_BOOTSTRAP_WIDTH/HEIGHT.
/// On QEMU virtio-gpu with `-display gtk`, the GTK window resizes to match this scanout.
const DISPLAY_WIDTH: u32 = 1280;
const DISPLAY_HEIGHT: u32 = 800;
pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let backend = open_backend_blocking()?;

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

fn open_backend_blocking() -> Result<VirtioGpuBackend, nexus_abi::AbiError> {
    const MAX_RETRIES: u32 = 64;
    let mut attempt: u32 = 0;
    loop {
        match open_backend_once() {
            Ok(backend) => return Ok(backend),
            Err(AbiError::InvalidArgument) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    let _ = debug_println("gpud: mmio fatal timeout");
                    return Err(AbiError::InvalidArgument);
                }
                // Emit diagnostic every 8 retries so the UART shows progress.
                if attempt & 7 == 0 {
                    let _ = debug_println("gpud: mmio retry");
                }
                let _ = yield_();
            }
            Err(err) => return Err(err),
        }
    }
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
    KernelServer::new_with_slots(GPUD_RECV_SLOT, GPUD_SEND_SLOT)
        .map_err(|_| nexus_abi::AbiError::InvalidArgument)
}

fn service_requests(
    server: KernelServer,
    mut backend: VirtioGpuBackend,
) -> Result<(), nexus_abi::AbiError> {
    let mut recv_frame = [0u8; 128];
    loop {
        match server.recv_request_with_meta_into(Wait::NonBlocking, &mut recv_frame) {
            Ok((frame_len, _sid, mut moved_cap)) => {
                let frame = &recv_frame[..frame_len];
                let op = match frame.first().copied() {
                    Some(op) => op,
                    None => {
                        let _ = server.send(&[STATUS_MALFORMED], Wait::Blocking);
                        continue;
                    }
                };
                let status = match op {
                    OP_SET_FRAMEBUFFER_VMO => {
                        let _ = debug_println("gpud: recv OP_SET_FRAMEBUFFER_VMO");
                        match moved_cap.take() {
                        Some(cap) => match backend.attach_external_framebuffer(
                            cap.slot(),
                            DISPLAY_WIDTH,
                            DISPLAY_HEIGHT,
                        ) {
                            Ok(()) => {
                                let _ = backend.move_cursor(0, 0);
                                let _ = debug_println(GPUD_CURSOR_ON);
                                let _ = debug_println(GPUD_DISPLAY_READY);
                                STATUS_OK
                            }
                            Err(e) => {
                                let _ = debug_println("gpud: ERROR attach framebuffer failed");
                                let _ = debug_println("gpud: ERROR attach framebuffer resource create failed");
                                let _ = e;
                                STATUS_DEVICE_ERROR
                            }
                        },
                        None => {
                            let _ = debug_println("gpud: ERROR no cap in VMO message");
                            STATUS_MALFORMED
                        }
                        }
                    },
                    _ => handle_frame(&mut backend, frame),
                };
                let response = [status];
                drop(moved_cap);
                let _ = server.send(&response, Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock)
            | Err(nexus_ipc::IpcError::Timeout)
            | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint))
            | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)) => {
                let _ = yield_();
            }
            Err(_) => return Err(nexus_abi::AbiError::InvalidArgument),
        }
    }
}

fn handle_frame(backend: &mut VirtioGpuBackend, frame: &[u8]) -> u8 {
    let Some(op) = frame.first().copied() else {
        return STATUS_MALFORMED;
    };
    match op {
        OP_SUBMIT_ANIMATION_FRAME => {
            // Animation frames are validated GPU command buffers.
            // The scanout framebuffer is managed separately via
            // OP_SET_FRAMEBUFFER_VMO (zero-copy VMO from windowd).
            let _ = backend;
            STATUS_OK
        }
        OP_MOVE_CURSOR => {
            if frame.len() < 9 {
                return STATUS_MALFORMED;
            }
            let x = i32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let y = i32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            if backend.move_cursor(x, y).is_err() {
                return STATUS_DEVICE_ERROR;
            }
            STATUS_OK
        }
        OP_PRESENT_DAMAGE => {
            if frame.len() < 17 {
                return STATUS_MALFORMED;
            }
            let x = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let y = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            let width = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
            let height = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
            if backend.present_scanout_damage(Rect { x, y, width, height }).is_err() {
                return STATUS_DEVICE_ERROR;
            }
            STATUS_OK
        }
        _ => STATUS_MALFORMED,
    }
}
