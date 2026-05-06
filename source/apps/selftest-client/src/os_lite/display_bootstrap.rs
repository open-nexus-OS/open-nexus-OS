// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Visible QEMU `ramfb` bootstrap path for TASK-0055B/TASK-0055C/TASK-0056B.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder plus `windowd`/`ui_windowd_host` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

extern crate alloc;

use alloc::{format, vec::Vec};
use input_live_protocol::{decode_visible_state, encode_get_visible_state, VisibleState};
use nexus_abi::{
    cap_clone, cap_query, nsec, page_flags, vmo_create, vmo_map_page, vmo_write, yield_, CapQuery,
    Handle,
};
use nexus_ipc::{Client as _, Wait};

use crate::os_lite::boot_cfg;
use crate::os_lite::ipc::clients::cached_reply_client;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::runtime_mode::RuntimeMode;

const VISIBLE_INPUT_PROOF_WIDTH: u32 = 64;
const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;
const VISIBLE_INPUT_SURFACE_X: i32 = 0;
const VISIBLE_INPUT_SURFACE_Y: i32 = 0;
const VISIBLE_INPUT_SURFACE_WIDTH: u32 = VISIBLE_INPUT_PROOF_WIDTH;
const VISIBLE_INPUT_SURFACE_HEIGHT: u32 = VISIBLE_INPUT_PROOF_HEIGHT;
const VISIBLE_INPUT_CURSOR_START_X: i32 = 24;
const VISIBLE_INPUT_CURSOR_START_Y: i32 = 12;
const VISIBLE_INPUT_CURSOR_END_X: i32 = 8;
const VISIBLE_INPUT_CURSOR_END_Y: i32 = 40;
const VISIBLE_INPUT_LEFT_SQUARE_X: u32 = 4;
const VISIBLE_INPUT_LEFT_SQUARE_Y: u32 = 36;
const VISIBLE_INPUT_RIGHT_SQUARE_X: u32 = 52;
const VISIBLE_INPUT_RIGHT_SQUARE_Y: u32 = 18;
const VISIBLE_INPUT_SQUARE_SIZE: u32 = 8;
const VISIBLE_INPUT_BGRA: [u8; 4] = [0x18, 0x30, 0x88, 0xff];
const VISIBLE_INPUT_LEFT_IDLE_BGRA: [u8; 4] = [0x30, 0x70, 0xd8, 0xff];
const VISIBLE_INPUT_LEFT_HOVER_BGRA: [u8; 4] = [0x20, 0xd0, 0xf8, 0xff];
const VISIBLE_INPUT_RIGHT_IDLE_BGRA: [u8; 4] = [0x90, 0x40, 0x40, 0xff];
const LIVE_INPUT_REPEAT_CODE: u32 = 0x04;
const LIVE_INPUT_TOUCH_BOUNDS: u32 = 128;

const DMA_VMO_VA: usize = 0x2002_0000;
const PAGE_SIZE: usize = 4096;

const RAMFB_FILE_NAME: &[u8] = b"etc/ramfb";
const RAMFB_CONFIG_LEN: usize = 28;
const RAMFB_CONFIG_OFFSET: usize = 0;
const DMA_ACCESS_OFFSET: usize = 64;
const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241; // "AR24"
const DISPLAY_SETTLE_NS: u64 = 750_000_000;
const DISPLAY_SETTLE_MAX_YIELDS: usize = 200_000;

#[derive(Clone, Copy)]
struct InteractiveDisplayCtx {
    framebuffer: Handle,
    mode: windowd::VisibleBootstrapMode,
}

static mut INTERACTIVE_DISPLAY_CTX: Option<InteractiveDisplayCtx> = None;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapFailure {
    WindowdEvidence,
    VisibleInputEvidence,
    InteractiveSceneEvidence,
    InvalidMode,
    FramebufferVmo,
    InvalidFramebufferCap,
    InvalidDisplayCapability,
    FrameWrite,
    FwCfgMap,
    FwCfgSignature,
    RamfbFileMissing,
    DmaVmo,
    InvalidDmaCap,
    DmaFailed,
}

pub(crate) struct BootstrapEvidence {
    pub(crate) runtime_mode: RuntimeMode,
    pub(crate) systemui: windowd::VisibleSystemUiEvidence,
    pub(crate) proof: Option<ProofBootstrapEvidence>,
    pub(crate) interactive: Option<InteractiveBootstrapEvidence>,
}

pub(crate) struct ProofBootstrapEvidence {
    pub(crate) visible_state: VisibleState,
}

pub(crate) struct LiveInputEvidence {
    pub(crate) hid_ready: bool,
    pub(crate) hid_keyboard_ready: bool,
    pub(crate) hid_mouse_ready: bool,
    pub(crate) touch_ready: bool,
    pub(crate) input_ready: bool,
    pub(crate) keymap_de_ok: bool,
    pub(crate) cursor_ok: bool,
    pub(crate) touch_ok: bool,
    pub(crate) repeat_ok: bool,
    pub(crate) ime_show: bool,
    pub(crate) ime_hide: bool,
}

