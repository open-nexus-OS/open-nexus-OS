// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! ┌──────────────────────── BOUNDARY: windowd = compositor SERVICE ─────────┐
//! │ windowd OWNS ONLY: client surfaces + atlas/VMO lifecycle, damage/tile    │
//! │ tracking, the retained-plane + present pacing + gpud handoff, input      │
//! │ routing, window z-order/focus, and per-frame scene assembly → nexus-gfx. │
//! │ windowd MUST NOT contain: the rasterizer (→ `nexus-gfx`), window chrome/ │
//! │ frames/controls/resize (→ `ui/widgets/window` widget), app/shell UI      │
//! │ content (→ `apps/*` DSL app-hosts), or theme/design values (→            │
//! │ `ui/theme-tokens`, pushed to apps). If you are about to add drawing/UI/   │
//! │ layout/chrome logic HERE, it belongs in a widget or the scene graph — see │
//! │ RFC-0067 + docs/dev/ui/patterns/windowing/windows-as-widgets.md.          │
//! └──────────────────────────────────────────────────────────────────────────┘
//!
//! CONTEXT: OS-lite display server main loop for `windowd` — retained-mode compositor with
//! tile-based damage tracking, two-pass renderer (shadow-pass → content-pass → cursor),
//! SDF anti-aliased shapes, backdrop blur via nexus-effects, coalesced cursor damage,
//! paint-only fast-path, and GPU-first rendering pipeline (Phase 6c).
//! control/data-plane separation: windowd heap = control plane,
//! shared 16MB VMO = data plane (4-plane: wallpaper / retained-scene / slot-A / slot-B).
//! gpud executes BlitSurface/FillSdfRoundedRect/BlurBackdrop/DrawTiles commands.
//! Part of TASK-0055/0056/0058/0059/0062.
//!
//! OWNERS: @ui
//! STATUS: Phase 6c closed (2026-06-05) — GPU wallpaper path, double-height VMO,
//!   deadline-driven VSync, honest fences, 10× vmo_write reduction
//! API_STABILITY: Unstable
//!
//! ARCHITECTURE:
//!   - Two-pass renderer: `compute_shadow_row` (shadow, zero-allocation),
//!     `draw_proof_surface_row` (content + backdrop blur, zero-allocation)
//!   - Tile-based damage: `TileMap` (64x64 tiles, 260 tiles) with `has_dirty_in_row_range`
//!     gating band writes in `write_rows`
//!   - Retained layer cache: `LayerCache` (insert/get/invalidate) with per-box blit
//!   - Cursor damage coalescing: old/new cursor bounds merge into the normal
//!     band flush path to avoid restore/new-frame flicker
//!   - Paint-only fast-path: `paint_only` flag skips non-paint boxes and backdrop blur
//!   - Zero-copy: `shadow_scratch` + `blur_row_buf` pre-allocated once
//!   - SDF integration: `fill_sdf_circle_row`, `fill_sdf_rounded_rect_row`
//!   - IPC: `KernelServer` receive loop for `OP_GET_VISIBLE_STATE`, `OP_UPDATE_VISIBLE_STATE`
//!
//! DEPENDENCIES:
//!   - nexus-layout, nexus-layout-types: layout computation
//!   - nexus-effects: shadow types, cache infrastructure (blur is zero-allocation inline)
//!   - nexus-sdf: rendering primitives
//!   - nexus-abi, nexus-ipc: kernel IPC
//!   - input-live-protocol: VisibleState wire format
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

// RFC-0067 P5-Final G3: CPU glass blur (`backdrop`) deleted — GPU-rendered.
mod damage;
mod filter;
mod framebuffer_grant;
#[cfg(nexus_env = "os")]
mod loop_telemetry;
pub(crate) mod material_glass;
mod reply_route;
mod runtime;
mod scene;
pub(crate) mod shell_window;
pub(crate) mod source;
#[cfg(test)]
mod tests;
mod tile_map;
mod types;

