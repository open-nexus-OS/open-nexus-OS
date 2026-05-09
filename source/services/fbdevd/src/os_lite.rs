// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `fbdevd` runtime for service-owned visible scanout.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by visible-bootstrap QEMU proofs plus host `fbdevd` tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

extern crate alloc;

use alloc::format;
use core::time::Duration;
use input_live_protocol::{
    decode_visible_state, encode_get_visible_state, encode_status, encode_visible_state_frame,
    frame_has_op, OP_GET_VISIBLE_STATE, STATUS_UNSUPPORTED,
};
use nexus_abi::{cap_clone, debug_println, nsec, yield_};
use nexus_ipc::{Client as _, IpcError, KernelClient, KernelServer, Server as _, Wait};

use crate::backend::framebuffer::FramebufferOwner;
use crate::backend::ramfb::{configure_ramfb, display_bootstrap_requested};
use crate::error::FbdevdError;
use crate::markers::{FLUSH_OK_MARKER, MAP_OK_MARKER, RAMFB_CONFIGURED_MARKER, READY_MARKER};
use crate::protocol::ROUTE_NAME;
use crate::reactor::{live_dirty_rows, DirtyRows, DisplayReactor, TickBudget};
use crate::service::FbdevService;

pub fn service_main_loop() -> Result<(), &'static str> {
    let server = KernelServer::new_for(ROUTE_NAME)
        .or_else(|_| KernelServer::new_with_slots(3, 4))
        .map_err(|_| "fbdevd: init fail kernel-server")?;
    if !display_bootstrap_requested() {
        let service = FbdevService::disabled();
        loop {
            service_requests(&server, service.visible_state())?;
            let _ = yield_();
        }
    }

    let bootstrap =
        windowd::bootstrap_display_handoff().map_err(|_| fail(FbdevdError::InvalidMode))?;
    let framebuffer = FramebufferOwner::allocate(bootstrap.mode).map_err(|err| fail(err))?;
    debug_println(READY_MARKER).map_err(|_| "fbdevd ready log failed")?;
    debug_println(MAP_OK_MARKER).map_err(|_| "fbdevd map log failed")?;
    configure_ramfb(framebuffer.base, bootstrap.mode).map_err(|err| fail(err))?;
    debug_println(RAMFB_CONFIGURED_MARKER).map_err(|_| "fbdevd ramfb log failed")?;
    framebuffer
        .write_handoff(&bootstrap)
        .map_err(|err| fail(err))?;
    debug_println(FLUSH_OK_MARKER).map_err(|_| "fbdevd flush log failed")?;

    let mut service = FbdevService::enabled(&bootstrap).map_err(|err| fail(err))?;
    let mut reactor = DisplayReactor::new(windowd::VISIBLE_BOOTSTRAP_HZ);
    loop {
        service_requests(&server, service.visible_state())?;
        let now_ns = nsec().unwrap_or(0);
        let mut budget = TickBudget::new(1);
        if reactor.should_present(now_ns, &mut budget) {
            if let Some(input_state) = fetch_input_visible_state() {
                let previous_state = service.visible_state();
                service.merge_input_state(input_state);
                let next_state = service.visible_state();
                match live_dirty_rows(previous_state, next_state, bootstrap.mode) {
                    DirtyRows::None => {}
                    DirtyRows::Range { start_y, end_y } => {
                        let byte_len = framebuffer
                            .write_live_visible_rows(next_state, start_y, end_y)
                            .map_err(|err| fail(err))?;
                        if byte_len != 0 {
                            service
                                .present_live_bytes(byte_len)
                                .map_err(|err| fail(err))?;
                        }
                    }
                    DirtyRows::Full => {
                        let handoff = windowd::live_visible_state_handoff(next_state)
                            .map_err(|_| fail(FbdevdError::InvalidMode))?;
                        service.present(&handoff).map_err(|err| fail(err))?;
                        framebuffer
                            .write_handoff(&handoff)
                            .map_err(|err| fail(err))?;
                    }
                }
            }
            if let Some((windowd_line, fbdevd_line)) = service.telemetry_if_due(now_ns) {
                if !windowd_line.is_empty() {
                    let _ = debug_println(&windowd_line);
                }
                if !fbdevd_line.is_empty() {
                    let _ = debug_println(&fbdevd_line);
                }
            }
        }
        let _ = yield_();
    }
}

