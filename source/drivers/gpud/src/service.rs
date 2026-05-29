// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite gpud service entry for the QEMU virtio-gpu proof path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_gfx::backend::traits::GfxBackend;
use nexus_abi::{debug_println, mmio_map, yield_, AbiError};
use nexus_gfx::PixelFormat;
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::backend::VirtioGpuBackend;
use crate::markers::{
    GPUD_CURSOR_ON, GPUD_MMIO_FAULT, GPUD_NO_DEVICE, GPUD_READY, GPUD_SCANOUT_OK,
    GPUD_VIRTIO_GPU_PROBED,
};

pub const ROUTE_NAME: &str = "gpud";
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
pub const OP_MOVE_CURSOR: u8 = 2;
pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_DEVICE_ERROR: u8 = 2;

const GPU_MMIO_CAP_SLOT: u32 = 48;
const GPU_MMIO_VA: usize = 0x2020_0000;
const GPU_MMIO_LEN: usize = 0x1000;
const PROOF_RESOURCE_W: u32 = 64;
const PROOF_RESOURCE_H: u32 = 64;

pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let mut backend = open_backend_blocking()?;
    let proof_resource = backend
        .create_resource(PROOF_RESOURCE_W, PROOF_RESOURCE_H, PixelFormat::Bgra8888)
        .map_err(|_| {
            let _ = debug_println(GPUD_MMIO_FAULT);
            nexus_abi::AbiError::InvalidArgument
        })?;
    backend
        .transfer_to_host(
            proof_resource,
            nexus_gfx::backend::types::Rect {
                x: 0,
                y: 0,
                width: PROOF_RESOURCE_W,
                height: PROOF_RESOURCE_H,
            },
        )
        .map_err(|_| {
            let _ = debug_println(GPUD_MMIO_FAULT);
            nexus_abi::AbiError::InvalidArgument
        })?;
    backend.set_scanout(proof_resource).map_err(|_| {
        let _ = debug_println(GPUD_MMIO_FAULT);
        nexus_abi::AbiError::InvalidArgument
    })?;
    debug_println(GPUD_SCANOUT_OK)?;
    backend.move_cursor(0, 0).map_err(|_| {
        let _ = debug_println(GPUD_MMIO_FAULT);
        nexus_abi::AbiError::InvalidArgument
    })?;
    debug_println(GPUD_CURSOR_ON)?;
    debug_println(GPUD_READY)?;

    let server = bind_server();
    service_requests(server, backend, proof_resource)
}

fn open_backend_blocking() -> Result<VirtioGpuBackend, nexus_abi::AbiError> {
    loop {
        match open_backend_once() {
            Ok(backend) => return Ok(backend),
            Err(AbiError::InvalidArgument) => {
                let _ = yield_();
            }
            Err(err) => return Err(err),
        }
    }
}

fn open_backend_once() -> Result<VirtioGpuBackend, nexus_abi::AbiError> {
    match mmio_map(GPU_MMIO_CAP_SLOT, GPU_MMIO_VA, 0) {
        Ok(()) | Err(AbiError::InvalidArgument) => {}
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

fn bind_server() -> KernelServer {
    loop {
        if let Ok(server) = KernelServer::new_for(ROUTE_NAME) {
            return server;
        }
        let _ = yield_();
    }
}

fn service_requests(
    server: KernelServer,
    mut backend: VirtioGpuBackend,
    proof_resource: nexus_gfx::backend::types::ResourceId,
) -> Result<(), nexus_abi::AbiError> {
    loop {
        match server.recv_request_with_meta(Wait::NonBlocking) {
            Ok((frame, _sender_service_id, reply)) => {
                let status = handle_frame(&mut backend, proof_resource, frame.as_slice());
                let response = [status];
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
                } else {
                    let _ = server.send(&response, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(_) => return Err(nexus_abi::AbiError::InvalidArgument),
        }
    }
}

fn handle_frame(
    backend: &mut VirtioGpuBackend,
    proof_resource: nexus_gfx::backend::types::ResourceId,
    frame: &[u8],
) -> u8 {
    let Some(op) = frame.first().copied() else {
        return STATUS_MALFORMED;
    };
    match op {
        OP_SUBMIT_ANIMATION_FRAME => {
            let transfer = backend.transfer_to_host(
                proof_resource,
                nexus_gfx::backend::types::Rect {
                    x: 0,
                    y: 0,
                    width: PROOF_RESOURCE_W,
                    height: PROOF_RESOURCE_H,
                },
            );
            if transfer.is_err() || backend.set_scanout(proof_resource).is_err() {
                return STATUS_DEVICE_ERROR;
            }
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
        _ => STATUS_MALFORMED,
    }
}