use damage::*;
use runtime::*;
use types::*;

extern crate alloc;

use core::fmt::Write as _;

use input_live_protocol::{
    decode_update_visible_state, encode_display_space, encode_status, encode_visible_state_frame,
    frame_has_op, OP_GET_DISPLAY_SPACE, OP_GET_VISIBLE_STATE, OP_UPDATE_VISIBLE_STATE,
    STATUS_MALFORMED, STATUS_UNSUPPORTED,
};
use nexus_abi::{debug_println, debug_trace, nsec};
use nexus_display_proto::layout;
use nexus_ipc::{IpcError, KernelServer, Wait};

use crate::markers::{ready_marker, WALLPAPER_FAIL};

use crate::telemetry::WindowdDisplayTelemetryReport;

// Phase 6c: control-plane / data-plane separation. All pixel data lives in the shared
// framebuffer — gpud's, granted to windowd (RFC-0098 C7) — laid out by
// `nexus_display_proto::layout`: plane 0 the wallpaper source, plane 1 the retained scene,
// plane 2 the display plane (frame ring slot A), plane 3 slot B (the blur cache), then the atlas.
/// The shared-VMO layout maximum — the resource budget, not a default mode. One home:
/// `nexus_display_proto::LAYOUT_MAX` (TASK-0324 P6-a; it lived three times before).
pub(crate) const DISPLAY_WIDTH: u32 = nexus_display_proto::LAYOUT_MAX.0;
pub(crate) const DISPLAY_HEIGHT: u32 = nexus_display_proto::LAYOUT_MAX.1;
// Byte twins of the live *_ROW_OFFSET values below — documented plane-layout
// contract (RFC-0067 retained-plane); kept for the layout math even where only
// the row form is consumed today.
#[allow(dead_code)]
pub(crate) const DISPLAY_OFFSET_BYTES: usize = layout::row_offset_bytes(layout::DISPLAY_ROW);
#[allow(dead_code)]
pub(crate) const DISPLAY_SLOT_B_OFFSET_BYTES: usize = layout::row_offset_bytes(layout::SLOT_B_ROW);
/// Plane 1 — retained scene. The CPU compositor renders the full cursor-free
/// scene (wallpaper + panels + text + glass) here. gpud blits damage regions
/// from this plane to the display plane per frame and overlays the cursor.
pub(crate) const RETAINED_OFFSET_BYTES: usize = layout::row_offset_bytes(layout::RETAINED_ROW);
/// Row offset of the retained plane within the VMO. Used as the BlitSurface source row base.
pub(crate) const RETAINED_ROW_OFFSET: u32 = layout::RETAINED_ROW;
/// Absolute VMO row where the display plane starts. Used as the BlitAbsolute source/dst for
/// blur cache writes.
pub(crate) const DISPLAY_ROW_OFFSET: u32 = layout::DISPLAY_ROW;
/// Absolute VMO row where Plane 3 (Slot B) starts — repurposed as blur cache. (Documented
/// plane-layout contract; the blur cache writers address Plane 3 through gpud blits today.)
#[allow(dead_code)]
pub(crate) const BLUR_CACHE_ROW_OFFSET: u32 = layout::SLOT_B_ROW;
pub(crate) const PROOF_PANEL_H: u32 = 260;

/// Shell-P2b: when `true`, source `proof_layouts` from the flat desktop-shell
/// scene and suppress the rich proof/glass overlays. The flat-rect render was a
/// regression (no glass/shadow/rounding), so this is `false`: we keep the rich
/// glass UI (chat window + buttons + sidebar) and add a real glass topbar instead.
/// Kept as a switch for the layout-driven path.
pub(crate) const USE_DESKTOP_SHELL: bool = false;

