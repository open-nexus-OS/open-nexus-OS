// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Visible QEMU `ramfb` bootstrap path for TASK-0055B/TASK-0055C/TASK-0056B.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder plus `windowd`/`ui_windowd_host` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

extern crate alloc;

use nexus_abi::{
    cap_query, mmio_map, nsec, page_flags, vmo_create, vmo_map_page, vmo_write, yield_, CapQuery,
    Handle,
};
use alloc::vec::Vec;

const VISIBLE_INPUT_PROOF_WIDTH: u32 = 64;
const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;
const VISIBLE_INPUT_SURFACE_X: i32 = 8;
const VISIBLE_INPUT_SURFACE_Y: i32 = 8;
const VISIBLE_INPUT_SURFACE_WIDTH: u32 = 32;
const VISIBLE_INPUT_SURFACE_HEIGHT: u32 = 24;
const VISIBLE_INPUT_CURSOR_START_X: i32 = 12;
const VISIBLE_INPUT_CURSOR_START_Y: i32 = 12;
const VISIBLE_INPUT_CURSOR_END_X: i32 = 36;
const VISIBLE_INPUT_CURSOR_END_Y: i32 = 28;
const VISIBLE_INPUT_INITIAL_BGRA: [u8; 4] = [0x20, 0x80, 0xf0, 0xff];
const LIVE_INPUT_REPEAT_CODE: u32 = 0x04;
const LIVE_INPUT_TOUCH_BOUNDS: u32 = 128;

const FW_CFG_SLOT: Handle = 0x31;
const FW_CFG_MMIO_VA: usize = 0x2001_0000;
const DMA_VMO_VA: usize = 0x2002_0000;
const PAGE_SIZE: usize = 4096;

const FW_CFG_DATA: usize = FW_CFG_MMIO_VA;
const FW_CFG_SELECTOR: usize = FW_CFG_MMIO_VA + 8;
const FW_CFG_DMA: usize = FW_CFG_MMIO_VA + 16;

const FW_CFG_FILE_DIR: u16 = 0x19;
const FW_CFG_DMA_CTL_ERROR: u32 = 1 << 0;
const FW_CFG_DMA_CTL_SELECT: u32 = 1 << 3;
const FW_CFG_DMA_CTL_WRITE: u32 = 1 << 4;

