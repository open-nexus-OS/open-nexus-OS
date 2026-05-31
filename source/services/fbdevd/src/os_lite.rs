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
use core::fmt::Write as _;
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
use crate::error::{
    classify_service_recv_error, FbdevdError, ServiceRecvAction, ServiceRecvErrorClass,
};
use crate::markers::{FLUSH_OK_MARKER, MAP_OK_MARKER, RAMFB_CONFIGURED_MARKER, READY_MARKER};
use crate::protocol::ROUTE_NAME;
use crate::reactor::{live_dirty_rows, DirtyRows, DisplayReactor, TickBudget};
use crate::scanout::DisplayScanoutReport;
use crate::service::FbdevService;
use crate::splash;
use windowd::WindowdDisplayTelemetryReport;

const ROUTE_BIND_RETRIES: usize = 256;

struct FixedDebugLine {
    buf: [u8; 256],
    len: usize,
}

impl FixedDebugLine {
    const fn new() -> Self {
        Self { buf: [0; 256], len: 0 }
    }

    fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.buf[..self.len]).ok()
    }
}

impl core::fmt::Write for FixedDebugLine {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.saturating_add(s.len());
        if end > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub fn service_main_loop() -> Result<(), &'static str> {
    let _ = debug_println("fbdevd: entry");
    let server = bind_server()?;
    debug_println(READY_MARKER).map_err(|_| "fbdevd entry log failed")?;
    // Always try to configure ramfb. If ramfb is not available (no fw_cfg entry),
    // configure_ramfb will fail and we fall through to the disabled loop below.
    // In interactive mode (just start), ramfb IS available via QEMU -device ramfb.
    let ramfb_available = display_bootstrap_requested()
        || crate::backend::ramfb::find_file_select(b"etc/ramfb").is_some();
    if !ramfb_available {
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
    // Write boot splash so the user sees something immediately.
    splash::write_splash(framebuffer.handle as u32, mode);

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
    let mut assets_summary_emitted = false;
    let mut windowd_retry_count: u32 = 0;
    let mut windowd_last_attempt_ns: u64 = 0;

    loop {
        service_requests(&server, service.visible_state())?;
        if !framebuffer_registered {
            framebuffer_registered = register_framebuffer_with_windowd(
                &mut windowd_frame_client,
                framebuffer.handle as u32,
                &mut windowd_retry_count,
                &mut windowd_last_attempt_ns,
            );
            if framebuffer_registered && !flush_ok_emitted {
                debug_println(FLUSH_OK_MARKER).map_err(|_| "fbdevd flush log failed")?;
                flush_ok_emitted = true;
                if !cursor_overlay_emitted {
                    let _ = debug_println(crate::markers::CURSOR_OVERLAY_ON_MARKER);
                    cursor_overlay_emitted = true;
                }
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
                let observer_state = service.visible_state();
                if !cursor_overlay_emitted
                    && (observer_state.cursor_overlay_visible
                        || (flush_ok_emitted
                            && observer_state.cursor_svg_visible
                            && observer_state.systemui_first_frame_visible))
                {
                    let _ = debug_println(crate::markers::CURSOR_OVERLAY_ON_MARKER);
                    cursor_overlay_emitted = true;
                }
                if cursor_overlay_emitted
                    && !assets_summary_emitted
                    && observer_state.cursor_svg_visible
                    && observer_state.text_target_visible
                    && observer_state.icon_target_visible
                    && observer_state.wallpaper_visible
                    && observer_state.input_visible_on
                    && (observer_state.wheel_up_visible || observer_state.wheel_down_visible)
                {
                    let _ = debug_println(windowd::SELFTEST_UI_V2B_ASSETS_OK_MARKER);
                    assets_summary_emitted = true;
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
            if let Some((windowd_report, fbdevd_report)) = service.telemetry_values_if_due(now_ns) {
                if let Some(report) = windowd_report {
                    emit_windowd_telemetry(report);
                }
                if let Some(report) = fbdevd_report {
                    emit_fbdevd_telemetry(report);
                }
            }
        }
        let _ = yield_();
    }
}

fn emit_windowd_telemetry(report: WindowdDisplayTelemetryReport) {
    let mut line = FixedDebugLine::new();
    if write!(
        &mut line,
        "fps: windowd compose_hz={} present_hz={} coalesced={} dropped={} damage_px={} avg_render_us={} max_render_us={}",
        report.compose_hz,
        report.present_hz,
        report.coalesced_events,
        report.dropped_events,
        report.damage_pixels,
        report.avg_render_us,
        report.max_render_us
    )
    .is_err()
    {
        return;
    }
    if let Some(line) = line.as_str() {
        let _ = debug_println(line);
    }
}

fn emit_fbdevd_telemetry(report: DisplayScanoutReport) {
    let mut line = FixedDebugLine::new();
    if write!(
        &mut line,
        "fps: fbdevd flush_hz={} vsync_hz={} bytes={} flush_fail={} stale_scanout={}",
        report.flush_hz,
        report.vsync_hz,
        report.bytes_flushed,
        report.flush_failures,
        report.stale_scanout
    )
    .is_err()
    {
        return;
    }
    if let Some(line) = line.as_str() {
        let _ = debug_println(line);
    }
}

fn bind_server() -> Result<KernelServer, &'static str> {
    for _ in 0..ROUTE_BIND_RETRIES {
        if let Ok(server) = KernelServer::new_for(ROUTE_NAME) {
            return Ok(server);
        }
        let _ = yield_();
    }
    KernelServer::new_with_slots(3, 4).map_err(|_| "fbdevd: init fail kernel-server")
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
    const RPC_SEND_TIMEOUT_MS: u64 = 2;
    const RPC_RECV_TIMEOUT_MS: u64 = 6;
    let send_wait = Wait::Timeout(Duration::from_millis(RPC_SEND_TIMEOUT_MS));
    let (reply_send_slot, _) = reply.slots();
    let reply_send_clone = cap_clone(reply_send_slot).ok()?;
    let request = encode_get_visible_state();
    client.send_with_cap_move_wait(&request, reply_send_clone, send_wait).ok()?;
    // `windowd` serves observer queries on the same loop as input-driven composition, so give
    // replies a slightly larger but still bounded budget to avoid under-load telemetry dropouts.
    let recv_wait = Wait::Timeout(Duration::from_millis(RPC_RECV_TIMEOUT_MS));
    let frame = reply.recv(recv_wait).ok()?;
    decode_visible_state(&frame)
}

fn register_framebuffer_with_windowd(
    client: &mut Option<KernelClient>,
    framebuffer_handle: u32,
    retry_count: &mut u32,
    last_attempt_ns: &mut u64,
) -> bool {
    // Exponential backoff: 10ms → 50ms → 250ms → 500ms (max).
    let now_ns = nsec().unwrap_or(0);
    let backoff_ns = match *retry_count {
        0 => 0,
        1..=3 => 10_000_000,
        4..=7 => 50_000_000,
        8..=15 => 250_000_000,
        _ => 500_000_000,
    };
    if *retry_count > 0 && now_ns.saturating_sub(*last_attempt_ns) < backoff_ns {
        return false;
    }
    *last_attempt_ns = now_ns;

    if client.is_none() {
        *client = KernelClient::new_for("windowd").ok();
    }
    let Some(windowd) = client.as_ref() else {
        *retry_count = retry_count.saturating_add(1);
        return false;
    };
    let Ok(clone) = cap_clone(framebuffer_handle) else {
        *retry_count = retry_count.saturating_add(1);
        return false;
    };
    let request = encode_send_composed_frame_vmo();
    if windowd
        .send_with_cap_move_wait(&request, clone, Wait::Timeout(Duration::from_millis(10)))
        .is_err()
    {
        *client = None;
        *retry_count = retry_count.saturating_add(1);
        return false;
    }
    match windowd.recv(Wait::Timeout(Duration::from_millis(500))) {
        Ok(frame) if decode_status(&frame, OP_SEND_COMPOSED_FRAME_VMO) == Some(STATUS_OK) => true,
        _ => {
            *client = None;
            *retry_count = retry_count.saturating_add(1);
            if *retry_count == 3 {
                let _ = debug_println("fbdevd: windowd register retry (windowd not ready?)");
            }
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