// The former `SHELL_TOPBAR` / `SHELL_SIDEPANEL` compile-time constants are gone:
// the glass topbar + side panel chrome is now driven at runtime by the shell
// configuration resolved from SystemUI's manifest registry
// (`DisplayServerRuntime.shell_config.desktop_chrome`), so the active shell —
// not a hardcoded constant — decides whether the desktop chrome is composited.

pub(crate) const LIVE_FILTER_VARIANTS: [&str; 5] = ["", "a", "ap", "c", "b"];
#[cfg(nexus_env = "os")]
pub(crate) const ROW_WRITE_CHUNK: usize = 40;
#[cfg(not(nexus_env = "os"))]
pub(crate) const ROW_WRITE_CHUNK: usize = 32;
// Drain a generous burst of input/IPC per loop iteration so a flood of pointer
// events (hidrawd can emit ~800/s during a drag) is consumed in one frame and the
// queue can't grow stale — the wheel deltas among them are coalesced into a single
// scroll step (`commit_scroll_input`), so a bigger batch costs no extra scrolling.
pub(crate) const IPC_BATCH_LIMIT: usize = 64;
// Backdrop/glass cache budget contract (blur-cache architecture); sizes are
// documented dimensions even where the current present path bypasses a cache.
#[allow(dead_code)]
pub(crate) const BACKDROP_CACHE_ENTRIES: usize = 4;
// C1: dimensions inlined (was `proof_panel_spec` PANEL_WIDTH 610 / PANEL_HEIGHT
// 260 / +GAP 16 +FILTER_PANEL_WIDTH 200 = 826). These now size the backdrop/
// layer caches only; the proof panel itself is deleted.
#[allow(dead_code)]
pub(crate) const BACKDROP_CACHE_MAX_WIDTH: usize = 610;
pub(crate) const COMBINED_PANEL_WIDTH: usize = 826;
#[allow(dead_code)]
pub(crate) const COMBINED_PANEL_HEIGHT: usize = 260;
// Glass-layer downscale budget contract (blur-cache architecture); consumed by
// the cache sizing docs — the live glass path samples via gpud today.
#[allow(dead_code)]
#[cfg(nexus_env = "os")]
pub(crate) const GLASS_LAYER_SCALE: u32 = 8;
#[allow(dead_code)]
#[cfg(not(nexus_env = "os"))]
pub(crate) const GLASS_LAYER_SCALE: u32 = 4;
#[allow(dead_code)]
pub(crate) const GLASS_LAYER_MAX_WIDTH: usize =
    COMBINED_PANEL_WIDTH.div_ceil(GLASS_LAYER_SCALE as usize);
#[allow(dead_code)]
pub(crate) const GLASS_LAYER_MAX_HEIGHT: usize =
    COMBINED_PANEL_HEIGHT.div_ceil(GLASS_LAYER_SCALE as usize);
