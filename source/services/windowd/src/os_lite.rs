// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

extern crate alloc;

use input_live_protocol::{
    encode_status, encode_visible_state_frame, frame_has_op, VisibleState, OP_GET_VISIBLE_STATE,
    STATUS_UNSUPPORTED,
};
use nexus_abi::{debug_println, nsec, yield_};
use nexus_ipc::{IpcError, KernelServer, Server as _, Wait};

use crate::frame::Layer;
use crate::geometry::Rect;
use crate::ids::{CallerCtx, CommitSeq};
use crate::markers::READY_MARKER;
use crate::server::{
    WindowServer, WindowdConfig, VISIBLE_BOOTSTRAP_HEIGHT, VISIBLE_BOOTSTRAP_HZ,
    VISIBLE_BOOTSTRAP_WIDTH,
};

const ROUTE_NAME: &str = "windowd";

pub fn service_main_loop() -> Result<(), &'static str> {
    let server = match KernelServer::new_for(ROUTE_NAME) {
        Ok(s) => s,
        Err(_) => {
            let _ = debug_println("windowd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| "windowd: init fail kernel-server")?
        }
    };
    let mut window = WindowServer::new(WindowdConfig {
        width: VISIBLE_BOOTSTRAP_WIDTH,
        height: VISIBLE_BOOTSTRAP_HEIGHT,
        hz: VISIBLE_BOOTSTRAP_HZ,
    })
    .map_err(|_| "windowd: init fail window-server")?;
    window.enable_fastpath();

    // --- TASK-0057: Load and commit embedded cursor SVG asset ---
    let cursor_caller = CallerCtx::system();
    if let Some(cursor_buf) = crate::render_assets::render_cursor_surface(cursor_caller) {
        if let Ok(cursor_sid) = window.create_surface(cursor_caller, cursor_buf.clone()) {
            let _ = window.queue_buffer(
                cursor_caller,
                cursor_sid,
                cursor_buf.clone(),
                &[Rect::new(0, 0, cursor_buf.width, cursor_buf.height)],
            );
            let _ = window.commit_scene(
                CallerCtx::system(),
                CommitSeq::new(1),
                &[Layer { surface: cursor_sid, x: 400, y: 300, z: 0 }],
            );
            let _ = debug_println(crate::markers::CURSOR_SVG_LOADED_MARKER);
        }
    } else {
        let _ = debug_println("windowd: cursor svg render failed");
    }

    let _ = debug_println(READY_MARKER);
    loop {
        match server.recv_request_with_meta(Wait::NonBlocking) {
            Ok((frame, _sid, reply)) => {
                if frame_has_op(&frame, OP_GET_VISIBLE_STATE) {
                    let has_frame = window.last_frame().is_some();
                    let state = VisibleState {
                        scene_ready: has_frame,
                        full_window_visible: has_frame,
                        click_target_visible: false,
                        keyboard_target_visible: false,
                        cursor_x: 0,
                        cursor_y: 0,
                        cursor_move_visible: false,
                        hover_visible: false,
                        focus_visible: false,
                        input_visible_on: false,
                        virtio_raw_seen: false,
                        hid_normalized_seen: false,
                        pointer_route_live: false,
                        keyboard_route_live: false,
                        backend_visible: has_frame,
                        display_scanout_ready: has_frame,
                        systemui_first_frame_visible: false,
                        launcher_click_visible: false,
                        keyboard_visible: false,
                        wheel_up_visible: false,
                        wheel_down_visible: false,
                    };
                    let response = encode_visible_state_frame(state);
                    if let Some(reply) = reply {
                        let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
                    } else {
                        let _ = server.send(&response, Wait::Blocking);
                    }
                } else {
                    let op = frame.get(3).copied().unwrap_or(0);
                    let response = encode_status(op, STATUS_UNSUPPORTED);
                    if let Some(reply) = reply {
                        let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
                    } else {
                        let _ = server.send(&response, Wait::Blocking);
                    }
                }
            }
            Err(IpcError::WouldBlock)
            | Err(IpcError::Timeout)
            | Err(IpcError::Disconnected)
            | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => {}
            Err(_) => {}
        }
        if let Ok(now_ns) = nsec() {
            if now_ns > 0 {
                let _ = window.present_tick();
            }
        }
        let _ = yield_();
    }
}
