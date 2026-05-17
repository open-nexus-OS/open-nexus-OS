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
    decode_status, decode_visible_state, encode_get_visible_state, encode_send_composed_frame_vmo,
    encode_status, encode_visible_state_frame, frame_has_op, VisibleState, OP_GET_VISIBLE_STATE,
    OP_SEND_COMPOSED_FRAME_VMO, STATUS_OK, STATUS_UNSUPPORTED,
};
use nexus_abi::{cap_clone, debug_println, nsec, yield_};
use nexus_ipc::{Client as _, IpcError, KernelClient, KernelServer, Server as _, Wait};

use crate::backend::framebuffer::FramebufferOwner;
use crate::backend::ramfb::{configure_ramfb, display_bootstrap_requested};
use crate::error::{classify_service_recv_error, FbdevdError, ServiceRecvAction, ServiceRecvErrorClass};
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

    // Allocate framebuffer VMO (fbdevd is the scanout owner)
    let mode =
        windowd::VisibleBootstrapMode::fixed().map_err(|_| fail(FbdevdError::InvalidMode))?;
    let framebuffer = FramebufferOwner::allocate(mode).map_err(|err| fail(err))?;
    debug_println(READY_MARKER).map_err(|_| "fbdevd ready log failed")?;
    debug_println(MAP_OK_MARKER).map_err(|_| "fbdevd map log failed")?;

    configure_ramfb(framebuffer.base, mode).map_err(|err| fail(err))?;
    debug_println(RAMFB_CONFIGURED_MARKER).map_err(|_| "fbdevd ramfb log failed")?;

    // fbdevd is now scanout-only. windowd composes and writes frames into our VMO.
    // We handle observer queries and telemetry.
    let mut service = FbdevService::disabled();
    service.set_display_enabled(true);
    let mut reactor = DisplayReactor::new(windowd::VISIBLE_BOOTSTRAP_HZ);
    let mut windowd_frame_client = KernelClient::new_for("windowd").ok();
    let mut windowd_obs_client = KernelClient::new_for("windowd").ok();
    let mut windowd_obs_reply = KernelClient::new_for("@reply").ok();
    let mut framebuffer_registered = false;
    let mut flush_ok_emitted = false;
    let mut cursor_overlay_emitted = false;

    loop {
        service_requests(&server, service.visible_state())?;
        if !framebuffer_registered {
            framebuffer_registered = register_framebuffer_with_windowd(
                &mut windowd_frame_client,
                framebuffer.handle as u32,
            );
            if framebuffer_registered && !flush_ok_emitted {
                debug_println(FLUSH_OK_MARKER).map_err(|_| "fbdevd flush log failed")?;
                flush_ok_emitted = true;
            }
        }
        let now_ns = nsec().unwrap_or(0);
        let mut budget = TickBudget::new(4);
        if reactor.should_present(now_ns, &mut budget) {
            // Query windowd for visible state (observer-only, no composition)
            let input_state = match (&windowd_obs_client, &windowd_obs_reply) {
                (Some(client), Some(reply)) => fetch_visible_state_cached(client, reply),
                _ => {
                    windowd_obs_client = KernelClient::new_for("windowd").ok();
                    windowd_obs_reply = KernelClient::new_for("@reply").ok();
                    None
                }
            };
            if let Some(input_state) = input_state {
                let previous_state = service.render_state();
                service.merge_input_state(input_state);
                let next_state = service.render_state();
                if next_state.cursor_overlay_visible && !cursor_overlay_emitted {
                    let _ = debug_println(crate::markers::CURSOR_OVERLAY_ON_MARKER);
                    cursor_overlay_emitted = true;
                }
                // Track dirty row count for telemetry
                match live_dirty_rows(previous_state, next_state, mode) {
                    DirtyRows::None => {}
                    DirtyRows::Range { start_y, end_y } => {
                        let byte_len = (end_y - start_y) as usize * mode.stride as usize;
                        if byte_len != 0 {
                            service.present_live_bytes(byte_len).map_err(|err| fail(err))?;
                        }
                    }
                    DirtyRows::Full => {
                        let byte_len =
                            mode.byte_len().map_err(|_| fail(FbdevdError::InvalidMode))?;
                        service.present_live_bytes(byte_len).map_err(|err| fail(err))?;
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

fn service_requests(server: &KernelServer, state: VisibleState) -> Result<(), &'static str> {
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
            Err(err) => match classify_service_recv_error(map_service_recv_error_class(err)) {
                ServiceRecvAction::ReturnOk => return Ok(()),
                ServiceRecvAction::ReturnOkWithBackpressureLog => return Ok(()),
                ServiceRecvAction::Fatal => {
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
            },
        }
    }
}

fn map_service_recv_error_class(err: IpcError) -> ServiceRecvErrorClass {
    match err {
        IpcError::WouldBlock | IpcError::Timeout => ServiceRecvErrorClass::Idle,
        IpcError::Disconnected | IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint) => {
            ServiceRecvErrorClass::PeerClosed
        }
        IpcError::NoSpace => ServiceRecvErrorClass::Backpressure,
        _ => ServiceRecvErrorClass::Fatal,
    }
}

fn fetch_visible_state_cached(client: &KernelClient, reply: &KernelClient) -> Option<VisibleState> {
    const RPC_TIMEOUT_MS: u64 = 2;
    let send_wait = Wait::Timeout(Duration::from_millis(RPC_TIMEOUT_MS));
    let (reply_send_slot, _) = reply.slots();
    let reply_send_clone = cap_clone(reply_send_slot).ok()?;
    let request = encode_get_visible_state();
    client.send_with_cap_move_wait(&request, reply_send_clone, send_wait).ok()?;
    let recv_wait = Wait::NonBlocking;
    let frame = reply.recv(recv_wait).ok()?;
    decode_visible_state(&frame)
}

fn register_framebuffer_with_windowd(
    client: &mut Option<KernelClient>,
    framebuffer_handle: u32,
) -> bool {
    if client.is_none() {
        *client = KernelClient::new_for("windowd").ok();
    }
    let Some(windowd) = client.as_ref() else {
        return false;
    };
    let Ok(clone) = cap_clone(framebuffer_handle) else {
        return false;
    };
    let request = encode_send_composed_frame_vmo();
    if windowd
        .send_with_cap_move_wait(&request, clone, Wait::Timeout(Duration::from_millis(10)))
        .is_err()
    {
        *client = None;
        return false;
    }
    match windowd.recv(Wait::Timeout(Duration::from_millis(500))) {
        Ok(frame) if decode_status(&frame, OP_SEND_COMPOSED_FRAME_VMO) == Some(STATUS_OK) => true,
        _ => {
            *client = None;
            false
        }
    }
}

fn fail(err: FbdevdError) -> &'static str {
    let _ = debug_println(err.label());
    err.label()
}

fn log_and_fail(label: &'static str) -> &'static str {
    let _ = debug_println(label);
    label
}

fn ipc_error_kind(err: IpcError) -> &'static str {
    match err {
        IpcError::WouldBlock => "would-block",
        IpcError::Timeout => "timeout",
        IpcError::Disconnected => "disconnected",
        IpcError::NoSpace => "no-space",
        IpcError::Unsupported => "unsupported",
        IpcError::Kernel(_) => "kernel",
        _ => "ipc",
    }
}

fn ipc_error_detail(err: IpcError) -> &'static str {
    match err {
        IpcError::Kernel(inner) => match inner {
            nexus_abi::IpcError::NoSuchEndpoint => "no-such-endpoint",
            nexus_abi::IpcError::QueueFull => "queue-full",
            nexus_abi::IpcError::QueueEmpty => "queue-empty",
            nexus_abi::IpcError::PermissionDenied => "permission-denied",
            nexus_abi::IpcError::TimedOut => "timed-out",
            nexus_abi::IpcError::NoSpace => "no-space",
            nexus_abi::IpcError::Unsupported => "unsupported",
        },
        _ => "none",
    }
}