#[allow(dead_code)]
pub(crate) const GLASS_LAYER_MAX_BYTES: usize = GLASS_LAYER_MAX_WIDTH * GLASS_LAYER_MAX_HEIGHT * 4;
pub(crate) const DARK_GLASS_BLUR_RADIUS: u32 = 20;
// Path/layer cache budget contract (blur-cache architecture): documented
// bounded-memory ceilings for the rounded-corner path cache.
#[allow(dead_code)]
pub(crate) const PATH_CACHE_ENTRIES: usize = 2;
pub(crate) const PATH_CACHE_MAX_SIDE: usize = 16;
#[allow(dead_code)]
pub(crate) const PATH_CACHE_MAX_PIXELS: usize = PATH_CACHE_MAX_SIDE * PATH_CACHE_MAX_SIDE * 4;
#[allow(dead_code)]
pub(crate) const LAYER_CACHE_MAX_BYTES: usize = 4 * 1024;
#[allow(dead_code)]
pub(crate) const LAYER_CACHE_MAX_LAYER_BYTES: usize = PATH_CACHE_MAX_PIXELS;
pub(crate) const TILE_SIZE: u32 = 64;
/// The damage tile grid covers the layout's maximum (`nexus_display_proto::LAYOUT_MAX`): the base
/// pass paints only rows with a dirty tile, so a row outside the grid is never painted (the
/// 1280x800 grid left every row past 832 unpainted at 1920x1080 — TASK-0251 P2a step 3).
pub(crate) const TILES_X: usize = DISPLAY_WIDTH.div_ceil(TILE_SIZE) as usize;
pub(crate) const TILES_Y: usize = DISPLAY_HEIGHT.div_ceil(TILE_SIZE) as usize;
// Every build of the compositor proves the grid covers the layout (this module is OS-only, so
// a unit test of it would never run).
const _: () = assert!(TILES_X as u32 * TILE_SIZE >= DISPLAY_WIDTH);
const _: () = assert!(TILES_Y as u32 * TILE_SIZE >= DISPLAY_HEIGHT);
pub(crate) const TILE_COUNT: usize = TILES_X * TILES_Y;
pub(crate) const TILE_DIRTY_WORDS: usize = (TILE_COUNT + 63) / 64;
// Shadow arena/scratch budget contract: documented bounded-memory ceilings for
// the zero-alloc shadow blur (nexus-effects ShadowArena) integration.
#[allow(dead_code)]
#[cfg(nexus_env = "os")]
pub(crate) const WINDOWD_SHADOW_ARENA_SIZE: usize = 8 * 1024;
#[allow(dead_code)]
#[cfg(not(nexus_env = "os"))]
pub(crate) const WINDOWD_SHADOW_ARENA_SIZE: usize = 16 * 1024;
#[allow(dead_code)]
pub(crate) const COL_SCRATCH_SIZE: usize = WINDOWD_SHADOW_ARENA_SIZE;
#[allow(dead_code)]
pub(crate) const SHADOW_BOX_CACHE_ENTRIES: usize = 8;
#[allow(dead_code)]
pub(crate) const SHADOW_CACHE_MAX_DOWNSCALE: u8 = 16;
pub(crate) const DARK_GLASS_SATURATION_PERCENT: u32 = 140;
#[cfg(nexus_env = "os")]
use loop_telemetry::LoopTelemetry;