fn service_requests(
    server: &KernelServer,
    state: input_live_protocol::VisibleState,
) -> Result<(), &'static str> {
    loop {
        match server.recv_request_with_meta(Wait::NonBlocking) {
            Ok((frame, _sender_service_id, reply)) => {
                if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                    let response = encode_visible_state_frame(state);
                    if let Some(reply) = reply {
                        reply
                            .reply_and_close_wait(&response, Wait::Blocking)
                            .map_err(|_| log_and_fail("fbdevd: reply visible-state failed"))?;
                    } else {
                        server
                            .send(&response, Wait::Blocking)
                            .map_err(|_| log_and_fail("fbdevd: send visible-state failed"))?;
                    }
                } else {
                    let op = frame.get(3).copied().unwrap_or(0);
                    let response = encode_status(op, STATUS_UNSUPPORTED);
                    if let Some(reply) = reply {
                        reply
                            .reply_and_close_wait(&response, Wait::Blocking)
                            .map_err(|_| log_and_fail("fbdevd: reply unsupported failed"))?;
                    } else {
                        server
                            .send(&response, Wait::Blocking)
                            .map_err(|_| log_and_fail("fbdevd: send unsupported failed"))?;
                    }
                }
            }
            Err(IpcError::WouldBlock)
            | Err(IpcError::Timeout)
            | Err(IpcError::Disconnected)
            | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => return Ok(()),
            Err(err) => {
                let (recv_slot, send_slot) = server.slots();
                let _ = debug_println(&format!(
                    "fbdevd: recv failed detail recv_slot={} send_slot={} kind={} detail={}",
                    recv_slot,
                    send_slot,
                    ipc_error_kind(err),
                    ipc_error_detail(err)
                ));
                return Err(log_and_fail("fbdevd: recv failed"));
            }
        }
    }
}

fn fetch_input_visible_state() -> Option<input_live_protocol::VisibleState> {
    const INPUT_VISIBLE_STATE_RPC_TIMEOUT_MS: u64 = 2;
    let wait = Wait::Timeout(Duration::from_millis(INPUT_VISIBLE_STATE_RPC_TIMEOUT_MS));
    let client = KernelClient::new_for("inputd").ok()?;
    let reply = KernelClient::new_for("@reply").ok()?;
    let (reply_send_slot, _) = reply.slots();
    let reply_send_clone = cap_clone(reply_send_slot).ok()?;
    let request = encode_get_visible_state();
    client
        .send_with_cap_move_wait(&request, reply_send_clone, wait)
        .ok()?;
    let frame = reply.recv(wait).ok()?;
    decode_visible_state(&frame)
}

fn fail(err: FbdevdError) -> &'static str {
    let _ = debug_println(err.label());
    err.label()
}

fn log_and_fail(label: &'static str) -> &'static str {
    let _ = debug_println(label);
    label
}

fn ipc_error_kind(err: nexus_ipc::IpcError) -> &'static str {
    match err {
        nexus_ipc::IpcError::WouldBlock => "would_block",
        nexus_ipc::IpcError::Timeout => "timeout",
        nexus_ipc::IpcError::Disconnected => "disconnected",
        nexus_ipc::IpcError::NoSpace => "no_space",
        nexus_ipc::IpcError::Kernel(_) => "kernel",
        nexus_ipc::IpcError::Unsupported => "unsupported",
        _ => "other",
    }
}

fn ipc_error_detail(err: nexus_ipc::IpcError) -> &'static str {
    match err {
        nexus_ipc::IpcError::Kernel(kernel) => match kernel {
            nexus_abi::IpcError::NoSuchEndpoint => "kernel_no_such_endpoint",
            nexus_abi::IpcError::QueueFull => "kernel_queue_full",
            nexus_abi::IpcError::QueueEmpty => "kernel_queue_empty",
            nexus_abi::IpcError::PermissionDenied => "kernel_permission_denied",
            nexus_abi::IpcError::TimedOut => "kernel_timed_out",
            nexus_abi::IpcError::NoSpace => "kernel_no_space",
            nexus_abi::IpcError::Unsupported => "kernel_unsupported",
        },
        _ => ipc_error_kind(err),
    }
}