pub(crate) struct InteractiveBootstrapEvidence {
    pub(crate) scene_ready: bool,
    pub(crate) full_window_visible: bool,
    pub(crate) click_target_visible: bool,
    pub(crate) keyboard_target_visible: bool,
}

pub(crate) fn enabled() -> bool {
    boot_cfg::display_bootstrap_enabled()
}

pub(crate) fn run() -> Option<BootstrapEvidence> {
    match run_result() {
        Ok(evidence) => Some(evidence),
        Err(_err) => {
            let _ = nexus_abi::debug_println(_err.log_label());
            None
        }
    }
}

pub(crate) fn run_result() -> Result<BootstrapEvidence, BootstrapFailure> {
    let runtime_mode = boot_cfg::runtime_mode().unwrap_or(RuntimeMode::Proof);
    let evidence =
        windowd::run_visible_systemui_smoke().map_err(|_| BootstrapFailure::WindowdEvidence)?;
    let mode = evidence.mode.validate().map_err(|_| BootstrapFailure::InvalidMode)?;
    let fb_len = mode.byte_len().map_err(|_| BootstrapFailure::InvalidMode)?;
    let framebuffer = vmo_create(fb_len).map_err(|_| BootstrapFailure::FramebufferVmo)?;
    let fb_query = query_cap(framebuffer).ok_or(BootstrapFailure::InvalidFramebufferCap)?;
    if fb_query.kind_tag != 1 || fb_query.len < fb_len as u64 {
        return Err(BootstrapFailure::InvalidFramebufferCap);
    }

    let cap = windowd::VisibleDisplayCapability { byte_len: fb_len, mapped: true, writable: true };
    windowd::validate_visible_bootstrap_capability(mode, cap)
        .map_err(|_| BootstrapFailure::InvalidDisplayCapability)?;
    write_windowd_composed_rows(framebuffer, mode, &evidence)?;
    configure_ramfb(fb_query.base, mode)?;
    settle_visible_display();
    let systemui = windowd::visible_systemui_marker_postflight_ready(Some(evidence))
        .map_err(|_| BootstrapFailure::WindowdEvidence)?;
    match runtime_mode {
        RuntimeMode::Proof => {
            let visible_state = observe_live_visible_input_proof()?;
            write_interactive_visible_state_rows(framebuffer, mode, visible_state)?;
            settle_visible_display();
            Ok(BootstrapEvidence {
                runtime_mode,
                systemui,
                proof: Some(ProofBootstrapEvidence { visible_state }),
                interactive: None,
            })
        }
        RuntimeMode::InteractiveMinimal | RuntimeMode::InteractiveFull => {
            let interactive = run_interactive_visible_scene()?;
            write_windowd_visible_input_rows(framebuffer, mode, &interactive.scene_frame)?;
            settle_visible_display();
            store_interactive_display_ctx(framebuffer, mode);
            Ok(BootstrapEvidence {
                runtime_mode,
                systemui,
                proof: None,
                interactive: Some(InteractiveBootstrapEvidence {
                    scene_ready: interactive.scene_ready,
                    full_window_visible: interactive.full_window_visible,
                    click_target_visible: interactive.click_target_visible,
                    keyboard_target_visible: interactive.keyboard_target_visible,
                }),
            })
        }
    }
}

impl BootstrapFailure {
    #[must_use]
    fn log_label(self) -> &'static str {
        match self {
            BootstrapFailure::WindowdEvidence => "bootstrap: failed windowd-evidence",
            BootstrapFailure::VisibleInputEvidence => "bootstrap: failed visible-input-evidence",
            BootstrapFailure::InteractiveSceneEvidence => {
                "bootstrap: failed interactive-scene-evidence"
            }
            BootstrapFailure::InvalidMode => "bootstrap: failed invalid-mode",
            BootstrapFailure::FramebufferVmo => "bootstrap: failed framebuffer-vmo",
            BootstrapFailure::InvalidFramebufferCap => "bootstrap: failed invalid-framebuffer-cap",
            BootstrapFailure::InvalidDisplayCapability => {
                "bootstrap: failed invalid-display-capability"
            }
            BootstrapFailure::FrameWrite => "bootstrap: failed frame-write",
            BootstrapFailure::FwCfgMap => "bootstrap: failed fw-cfg-map",
            BootstrapFailure::FwCfgSignature => "bootstrap: failed fw-cfg-signature",
            BootstrapFailure::RamfbFileMissing => "bootstrap: failed ramfb-file-missing",
            BootstrapFailure::DmaVmo => "bootstrap: failed dma-vmo",
            BootstrapFailure::InvalidDmaCap => "bootstrap: failed invalid-dma-cap",
            BootstrapFailure::DmaFailed => "bootstrap: failed dma",
        }
    }
}

struct InteractiveSceneSetup {
    scene_ready: bool,
    full_window_visible: bool,
    click_target_visible: bool,
    keyboard_target_visible: bool,
    scene_frame: windowd::UiVisibleInputEvidence,
}