/// Dispatch ONE client request frame (input state, surface create/present/
/// destroy/events, or an unknown op). The SINGLE source of truth for the
/// windowd server protocol — called from BOTH recv sites (the drain batch AND
/// the idle blocking recv). The idle recv previously handled only the two
/// visible-state ops and answered everything else UNSUPPORTED: a client
/// surface present arriving while the desktop was idle (the exact state after
/// an app window opens and the user taps) was silently dropped — the "+ reacts
/// only once" bug. Factoring both sites through here makes a missing branch
/// impossible.
#[cfg(nexus_env = "os")]
fn dispatch_client_frame(
    runtime: &mut DisplayServerRuntime,
    server: &KernelServer,
    frame: &[u8],
    mut moved_cap: Option<nexus_ipc::ReplyCap>,
    sender_sid: u64,
) {
    // RFC-0075: imed pushes arrive on the same server endpoint but speak the
    // `'I','E'` protocol — discriminate by MAGIC before the op switch (the
    // `'I','N'` op space below would misread them). Fire-and-forget: no reply.
    if frame.len() >= 4 && frame[0] == b'I' && frame[1] == b'E' {
        runtime.handle_imed_push(frame, sender_sid);
        return;
    }
    // RFC-0083: settingsd watch events (`'S','T'` OP_EVENT) — the watch caps
    // are clones of OUR OWN server send half, so a settings change wakes an
    // idle compositor as ordinary inbound IPC. Fire-and-forget: no reply.
    if frame.len() >= 4 && frame[0] == b'S' && frame[1] == b'T' {
        runtime.handle_settings_event(frame);
        return;
    }
    if frame_has_op(frame, OP_GET_DISPLAY_SPACE) {
        // RFC-0098 C7: inputd's one ask — answered on the reply inbox it moved, never on the
        // shared response endpoint (several services read that one).
        let (w, h) = runtime.visible_mode();
        match moved_cap.take() {
            Some(reply) => reply_route::answer(
                server,
                Some(reply),
                &encode_display_space(w, h),
                false,
                OP_GET_DISPLAY_SPACE,
            ),
            None => {
                let _ = debug_println("windowd: FAIL display space ask without a reply inbox");
            }
        }
    } else if frame_has_op(frame, OP_GET_VISIBLE_STATE) {
        let response = encode_visible_state_frame(runtime.visible_state());
        reply_route::answer(server, moved_cap.take(), &response, false, OP_GET_VISIBLE_STATE);
    } else if frame_has_op(frame, OP_UPDATE_VISIBLE_STATE) {
        // Frame-aligned coalescing: STAGE the update (latest sample wins,
        // wheel sums); applied ONCE per frame by apply_staged_input. Reply
        // immediately so inputd is never blocked.
        let status = match decode_update_visible_state(frame) {
            Some(state) => runtime.stage_input_state(state),
            None => STATUS_MALFORMED,
        };
        if let Some(reply) = moved_cap.take() {
            let response = encode_status(OP_UPDATE_VISIBLE_STATE, status);
            let _ = reply.reply_and_close_wait(&response, Wait::Blocking);
        }
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_EVENTS)
    {
        // ADR-0042 per-app event channel: the moved capability is the
        // channel's SEND half, attached by the APP-HOST ITSELF and tagged with
        // its nonce — SURFACE_CREATE repeats the nonce, so windowd binds
        // channel↔surface deterministically (never by arrival order; N
        // app-hosts connect concurrently). No reply; the marker is the proof.
        let nonce = nexus_display_proto::client_surface::decode_surface_events(frame);
        let send_slot = moved_cap.take().map(|cap| {
            let slot = cap.slot();
            core::mem::forget(cap); // keep the slot alive (no close)
            slot
        });
        runtime.attach_app_event_channel(send_slot, nonce);
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_CREATE)
    {
        // ADR-0042: the moved capability IS the app's surface VMO (gpud-attach
        // pattern), NOT a reply cap. Retain its slot; the ack returns over the
        // app's dedicated event channel (fallback: shared response endpoint).
        let vmo_slot = moved_cap.take().map(|cap| {
            let slot = cap.slot();
            core::mem::forget(cap); // keep the slot alive (no close)
            slot
        });
        // Ack routing BY NONCE (the create frame carries it): the reply must
        // reach the CREATING client's own event channel — `send_app_frame`
        // (the floating channel) sent DESKTOP create-acks into the void and
        // the shell/greeter app-host hung in its ack wait forever.
        let nonce = nexus_display_proto::client_surface::decode_surface_create(frame).map(|t| t.7);
        let ack = runtime.handle_surface_create(frame, vmo_slot, sender_sid);
        let delivered = match nonce {
            Some(n) => runtime.send_frame_for_nonce(n, &ack),
            None => false,
        };
        // Every client owns a nonce-bound channel; a missing one falls back to
        // the shared response endpoint (bring-up paths).
        reply_route::answer(
            server,
            None,
            &ack,
            delivered,
            nexus_display_proto::client_surface::OP_SURFACE_CREATE,
        );
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_PRESENT)
    {
        // Ack routing BY SURFACE OWNER: a desktop present acks on the desktop
        // channel, a floating present on the app channel.
        let sid = nexus_display_proto::client_surface::decode_surface_present(frame)
            .map(|(id, _, _, _)| id);
        let ack = runtime.handle_surface_present(frame);
        let delivered = match sid {
            Some(id) => runtime.send_surface_frame(id, &ack),
            None => false,
        };
        let op = frame.get(3).copied().unwrap_or(0);
        reply_route::answer(server, moved_cap.take(), &ack, delivered, op);
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_DESTROY)
    {
        // Same owner routing as present — decode BEFORE the destroy drops the
        // surface bookkeeping.
        let sid = nexus_display_proto::client_surface::decode_surface_destroy(frame);
        let was_desktop = sid.is_some() && sid == runtime.desktop_surface_id_for_ack();
        // Resolve the owning window BEFORE the destroy clears the binding —
        // the ack still rides that window's dedicated channel.
        let app_idx = sid.and_then(|id| runtime.app_index_by_surface(id));
        let ack = runtime.handle_surface_destroy(frame);
        let delivered = if was_desktop {
            runtime.send_desktop_ack(&ack)
        } else {
            app_idx.map(|i| runtime.send_app_frame(i, &ack)).unwrap_or(false)
        };
        let op = frame.get(3).copied().unwrap_or(0);
        reply_route::answer(server, moved_cap.take(), &ack, delivered, op);
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_LAYERS)
    {
        // R1 layer seam: the app declares its material-tagged glass regions.
        // Data-only frame (no moved cap, no reply); the next present composites.
        runtime.handle_surface_layers(frame);
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::client_surface::OP_SURFACE_FRAME_REQ)
    {
        // Frame pulse request (Choreographer one-shot): armed here, answered
        // after the next composited frame in the present loop.
        runtime.handle_surface_frame_req(frame);
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_CONTROL)
    {
        // Presentation control from a shell surface; applied live + persisted.
        runtime.handle_surface_control(frame, sender_sid);
    } else if frame.get(3).copied() == Some(nexus_display_proto::OP_SURFACE_TASKBAR) {
        runtime.handle_surface_taskbar(frame, sender_sid); // RFC-0086
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::surface_text::OP_SURFACE_TEXT_FOCUS)
    {
        // Widget text-focus announcement (RFC-0075): record + relay to imed.
        // Data-only frame; identity-resolved inside (sender's own surface).
        runtime.handle_surface_text_focus(frame, sender_sid);
    } else if frame.get(3).copied()
        == Some(nexus_display_proto::surface_text::OP_SURFACE_CURSOR_HINT)
    {
        // Pointer-cursor hint (I-beam over editable fields): the app owns
        // hover semantics inside its surface. Data-only frame.
        runtime.handle_surface_cursor_hint(frame);
    } else if frame.get(3).copied() == Some(nexus_display_proto::client_surface::OP_SURFACE_INTENT)
    {
        // Window intent (before create): the WM stores it + answers the content
        // rect on the app event channel. Data-only frame (no moved cap here).
        runtime.handle_surface_intent(frame);
    } else {
        let op = frame.get(3).copied().unwrap_or(0);
        let response = encode_status(op, STATUS_UNSUPPORTED);
        reply_route::answer(server, moved_cap.take(), &response, false, op);
    }
}