const RAMFB_FILE_NAME: &[u8] = b"etc/ramfb";
const RAMFB_CONFIG_LEN: usize = 28;
const RAMFB_CONFIG_OFFSET: usize = 0;
const DMA_ACCESS_OFFSET: usize = 64;
const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241; // "AR24"
const DISPLAY_SETTLE_NS: u64 = 750_000_000;
const DISPLAY_SETTLE_MAX_YIELDS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapFailure {
    WindowdEvidence,
    VisibleInputEvidence,
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
    pub(crate) systemui: windowd::VisibleSystemUiEvidence,
    pub(crate) visible_input: windowd::UiVisibleInputEvidence,
    pub(crate) live_input: LiveInputEvidence,
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

pub(crate) fn enabled() -> bool {
    option_env!("NEXUS_DISPLAY_BOOTSTRAP") == Some("1")
}

pub(crate) fn run() -> Option<BootstrapEvidence> {
    run_result().ok()
}

pub(crate) fn run_result() -> Result<BootstrapEvidence, BootstrapFailure> {
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
    let (visible_input, live_input) = run_live_visible_input_proof()?;
    let visible_input = windowd::visible_input_marker_postflight_ready(Some(visible_input))
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    write_windowd_visible_cursor_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    write_windowd_visible_hover_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    write_windowd_visible_input_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    Ok(BootstrapEvidence { systemui, visible_input, live_input })
}

fn run_live_visible_input_proof(
) -> Result<(windowd::UiVisibleInputEvidence, LiveInputEvidence), BootstrapFailure> {
    let launcher = windowd::CallerCtx::from_service_metadata(0x55);
    let mut server = windowd::WindowServer::new(windowd::WindowdConfig {
        width: VISIBLE_INPUT_PROOF_WIDTH,
        height: VISIBLE_INPUT_PROOF_HEIGHT,
        hz: 60,
    })
    .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let initial = windowd::SurfaceBuffer::solid(
        launcher,
        50,
        VISIBLE_INPUT_SURFACE_WIDTH,
        VISIBLE_INPUT_SURFACE_HEIGHT,
        VISIBLE_INPUT_INITIAL_BGRA,
    )
    .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let surface =
        server.create_surface(launcher, initial.clone()).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    server
        .queue_buffer(
            launcher,
            surface,
            initial,
            &[windowd::Rect::new(
                0,
                0,
                VISIBLE_INPUT_SURFACE_WIDTH,
                VISIBLE_INPUT_SURFACE_HEIGHT,
            )],
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
    let mut touch_service = touchd::TouchdService::new(bounds, touchd::SyntheticTouchMode::ProofFixture);
    touch_service.register_device(touchd::TouchDeviceId::new(9));

    let config = inputd::InputdConfig::new("de", 100, 10, 64, 1, 1, 96, 32, 0, 0)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let mut input_service =
        inputd::InputdService::new(server, config).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;

    let start_batch = hid_service
        .ingest_mouse_report(
            mouse_id,
            hid::TimestampNs::new(1),
            &[0, VISIBLE_INPUT_CURSOR_START_X as u8, VISIBLE_INPUT_CURSOR_START_Y as u8],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let start_dispatches =
        input_service.apply_hid_batch(&start_batch).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_start_position = input_service
        .router()
        .pointer_position()
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let cursor_frame = input_service
        .router_mut()
        .render_visible_input_frame()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_start_visible = matches!(
        start_dispatches.as_slice(),
        [inputd::InputDispatch::PointerMove { x: VISIBLE_INPUT_CURSOR_START_X, y: VISIBLE_INPUT_CURSOR_START_Y, .. }]
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
    let move_dispatches =
        input_service.apply_hid_batch(&move_batch).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_position = input_service
        .router()
        .pointer_position()
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let hover_frame = input_service
        .router_mut()
        .render_visible_input_frame()
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let cursor_move_visible = cursor_start_visible
        && matches!(
            move_dispatches.as_slice(),
            [inputd::InputDispatch::PointerMove { x: VISIBLE_INPUT_CURSOR_END_X, y: VISIBLE_INPUT_CURSOR_END_Y, .. }]
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
            VISIBLE_INPUT_SURFACE_X,
            VISIBLE_INPUT_SURFACE_Y,
            windowd::VISIBLE_HOVER_BGRA,
        )?;

    let down_batch = hid_service
        .ingest_mouse_report(mouse_id, hid::TimestampNs::new(3), &[0b001, 0, 0])
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let down_dispatches =
        input_service.apply_hid_batch(&down_batch).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let routed_click = matches!(
        down_dispatches.as_slice(),
        [inputd::InputDispatch::PointerDown { x: VISIBLE_INPUT_CURSOR_END_X, y: VISIBLE_INPUT_CURSOR_END_Y, .. }]
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
        input_service
            .set_text_focus(true)
            .map_err(|_| BootstrapFailure::VisibleInputEvidence)?,
        Some(inputd::ImeHook::Show)
    ) && ime_service.show()
        && ime_overlay.show()
        && ime_service.visible()
        && ime_overlay.visible();

    let highlighted = windowd::SurfaceBuffer::solid(
        launcher,
        51,
        VISIBLE_INPUT_SURFACE_WIDTH,
        VISIBLE_INPUT_SURFACE_HEIGHT,
        windowd::VISIBLE_INPUT_CLICK_BGRA,
    )
    .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    input_service
        .router_mut()
        .acquire_back_buffer(launcher, surface, windowd::FrameIndex::new(1), highlighted)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    input_service
        .router_mut()
        .present_frame(
            launcher,
            surface,
            windowd::FrameIndex::new(1),
            &[windowd::Rect::new(
                0,
                0,
                VISIBLE_INPUT_SURFACE_WIDTH,
                VISIBLE_INPUT_SURFACE_HEIGHT,
            )],
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
    let launcher_click_visible =
        pixel_eq(&visible_frame, 24, 18, windowd::VISIBLE_INPUT_CLICK_BGRA)?;

    let keymap_batch = hid_service
        .ingest_keyboard_report(
            keyboard_id,
            hid::TimestampNs::new(4),
            &[0, 0, 0x2f, 0, 0, 0, 0, 0],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_dispatches =
        input_service.apply_hid_batch(&keymap_batch).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_delivered = input_service
        .router_mut()
        .take_input_events(launcher, surface)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let keymap_de_ok = matches!(
        keymap_dispatches.as_slice(),
        [inputd::InputDispatch::Keyboard { output: keymaps::KeyOutput::Text('ü'), repeated: false, .. }]
    ) && keymap_delivered
        .iter()
        .any(|event| matches!(event.kind, windowd::InputEventKind::Keyboard { key_code: 0x2f }));

    let keymap_release = hid_service
        .ingest_keyboard_report(keyboard_id, hid::TimestampNs::new(5), &[0, 0, 0, 0, 0, 0, 0, 0])
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let _ =
        input_service.apply_hid_batch(&keymap_release).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;

    let repeat_press = hid_service
        .ingest_keyboard_report(
            keyboard_id,
            hid::TimestampNs::new(6),
            &[0, 0, LIVE_INPUT_REPEAT_CODE as u8, 0, 0, 0, 0, 0],
        )
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let repeat_start =
        input_service.apply_hid_batch(&repeat_press).map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
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
        .filter(|event| matches!(event.kind, windowd::InputEventKind::Keyboard { key_code: LIVE_INPUT_REPEAT_CODE }))
        .count()
        == 2;

    let touch_events = touch_service
        .synthetic_sequence(1_000)
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let touch_dispatches = touch_events
        .into_iter()
        .map(|event| input_service.apply_touch_event(event).map_err(|_| BootstrapFailure::VisibleInputEvidence))
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
            windowd::InputDelivery { kind: windowd::InputEventKind::TouchDown { x: 20, y: 20 }, .. },
            windowd::InputDelivery { kind: windowd::InputEventKind::TouchMove { x: 28, y: 22 }, .. },
            windowd::InputDelivery { kind: windowd::InputEventKind::TouchUp { x: 28, y: 22 }, .. },
        ]
    );

    let ime_hide = matches!(
        input_service
            .set_text_focus(false)
            .map_err(|_| BootstrapFailure::VisibleInputEvidence)?,
        Some(inputd::ImeHook::Hide)
    ) && ime_service.hide()
        && ime_overlay.hide()
        && !ime_service.visible()
        && !ime_overlay.visible();

    let focused_surface = input_service
        .router()
        .focused_surface()
        .ok_or(BootstrapFailure::VisibleInputEvidence)?;
    let visible_input = windowd::UiVisibleInputEvidence {
        input_visible_on: input_service.router().input_enabled()
            && cursor_move_visible
            && hover_visible
            && focus_visible,
        cursor_move_visible,
        hover_visible,
        focus_visible,
        launcher_click_visible,
        focused_surface,
        cursor_start_position,
        cursor_position,
        scheduled_present,
        cursor_frame: Some(cursor_frame),
        hover_frame: Some(hover_frame),
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

fn pixel_eq(frame: &windowd::Frame, x: i32, y: i32, expected: [u8; 4]) -> Result<bool, BootstrapFailure> {
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

fn configure_ramfb(
    fb_base: u64,
    mode: windowd::VisibleBootstrapMode,
) -> Result<(), BootstrapFailure> {
    mmio_map(FW_CFG_SLOT, FW_CFG_MMIO_VA, 0).map_err(|_| BootstrapFailure::FwCfgMap)?;
    if !fw_cfg_signature_ok() {
        return Err(BootstrapFailure::FwCfgSignature);
    }
    let select = find_ramfb_file_select().ok_or(BootstrapFailure::RamfbFileMissing)?;
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
    let control = ((select as u32) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE;
    write_be_u32(&mut access[0..4], control);
    write_be_u32(&mut access[4..8], RAMFB_CONFIG_LEN as u32);
    write_be_u64(&mut access[8..16], dma_query.base + RAMFB_CONFIG_OFFSET as u64);
    vmo_write(dma_vmo, DMA_ACCESS_OFFSET, &access).map_err(|_| BootstrapFailure::DmaVmo)?;

    let dma_access_pa = dma_query.base + DMA_ACCESS_OFFSET as u64;
    trigger_dma(dma_access_pa);
    wait_dma_complete().then_some(()).ok_or(BootstrapFailure::DmaFailed)
}

fn fw_cfg_signature_ok() -> bool {
    select_fw_cfg(0);
    let mut sig = [0u8; 4];
    for byte in &mut sig {
        *byte = read_fw_cfg_u8();
    }
    &sig == b"QEMU"
}

fn find_ramfb_file_select() -> Option<u16> {
    select_fw_cfg(FW_CFG_FILE_DIR);
    let count = read_fw_cfg_be_u32();
    if count > 128 {
        return None;
    }
    for _ in 0..count {
        let _size = read_fw_cfg_be_u32();
        let select = read_fw_cfg_be_u16();
        let _reserved = read_fw_cfg_be_u16();
        let mut name = [0u8; 56];
        for byte in &mut name {
            *byte = read_fw_cfg_u8();
        }
        if name_matches(&name, RAMFB_FILE_NAME) {
            return Some(select);
        }
    }
    None
}

fn name_matches(name: &[u8; 56], expected: &[u8]) -> bool {
    name.starts_with(expected) && name.get(expected.len()).copied().unwrap_or(1) == 0
}

fn wait_dma_complete() -> bool {
    for _ in 0..100_000 {
        let control = unsafe {
            core::ptr::read_volatile((DMA_VMO_VA + DMA_ACCESS_OFFSET) as *const u32).to_be()
        };
        if control == 0 {
            return true;
        }
        if (control & FW_CFG_DMA_CTL_ERROR) != 0 {
            return false;
        }
    }
    false
}

fn select_fw_cfg(select: u16) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_SELECTOR as *mut u16, select.to_be());
    }
}

fn read_fw_cfg_u8() -> u8 {
    unsafe { core::ptr::read_volatile(FW_CFG_DATA as *const u8) }
}

fn read_fw_cfg_be_u16() -> u16 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    u16::from_be_bytes([b0, b1])
}

fn read_fw_cfg_be_u32() -> u32 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    let b2 = read_fw_cfg_u8();
    let b3 = read_fw_cfg_u8();
    u32::from_be_bytes([b0, b1, b2, b3])
}

fn trigger_dma(addr: u64) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_DMA as *mut u32, ((addr >> 32) as u32).to_be());
        core::ptr::write_volatile((FW_CFG_DMA + 4) as *mut u32, (addr as u32).to_be());
    }
}

fn write_be_u32(dst: &mut [u8], value: u32) {
    dst.copy_from_slice(&value.to_be_bytes());
}

fn write_be_u64(dst: &mut [u8], value: u64) {
    dst.copy_from_slice(&value.to_be_bytes());
}