fn run_interactive_visible_scene() -> Result<InteractiveSceneSetup, BootstrapFailure> {
    let launcher = windowd::CallerCtx::from_service_metadata(0x55);
    let mut server = windowd::WindowServer::new(windowd::WindowdConfig {
        width: VISIBLE_INPUT_PROOF_WIDTH,
        height: VISIBLE_INPUT_PROOF_HEIGHT,
        hz: 60,
    })
    .map_err(|_| BootstrapFailure::InteractiveSceneEvidence)?;
    let initial = visible_input_scene_surface(
        launcher,
        90,
        VISIBLE_INPUT_LEFT_IDLE_BGRA,
        VISIBLE_INPUT_RIGHT_IDLE_BGRA,
    )?;
    let surface = server
        .create_surface(launcher, initial.clone())
        .map_err(|_| BootstrapFailure::InteractiveSceneEvidence)?;
    server
        .queue_buffer(
            launcher,
            surface,
            initial,
            &[windowd::Rect::new(0, 0, VISIBLE_INPUT_SURFACE_WIDTH, VISIBLE_INPUT_SURFACE_HEIGHT)],
        )
        .map_err(|_| BootstrapFailure::InteractiveSceneEvidence)?;
    server
        .commit_scene(
            windowd::CallerCtx::system(),
            windowd::CommitSeq::new(1),
            &[windowd::Layer {
                surface,
                x: VISIBLE_INPUT_SURFACE_X,
                y: VISIBLE_INPUT_SURFACE_Y,
                z: 0,
            }],
        )
        .map_err(|_| BootstrapFailure::InteractiveSceneEvidence)?;
    let initial_present =
        match server.present_tick().map_err(|_| BootstrapFailure::InteractiveSceneEvidence)? {
            Some(present) => {
                let _ = nexus_abi::debug_println("debug8cde1d: interactive present-tick-ok");
                present
            }
            None => return Err(BootstrapFailure::InteractiveSceneEvidence),
        };
    let scheduled_present = windowd::ScheduledPresentAck {
        seq: initial_present.seq,
        damage_rects: initial_present.damage_rects,
        frames_coalesced: 1,
        fences_signaled: 0,
        latency_ms: 0,
    };
    let scene_frame =
        server.last_frame().cloned().ok_or(BootstrapFailure::InteractiveSceneEvidence)?;
    let full_window_visible = pixel_eq(&scene_frame, 16, 16, VISIBLE_INPUT_BGRA)?
        || pixel_eq(&scene_frame, 16, 16, [0x24, 0x38, 0xa0, 0xff])?;
    let click_target_visible = pixel_eq(
        &scene_frame,
        VISIBLE_INPUT_LEFT_SQUARE_X as i32 + 1,
        VISIBLE_INPUT_LEFT_SQUARE_Y as i32 + 1,
        VISIBLE_INPUT_LEFT_IDLE_BGRA,
    )?;
    let keyboard_target_visible = pixel_eq(
        &scene_frame,
        VISIBLE_INPUT_RIGHT_SQUARE_X as i32 + 1,
        VISIBLE_INPUT_RIGHT_SQUARE_Y as i32 + 1,
        VISIBLE_INPUT_RIGHT_IDLE_BGRA,
    )?;
    Ok(InteractiveSceneSetup {
        scene_ready: scheduled_present.seq.raw() == 1
            && scene_frame.width == VISIBLE_INPUT_PROOF_WIDTH,
        full_window_visible,
        click_target_visible,
        keyboard_target_visible,
        scene_frame: windowd::UiVisibleInputEvidence {
            input_visible_on: false,
            full_window_visible,
            cursor_move_visible: false,
            hover_visible: false,
            focus_visible: false,
            launcher_click_visible: click_target_visible,
            keyboard_visible: keyboard_target_visible,
            focused_surface: surface,
            cursor_start_position: windowd::PointerPosition {
                x: VISIBLE_INPUT_CURSOR_START_X,
                y: VISIBLE_INPUT_CURSOR_START_Y,
            },
            cursor_position: windowd::PointerPosition {
                x: VISIBLE_INPUT_CURSOR_START_X,
                y: VISIBLE_INPUT_CURSOR_START_Y,
            },
            scheduled_present,
            cursor_frame: None,
            hover_frame: None,
            keyboard_frame: None,
            visible_frame: Some(scene_frame),
        },
    })
}

fn store_interactive_display_ctx(framebuffer: Handle, mode: windowd::VisibleBootstrapMode) {
    unsafe {
        INTERACTIVE_DISPLAY_CTX = Some(InteractiveDisplayCtx { framebuffer, mode });
    }
}

pub(crate) fn interactive_live_tick() -> Option<VisibleState> {
    let ctx = unsafe { INTERACTIVE_DISPLAY_CTX }?;
    let state = fetch_live_visible_state()?;
    write_interactive_visible_state_rows(ctx.framebuffer, ctx.mode, state).ok()?;
    Some(state)
}

fn fetch_live_visible_state() -> Option<VisibleState> {
    let client = route_with_retry("inputd").ok()?;
    let reply = cached_reply_client().ok()?;
    let (reply_send_slot, _) = reply.slots();
    let reply_send_clone = cap_clone(reply_send_slot).ok()?;
    let request = encode_get_visible_state();
    client.send_with_cap_move_wait(&request, reply_send_clone, Wait::Blocking).ok()?;
    let frame = reply.recv(Wait::Blocking).ok()?;
    decode_visible_state(&frame)
}