pub fn service_main_loop() -> Result<(), &'static str> {
    // Verdict folding: fold windowd's scattered `debug_println` bring-up markers (route/shell/
    // wallpaper/handoff/present…) into one `windowd N/N` grid line in interactive boots. Flushed
    // once the present scheduler is on; FAIL lines print live; proof boots emit everything raw.
    nexus_abi::service_verdict_arm();
    // The declared server pair (TASK-0324 P4), pinned before this task runs. No route ask at
    // start-up (P7-b): an ask has no clock and init may be blocked in a synchronous exchange
    // with a service that, in turn, waits for THIS server — the ask made that a deadlock.
    let server = KernelServer::new_with_slots(
        nexus_service_topology::slots::windowd::SERVER.recv,
        nexus_service_topology::slots::windowd::SERVER.send,
    )
    .map_err(|_| "windowd: init fail kernel-server")?;
    // RFC-0098 C7: the mode and the framebuffer are gpud's — asked once, before the compositor
    // exists, because the compositor is built at the granted mode. No default stands in: a
    // stack without gpud is named (`windowd: display none (…)`) and runs display-less.
    let grant = framebuffer_grant::request();
    let (visible_w, visible_h) = grant.mode;
    let mut runtime = match DisplayServerRuntime::new_with_mode(visible_w, visible_h) {
        Ok(rt) => {
            let _ = nexus_service_entry::ready(&ready_marker(visible_w, visible_h));
            rt
        }
        Err(_) => {
            let _ = debug_println("windowd: init fail display-server (wallpaper?)");
            let _ = debug_println(WALLPAPER_FAIL);
            return Err("windowd: init fail display-server");
        }
    };

    // The granted framebuffer: the wallpaper into plane 0, the first frame, then the attach —
    // gpud scans it out. Display-less (no grant): nothing to write, nothing to attach.
    if let Some(handle) = grant.framebuffer {
        let _ = debug_println("windowd: backend=gpu");
        runtime.register_framebuffer_vmo(handle);
        let _ = runtime.write_source_frame_to_vmo();
        let _ = runtime.process_deferred_framebuffer_write();
    }

    let mut recv_frame = [0u8; 512];
    // Loop-cadence telemetry (hyper-smooth + SMP-flicker diagnosis): the ~1s
    // `windowd: loop hz=` window with NACK counters — see `loop_telemetry.rs`.
    #[cfg(nexus_env = "os")]
    let mut loop_stats = LoopTelemetry::new();
    // TASK-0324 P7-c: ONE waitset (RFC-0033) over every input this loop has — its server
    // endpoint, gpud's replies (every present ack is a display-ring COMPLETION: the frame
    // clock), the settings push channel, the session push channel and abilitymgr's replies.
    // No timer cap, no idle tick, no self-paced fallback: an animation runs by presenting a
    // frame, waking on its completion and presenting the next; idle is zero wakes.
    #[cfg(nexus_env = "os")]
    let waitset = build_waitset(&server);
    #[cfg(nexus_env = "os")]
    if waitset.is_none() {
        let _ = debug_println("windowd: FAIL waitset (blocking on the server endpoint alone)");
    }
    #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
    {
        // Session state is PUSHED from now on (sessiond drains this once it starts, after
        // our `DisplayReady` report — the pushes land on a waitset member).
        let _ = debug_println(if crate::session_client::subscribe_session_watch() {
            "windowd: session watch subscribed"
        } else {
            "windowd: FAIL session watch subscribe"
        });
    }
    loop {
        // 1. Completions first: they clock every animation and free ring slots.
        let completed = runtime.drain_gpud_replies();
        #[cfg(nexus_env = "os")]
        loop_stats.tick(nexus_abi::nsec().unwrap_or(0), &runtime);
        #[cfg(nexus_env = "os")]
        let _ = runtime.process_deferred_framebuffer_write();
        if completed {
            runtime.on_frame_completed(nsec().unwrap_or(0));
        }
        // 2. Requests, in a bounded batch.
        for _ in 0..IPC_BATCH_LIMIT {
            match server.recv_request_with_meta_into(Wait::NonBlocking, &mut recv_frame) {
                Ok((frame_len, sender_sid, mut moved_cap)) => {
                    let frame = &recv_frame[..frame_len];
                    dispatch_client_frame(
                        &mut runtime,
                        &server,
                        frame,
                        moved_cap.take(),
                        sender_sid,
                    );
                }
                Err(IpcError::WouldBlock)
                | Err(IpcError::Timeout)
                | Err(IpcError::Disconnected)
                | Err(IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => break,
                Err(_) => {}
            }
        }
        // 3. Frame-aligned input: apply the staged sample (latest cursor/buttons + summed
        //    wheel) ONCE per frame, independent of how many raw events arrived.
        let applied = runtime.apply_staged_input();
        // 4. Pushes: settings (RFC-0083), session state (P7-c) and launch replies — every one
        //    a waitset member, drained here, never polled on a cadence.
        runtime.pump_region_watch();
        runtime.drain_settings_events();
        #[cfg(all(nexus_env = "os", target_os = "none"))]
        runtime.drain_session_pushes();
        #[cfg(nexus_env = "os")]
        runtime.drain_launch_replies();
        runtime.pump_presentation();
        #[cfg(nexus_env = "os")]
        loop_stats.note_apply(applied);
        #[cfg(not(nexus_env = "os"))]
        let _ = applied;
        // 5. Present when the ring has a slot (skip while the handoff is pending — the VMO
        //    must arrive at gpud before any present-damage frame).
        if !runtime.is_handoff_pending() {
            runtime.keep_frame_clock_alive();
            if let Err(err) = runtime.flush_pending_damage_if_slot_free() {
                let _ = debug_println(flush_error_label(err));
            }
        }
        // Frame pulses AFTER the frame's compose/present work: animating clients tick their
        // physics on the REAL frame cadence.
        runtime.flush_frame_pulses();
        // 6. WAIT — no clock. Any member with a queued message wakes us; nothing pending
        //    anywhere means zero CPU until the next event.
        #[cfg(nexus_env = "os")]
        {
            match waitset {
                Some(ws) => {
                    let _ = nexus_abi::waitset_wait(ws, 0);
                }
                None => {
                    if let Ok((frame_len, sender_sid, mut moved_cap)) =
                        server.recv_request_with_meta_into(Wait::Blocking, &mut recv_frame)
                    {
                        let frame = &recv_frame[..frame_len];
                        dispatch_client_frame(
                            &mut runtime,
                            &server,
                            frame,
                            moved_cap.take(),
                            sender_sid,
                        );
                    }
                }
            }
        }
        #[cfg(not(nexus_env = "os"))]
        {
            if let Ok((frame_len, sender_sid, mut moved_cap)) =
                server.recv_request_with_meta_into(Wait::Blocking, &mut recv_frame)
            {
                let frame = &recv_frame[..frame_len];
                dispatch_client_frame(&mut runtime, &server, frame, moved_cap.take(), sender_sid);
            }
        }
    }
}