fn observe_live_visible_input_proof() -> Result<VisibleState, BootstrapFailure> {
    // QMP-visible input can arrive shortly after the observer scene is live, so the proof lane
    // needs enough patience to see the real service chain without becoming an authority itself.
    const OBSERVER_MAX_POLLS: usize = 128;
    const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;
    let mut last_state = None;

    for _ in 0..OBSERVER_MAX_POLLS {
        if let Some(state) = fetch_live_visible_state() {
            last_state = Some(state);
            if state.virtio_raw_seen
                && state.hid_normalized_seen
                && state.scene_ready
                && state.full_window_visible
                && state.click_target_visible
                && state.keyboard_target_visible
                && state.input_visible_on
                && state.cursor_move_visible
                && state.hover_visible
                && state.focus_visible
                && state.launcher_click_visible
                && state.keyboard_visible
                && state.pointer_route_live
                && state.keyboard_route_live
            {
                return Ok(state);
            }
        }
        for _ in 0..OBSERVER_YIELDS_BETWEEN_POLLS {
            let _ = yield_();
        }
    }

    if let Some(state) = last_state {
        let _ = nexus_abi::debug_println(&format!(
            "bootstrap: visible-state timeout raw={} norm={} scene={} full={} click_target={} keyboard_target={} input_on={} cursor={} hover={} focus={} click={} keyboard={} pointer_route={} keyboard_route={}",
            u8::from(state.virtio_raw_seen),
            u8::from(state.hid_normalized_seen),
            u8::from(state.scene_ready),
            u8::from(state.full_window_visible),
            u8::from(state.click_target_visible),
            u8::from(state.keyboard_target_visible),
            u8::from(state.input_visible_on),
            u8::from(state.cursor_move_visible),
            u8::from(state.hover_visible),
            u8::from(state.focus_visible),
            u8::from(state.launcher_click_visible),
            u8::from(state.keyboard_visible),
            u8::from(state.pointer_route_live),
            u8::from(state.keyboard_route_live),
        ));
    }

    Err(BootstrapFailure::VisibleInputEvidence)
}

fn write_interactive_visible_state_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    state: VisibleState,
) -> Result<(), BootstrapFailure> {
    let left_square = if state.launcher_click_visible {
        windowd::VISIBLE_INPUT_CLICK_BGRA
    } else if state.hover_visible {
        VISIBLE_INPUT_LEFT_HOVER_BGRA
    } else {
        VISIBLE_INPUT_LEFT_IDLE_BGRA
    };
    let right_square = if state.keyboard_visible {
        windowd::VISIBLE_INPUT_KEYBOARD_BGRA
    } else {
        VISIBLE_INPUT_RIGHT_IDLE_BGRA
    };
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        write_interactive_visible_state_row(y, mode, state, left_square, right_square, &mut row)?;
        let dst_offset = y as usize * row_len;
        vmo_write(handle, dst_offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_interactive_visible_state_row(
    y: u32,
    mode: windowd::VisibleBootstrapMode,
    state: VisibleState,
    left_square: [u8; 4],
    right_square: [u8; 4],
    row: &mut [u8],
) -> Result<(), BootstrapFailure> {
    let width = core::cmp::min(mode.width as usize, windowd::VISIBLE_BOOTSTRAP_WIDTH as usize);
    let cursor_x = u32::try_from(state.cursor_x).ok();
    let cursor_y = u32::try_from(state.cursor_y).ok();
    for x in 0..width {
        let mut bgra = visible_input_scene_pixel_bgra(x as u32, y, left_square, right_square);
        if state.scene_ready && cursor_x == Some(x as u32) && cursor_y == Some(y) {
            bgra = windowd::VISIBLE_CURSOR_BGRA;
        }
        let idx = x.checked_mul(4).ok_or(BootstrapFailure::FrameWrite)?;
        row[idx..idx + 4].copy_from_slice(&bgra);
    }
    Ok(())
}

fn run_deterministic_visible_input_proof(
) -> Result<(windowd::UiVisibleInputEvidence, LiveInputEvidence), BootstrapFailure> {
    let launcher = windowd::CallerCtx::from_service_metadata(0x55);
    let mut server = windowd::WindowServer::new(windowd::WindowdConfig {
        width: VISIBLE_INPUT_PROOF_WIDTH,
        height: VISIBLE_INPUT_PROOF_HEIGHT,
        hz: 60,
    })
    .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let initial = visible_input_scene_surface(
        launcher,
        50,
        VISIBLE_INPUT_LEFT_IDLE_BGRA,
        VISIBLE_INPUT_RIGHT_IDLE_BGRA,
    )?;
    let surface = server
        .create_surface(launcher, initial.clone())
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    server
        .queue_buffer(
            launcher,
            surface,
            initial,
            &[windowd::Rect::new(0, 0, VISIBLE_INPUT_SURFACE_WIDTH, VISIBLE_INPUT_SURFACE_HEIGHT)],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    server
        .commit_scene(
            windowd::CallerCtx::system(),
            windowd::CommitSeq::new(1),
            &[windowd::Layer {
                surface,
                x: VISIBLE_INPUT_SURFACE_X,
                y: VISIBLE_INPUT_SURFACE_Y,
                z: 0,
            }],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let _ = server.present_tick().map_err(|_| BootstrapFailure::VisibleInputEvidence)?;

    let mut hid_service = hidrawd::HidrawdService::new();
    let keyboard_id = hidrawd::DeviceId::new(7);
    let mouse_id = hidrawd::DeviceId::new(8);
    hid_service.register_keyboard(keyboard_id);
    hid_service.register_mouse(mouse_id);

    let bounds = touch::TouchBounds::new(LIVE_INPUT_TOUCH_BOUNDS, LIVE_INPUT_TOUCH_BOUNDS)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let mut touch_service =
        touchd::TouchdService::new(bounds, touchd::SyntheticTouchMode::ProofFixture);
    touch_service.register_device(touchd::TouchDeviceId::new(9));

    let config = inputd::InputdConfig::new("de", 100, 10, 64, 1, 1, 96, 32, 0, 0)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let mut input_service = inputd::InputdService::new(server, config)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;

    let start_batch = hid_service
        .ingest_mouse_report(
            mouse_id,
            hid::TimestampNs::new(1),
            &[0, VISIBLE_INPUT_CURSOR_START_X as u8, VISIBLE_INPUT_CURSOR_START_Y as u8],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let start_dispatches = input_service
        .apply_hid_batch(&start_batch)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_start_position =
        input_service.router().pointer_position().ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let cursor_frame = input_service
        .router_mut()
        .render_visible_input_frame()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_start_visible = matches!(
        start_dispatches.as_slice(),
        [inputd::InputDispatch::PointerMove {
            x: VISIBLE_INPUT_CURSOR_START_X,
            y: VISIBLE_INPUT_CURSOR_START_Y,
            ..
        }]
    ) && pixel_eq(
        &cursor_frame,
        cursor_start_position.x,
        cursor_start_position.y,
        windowd::VISIBLE_CURSOR_BGRA,
    )?;

    let move_batch = hid_service
        .ingest_mouse_report(
            mouse_id,
            hid::TimestampNs::new(2),
            &[
                0,
                (VISIBLE_INPUT_CURSOR_END_X - VISIBLE_INPUT_CURSOR_START_X) as u8,
                (VISIBLE_INPUT_CURSOR_END_Y - VISIBLE_INPUT_CURSOR_START_Y) as u8,
            ],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let move_dispatches = input_service
        .apply_hid_batch(&move_batch)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_position =
        input_service.router().pointer_position().ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let hover_surface = visible_input_scene_surface(
        launcher,
        51,
        VISIBLE_INPUT_LEFT_HOVER_BGRA,
        VISIBLE_INPUT_RIGHT_IDLE_BGRA,
    )?;
    input_service
        .router_mut()
        .acquire_back_buffer(launcher, surface, windowd::FrameIndex::new(1), hover_surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    input_service
        .router_mut()
        .present_frame(
            launcher,
            surface,
            windowd::FrameIndex::new(1),
            &[windowd::Rect::new(0, 0, VISIBLE_INPUT_SURFACE_WIDTH, VISIBLE_INPUT_SURFACE_HEIGHT)],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let _ = input_service
        .router_mut()
        .present_scheduler_tick()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let hover_frame = input_service
        .router_mut()
        .render_visible_input_frame()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_move_visible = cursor_start_visible
        && matches!(
            move_dispatches.as_slice(),
            [inputd::InputDispatch::PointerMove {
                x: VISIBLE_INPUT_CURSOR_END_X,
                y: VISIBLE_INPUT_CURSOR_END_Y,
                ..
            }]
        )
        && cursor_position != cursor_start_position
        && pixel_eq(
            &hover_frame,
            cursor_position.x,
            cursor_position.y,
            windowd::VISIBLE_CURSOR_BGRA,
        )?;
    let hover_visible = input_service.router().last_pointer_hit() == Some(surface)
        && pixel_eq(
            &hover_frame,
            VISIBLE_INPUT_LEFT_SQUARE_X as i32 + 1,
            VISIBLE_INPUT_LEFT_SQUARE_Y as i32 + 1,
            VISIBLE_INPUT_LEFT_HOVER_BGRA,
        )?;

    let down_batch = hid_service
        .ingest_mouse_report(mouse_id, hid::TimestampNs::new(3), &[0b001, 0, 0])
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let down_dispatches = input_service
        .apply_hid_batch(&down_batch)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let routed_click = matches!(
        down_dispatches.as_slice(),
        [inputd::InputDispatch::PointerDown {
            x: VISIBLE_INPUT_CURSOR_END_X,
            y: VISIBLE_INPUT_CURSOR_END_Y,
            ..
        }]
    ) && delivered
        .iter()
        .any(|event| matches!(event.kind, windowd::InputEventKind::PointerMove { .. }))
        && delivered.iter().any(|event| event.kind == windowd::InputEventKind::PointerDown);
    if !routed_click {
        return Err(BootstrapFailure::VisibleInputEvidence);
    }

    let mut ime_service = ime::ImeService::new();
    let mut ime_overlay = systemui::ImeOverlayState::new();
    let ime_show = matches!(
        input_service.set_text_focus(true).map_err(|_| BootstrapFailure::VisibleInputEvidence)?,
        Some(inputd::ImeHook::Show)
    ) && ime_service.show()
        && ime_overlay.show()
        && ime_service.visible()
        && ime_overlay.visible();

    let highlighted = visible_input_scene_surface(
        launcher,
        52,
        windowd::VISIBLE_INPUT_CLICK_BGRA,
        VISIBLE_INPUT_RIGHT_IDLE_BGRA,
    )?;
    input_service
        .router_mut()
        .acquire_back_buffer(launcher, surface, windowd::FrameIndex::new(2), highlighted)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    input_service
        .router_mut()
        .present_frame(
            launcher,
            surface,
            windowd::FrameIndex::new(2),
            &[windowd::Rect::new(0, 0, VISIBLE_INPUT_SURFACE_WIDTH, VISIBLE_INPUT_SURFACE_HEIGHT)],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let scheduled_present = input_service
        .router_mut()
        .present_scheduler_tick()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let visible_frame = input_service
        .router()
        .last_frame()
        .cloned()
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let focus_visible = input_service.router().focused_surface() == Some(surface)
        && pixel_eq(
            &visible_frame,
            VISIBLE_INPUT_SURFACE_X,
            VISIBLE_INPUT_SURFACE_Y,
            windowd::VISIBLE_FOCUS_BGRA,
        )?;
    let launcher_click_visible = pixel_eq(
        &visible_frame,
        VISIBLE_INPUT_LEFT_SQUARE_X as i32 + 1,
        VISIBLE_INPUT_LEFT_SQUARE_Y as i32 + 1,
        windowd::VISIBLE_INPUT_CLICK_BGRA,
    )?;

    let keymap_batch = hid_service
        .ingest_keyboard_report(keyboard_id, hid::TimestampNs::new(4), &[0, 0, 0x2f, 0, 0, 0, 0, 0])
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_dispatches = input_service
        .apply_hid_batch(&keymap_batch)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_de_ok = matches!(
        keymap_dispatches.as_slice(),
        [inputd::InputDispatch::Keyboard {
            output: keymaps::KeyOutput::Text('ü'),
            repeated: false,
            ..
        }]
    ) && keymap_delivered
        .iter()
        .any(|event| matches!(event.kind, windowd::InputEventKind::Keyboard { key_code: 0x2f }));
    let keyboard_surface = visible_input_scene_surface(
        launcher,
        53,
        windowd::VISIBLE_INPUT_CLICK_BGRA,
        windowd::VISIBLE_INPUT_KEYBOARD_BGRA,
    )?;
    input_service
        .router_mut()
        .acquire_back_buffer(launcher, surface, windowd::FrameIndex::new(3), keyboard_surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    input_service
        .router_mut()
        .present_frame(
            launcher,
            surface,
            windowd::FrameIndex::new(3),
            &[windowd::Rect::new(0, 0, VISIBLE_INPUT_SURFACE_WIDTH, VISIBLE_INPUT_SURFACE_HEIGHT)],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let _ = input_service
        .router_mut()
        .present_scheduler_tick()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let keyboard_frame = input_service
        .router()
        .last_frame()
        .cloned()
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let keyboard_visible = keymap_de_ok
        && pixel_eq(
            &keyboard_frame,
            VISIBLE_INPUT_RIGHT_SQUARE_X as i32 + 1,
            VISIBLE_INPUT_RIGHT_SQUARE_Y as i32 + 1,
            windowd::VISIBLE_INPUT_KEYBOARD_BGRA,
        )?;
    let full_window_visible = pixel_eq(&keyboard_frame, 16, 16, VISIBLE_INPUT_BGRA)?
        || pixel_eq(&keyboard_frame, 16, 16, [0x24, 0x38, 0xa0, 0xff])?;

    let keymap_release = hid_service
        .ingest_keyboard_report(keyboard_id, hid::TimestampNs::new(5), &[0, 0, 0, 0, 0, 0, 0, 0])
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let _ = input_service
        .apply_hid_batch(&keymap_release)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;

    let repeat_press = hid_service
        .ingest_keyboard_report(
            keyboard_id,
            hid::TimestampNs::new(6),
            &[0, 0, LIVE_INPUT_REPEAT_CODE as u8, 0, 0, 0, 0, 0],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let repeat_start = input_service
        .apply_hid_batch(&repeat_press)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let repeated = input_service
        .tick_repeat(100_000_006)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let repeat_delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let repeat_ok = matches!(
        repeat_start.as_slice(),
        [inputd::InputDispatch::Keyboard {
            key_code: LIVE_INPUT_REPEAT_CODE,
            repeated: false,
            output: keymaps::KeyOutput::Text('a'),
            ..
        }]
    ) && matches!(
        repeated.as_slice(),
        [inputd::InputDispatch::Keyboard {
            key_code: LIVE_INPUT_REPEAT_CODE,
            repeated: true,
            output: keymaps::KeyOutput::Text('a'),
            ..
        }]
    ) && repeat_delivered
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                windowd::InputEventKind::Keyboard { key_code: LIVE_INPUT_REPEAT_CODE }
            )
        })
        .count()
        == 2;

    let touch_events = touch_service
        .synthetic_sequence(1_000)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let touch_dispatches = touch_events
        .into_iter()
        .map(|event| {
            input_service
                .apply_touch_event(event)
                .map_err(|_| BootstrapFailure::VisibleInputEvidence)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let touch_delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let touch_ok = matches!(
        touch_dispatches.as_slice(),
        [
            inputd::InputDispatch::Touch { event: first, x: 20, y: 20, .. },
            inputd::InputDispatch::Touch { event: second, x: 28, y: 22, .. },
            inputd::InputDispatch::Touch { event: third, x: 28, y: 22, .. },
        ] if first.phase() == touch::TouchPhase::Down
            && second.phase() == touch::TouchPhase::Move
            && third.phase() == touch::TouchPhase::Up
    ) && matches!(
        touch_delivered.as_slice(),
        [
            windowd::InputDelivery {
                kind: windowd::InputEventKind::TouchDown { x: 20, y: 20 },
                ..
            },
            windowd::InputDelivery {
                kind: windowd::InputEventKind::TouchMove { x: 28, y: 22 },
                ..
            },
            windowd::InputDelivery { kind: windowd::InputEventKind::TouchUp { x: 28, y: 22 }, .. },
        ]
    );

    let ime_hide = matches!(
        input_service.set_text_focus(false).map_err(|_| BootstrapFailure::VisibleInputEvidence)?,
        Some(inputd::ImeHook::Hide)
    ) && ime_service.hide()
        && ime_overlay.hide()
        && !ime_service.visible()
        && !ime_overlay.visible();

    let focused_surface =
        input_service.router().focused_surface().ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let visible_input = windowd::UiVisibleInputEvidence {
        input_visible_on: input_service.router().input_enabled()
            && cursor_move_visible
            && hover_visible
            && focus_visible
            && keyboard_visible,
        full_window_visible,
        cursor_move_visible,
        hover_visible,
        focus_visible,
        launcher_click_visible,
        keyboard_visible,
        focused_surface,
        cursor_start_position,
        cursor_position,
        scheduled_present,
        cursor_frame: Some(cursor_frame),
        hover_frame: Some(hover_frame),
        keyboard_frame: Some(keyboard_frame),
        visible_frame: Some(visible_frame),
    };
    let live_input = LiveInputEvidence {
        hid_ready: hid_service.keyboard_ready() && hid_service.mouse_ready(),
        hid_keyboard_ready: hid_service.keyboard_ready(),
        hid_mouse_ready: hid_service.mouse_ready(),
        touch_ready: touch_service.ready(),
        input_ready: input_service.layout_name() == "de",
        keymap_de_ok,
        cursor_ok: visible_input.input_visible_on && visible_input.launcher_click_visible,
        touch_ok,
        repeat_ok,
        ime_show,
        ime_hide,
    };
    Ok((visible_input, live_input))
}

fn visible_input_scene_surface(
    caller: windowd::CallerCtx,
    frame_index: u64,
    left_square: [u8; 4],
    right_square: [u8; 4],
) -> Result<windowd::SurfaceBuffer, BootstrapFailure> {
    let mut surface = windowd::SurfaceBuffer::solid(
        caller,
        frame_index,
        VISIBLE_INPUT_SURFACE_WIDTH,
        VISIBLE_INPUT_SURFACE_HEIGHT,
        VISIBLE_INPUT_BGRA,
    )
    .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    for y in 0..surface.height {
        for x in 0..surface.width {
            let bgra = visible_input_scene_pixel_bgra(x, y, left_square, right_square);
            let idx = (y as usize * surface.stride as usize) + (x as usize * 4);
            surface.pixels[idx..idx + 4].copy_from_slice(&bgra);
        }
    }
    Ok(surface)
}

fn visible_input_scene_pixel_bgra(
    x: u32,
    y: u32,
    left_square: [u8; 4],
    right_square: [u8; 4],
) -> [u8; 4] {
    if rect_contains(
        x,
        y,
        VISIBLE_INPUT_LEFT_SQUARE_X,
        VISIBLE_INPUT_LEFT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
    ) {
        left_square
    } else if rect_contains(
        x,
        y,
        VISIBLE_INPUT_RIGHT_SQUARE_X,
        VISIBLE_INPUT_RIGHT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
    ) {
        right_square
    } else {
        let stripe = ((x / 8) + (y / 8)) & 1;
        if stripe == 0 {
            VISIBLE_INPUT_BGRA
        } else {
            [0x24, 0x38, 0xa0, 0xff]
        }
    }
}

fn rect_contains(x: u32, y: u32, left: u32, top: u32, width: u32, height: u32) -> bool {
    x >= left && x < left.saturating_add(width) && y >= top && y < top.saturating_add(height)
}

fn pixel_eq(
    frame: &windowd::Frame,
    x: i32,
    y: i32,
    expected: [u8; 4],
) -> Result<bool, BootstrapFailure> {
    if x < 0 || y < 0 {
        return Ok(false);
    }
    let x = u32::try_from(x).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let y = u32::try_from(y).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    if x >= frame.width || y >= frame.height {
        return Ok(false);
    }
    let idx = (y as usize)
        .checked_mul(frame.stride as usize)
        .and_then(|base| base.checked_add((x as usize).checked_mul(4)?))
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    Ok(frame.pixels.get(idx..idx + 4) == Some(&expected))
}

fn settle_visible_display() {
    let start = nsec().ok();
    for _ in 0..DISPLAY_SETTLE_MAX_YIELDS {
        if let (Some(start), Ok(now)) = (start, nsec()) {
            if now.saturating_sub(start) >= DISPLAY_SETTLE_NS {
                break;
            }
        }
        let _ = yield_();
    }
}

fn query_cap(handle: Handle) -> Option<CapQuery> {
    let mut query = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    cap_query(handle, &mut query).ok()?;
    Some(query)
}

fn write_windowd_composed_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::VisibleSystemUiEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_composed_row(y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_input_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_composed_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_cursor_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_cursor_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_hover_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_hover_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_keyboard_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_keyboard_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn configure_ramfb(
    fb_base: u64,
    mode: windowd::VisibleBootstrapMode,
) -> Result<(), BootstrapFailure> {
    boot_cfg::ensure_mapped().map_err(|_| BootstrapFailure::FwCfgMap)?;
    if !boot_cfg::fw_cfg_signature_ok() {
        return Err(BootstrapFailure::FwCfgSignature);
    }
    let (select, _) =
        boot_cfg::find_file_select(RAMFB_FILE_NAME).ok_or(BootstrapFailure::RamfbFileMissing)?;
    let dma_vmo = vmo_create(PAGE_SIZE).map_err(|_| BootstrapFailure::DmaVmo)?;
    vmo_map_page(
        dma_vmo,
        DMA_VMO_VA,
        0,
        page_flags::VALID | page_flags::READ | page_flags::WRITE | page_flags::USER,
    )
    .map_err(|_| BootstrapFailure::DmaVmo)?;
    let dma_query = query_cap(dma_vmo).ok_or(BootstrapFailure::InvalidDmaCap)?;
    if dma_query.kind_tag != 1 || dma_query.len < PAGE_SIZE as u64 {
        return Err(BootstrapFailure::InvalidDmaCap);
    }

    // QEMU's etc/ramfb ABI is addr, fourcc, flags, width, height, stride.
    let mut cfg = [0u8; RAMFB_CONFIG_LEN];
    write_be_u64(&mut cfg[0..8], fb_base);
    write_be_u32(&mut cfg[8..12], DRM_FORMAT_ARGB8888);
    write_be_u32(&mut cfg[12..16], 0);
    write_be_u32(&mut cfg[16..20], mode.width);
    write_be_u32(&mut cfg[20..24], mode.height);
    write_be_u32(&mut cfg[24..28], mode.stride);
    vmo_write(dma_vmo, RAMFB_CONFIG_OFFSET, &cfg).map_err(|_| BootstrapFailure::DmaVmo)?;

    let mut access = [0u8; 16];
    let control =
        ((select as u32) << 16) | boot_cfg::FW_CFG_DMA_CTL_SELECT | boot_cfg::FW_CFG_DMA_CTL_WRITE;
    write_be_u32(&mut access[0..4], control);
    write_be_u32(&mut access[4..8], RAMFB_CONFIG_LEN as u32);
    write_be_u64(&mut access[8..16], dma_query.base + RAMFB_CONFIG_OFFSET as u64);
    vmo_write(dma_vmo, DMA_ACCESS_OFFSET, &access).map_err(|_| BootstrapFailure::DmaVmo)?;

    let dma_access_pa = dma_query.base + DMA_ACCESS_OFFSET as u64;
    trigger_dma(dma_access_pa);
    wait_dma_complete().then_some(()).ok_or(BootstrapFailure::DmaFailed)
}

fn wait_dma_complete() -> bool {
    for _ in 0..100_000 {
        let control = unsafe {
            core::ptr::read_volatile((DMA_VMO_VA + DMA_ACCESS_OFFSET) as *const u32).to_be()
        };
        if control == 0 {
            return true;
        }
        if (control & boot_cfg::FW_CFG_DMA_CTL_ERROR) != 0 {
            return false;
        }
    }
    false
}

fn trigger_dma(addr: u64) {
    unsafe {
        core::ptr::write_volatile(boot_cfg::FW_CFG_DMA as *mut u32, ((addr >> 32) as u32).to_be());
        core::ptr::write_volatile((boot_cfg::FW_CFG_DMA + 4) as *mut u32, (addr as u32).to_be());
    }
}

fn write_be_u32(dst: &mut [u8], value: u32) {
    dst.copy_from_slice(&value.to_be_bytes());
}

fn write_be_u64(dst: &mut [u8], value: u64) {
    dst.copy_from_slice(&value.to_be_bytes());
}