/// The compositor loop's waitset (TASK-0324 P7-c): its server endpoint, gpud's replies,
/// the settings and session push channels and abilitymgr's replies — the declared slots
/// (`nexus-service-topology`), so the members exist before this task runs. `None` = the
/// kernel refused (reported by the caller; the loop then blocks on the server alone).
#[cfg(nexus_env = "os")]
fn build_waitset(server: &KernelServer) -> Option<u32> {
    use nexus_service_topology::slots::windowd as topo;
    let ws = nexus_abi::waitset_create().ok()?;
    let (server_recv, _) = server.slots();
    for slot in [
        server_recv,
        topo::GPUD.recv,
        topo::WATCH_RECV,
        topo::SESSION_WATCH_RECV,
        topo::ABILITYMGR.recv,
    ] {
        nexus_abi::waitset_add(ws, slot).ok()?;
    }
    Some(ws)
}

fn emit_windowd_telemetry(report: WindowdDisplayTelemetryReport) {
    let mut line = FixedDebugLine::new();
    if write!(
        &mut line,
        "fps: windowd compose_hz={} present_hz={} coalesced={} dropped={} damage_px={} avg_render_us={} max_render_us={} spin_hz={}",
        report.compose_hz,
        report.present_hz,
        report.coalesced_events,
        report.dropped_events,
        report.damage_pixels,
        report.avg_render_us,
        report.max_render_us,
        report.spin_hz
    )
    .is_err()
    {
        return;
    }
    if let Some(line) = line.as_str() {
        // Periodic compositor counters: off by default, one runtime flag away. Phase 3
        // promotes these to metricsd counters.
        let _ = debug_trace(line);
    }
}
