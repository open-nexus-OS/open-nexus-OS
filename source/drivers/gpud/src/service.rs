// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's service entry and its request loop (RFC-0093 §5) over ONE display
//! (`backend::display::Display`): the virtio GPU on QEMU, the board's display controller when init
//! granted it instead (TASK-0251 P2a). The loop owns the wire — it decodes every request,
//! validates a present's commands, derives its damage, prints the chain trace and the present
//! statistics, latches the reveal and encodes every answer; the display owns its device.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_abi::{debug_println, mmio_map_auto, nsec, yield_, AbiError};
use nexus_display_proto::PRESENT_HEADER_LEN;
use nexus_ipc::{KernelServer, Server as _, Wait};

use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::CommittedBuffer;

use crate::backend::display::Display;
use crate::backend::VirtioGpuBackend;
use crate::markers::{
    GPUD_CURSOR_ON, GPUD_DISPLAY_READY, GPUD_MMIO_FAULT, GPUD_NO_DEVICE, GPUD_READY,
    GPUD_SCANOUT_MODE, GPUD_SCANOUT_OK, GPUD_VIRTIO_GPU_PROBED,
};
use crate::service_stats::{emit_handoff_timing, PresentStats};

// Wire opcodes/status/cursor magics are the shared SSOT in `nexus-display-proto`
// (Gate 2) — re-exported here under the historical local names so call sites and
// `crate::service::OP_*` references stay unchanged. Values live in one place now.
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = nexus_display_proto::OP_SUBMIT_ANIMATION_FRAME;
pub const OP_MOVE_CURSOR: u8 = nexus_display_proto::OP_MOVE_CURSOR;
pub const OP_SET_FRAMEBUFFER_VMO: u8 = nexus_display_proto::OP_SET_FRAMEBUFFER_VMO;
pub const OP_PRESENT_DAMAGE: u8 = nexus_display_proto::OP_PRESENT_DAMAGE;
pub const OP_UPLOAD_CURSOR: u8 = nexus_display_proto::OP_UPLOAD_CURSOR;
/// Scroll fast path: windowd sends the chat layer's new absolute atlas source row
/// (5 bytes: op + u32). gpud re-samples the retained scrollable layer at that row
/// and re-composites on the GPU (~54µs) — no windowd CPU compose, the analogue of
/// `OP_MOVE_CURSOR`.
pub const OP_SET_LAYER_SCROLL: u8 = nexus_display_proto::OP_SET_LAYER_SCROLL;
pub const OP_SET_LAYER_TRANSFORM: u8 = nexus_display_proto::OP_SET_LAYER_TRANSFORM;
/// Upload a real icon sprite to composite as a GPU layer in the virgl buildup.
/// Payload: op + tex_w(u32) + tex_h(u32) + dst_x(u32) + dst_y(u32) + dst_w(u32) +
/// dst_h(u32) + BGRA pixels. The texture may be rendered at 2× (supersampled) and
/// is GPU-downscaled to dst_w×dst_h. Stored + composited like the cursor sprite.
pub const OP_UPLOAD_ICON: u8 = nexus_display_proto::OP_UPLOAD_ICON;
/// Cursor shape cache: fill a slot (no arming) / switch the active sprite.
/// Together they replace the blocking per-shape-change 4KB re-upload with a
/// 2-byte fire-and-forget select (hyper-smooth pointer at window edges).
pub const OP_UPLOAD_CURSOR_SHAPE: u8 = nexus_display_proto::OP_UPLOAD_CURSOR_SHAPE;
pub const OP_SELECT_CURSOR_SHAPE: u8 = nexus_display_proto::OP_SELECT_CURSOR_SHAPE;
pub const STATUS_OK: u8 = nexus_display_proto::STATUS_OK;
pub const STATUS_MALFORMED: u8 = nexus_display_proto::STATUS_MALFORMED;
pub const STATUS_DEVICE_ERROR: u8 = nexus_display_proto::STATUS_DEVICE_ERROR;

const GPU_MMIO_CAP_SLOT: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
const GPU_MMIO_LEN: usize = 0x1000;
const GPUD_RECV_SLOT: u32 = nexus_service_topology::slots::gpud::SERVER.recv;
const GPUD_SEND_SLOT: u32 = nexus_service_topology::slots::gpud::SERVER.send;
/// Endpoint cap slot the GPU IRQ is routed to: gpud's idle control-reply endpoint
/// (slot 2 — the same idle endpoint hidrawd reuses for input IRQs). Deliberately
/// NOT the windowd↔gpud server endpoint (slot 3): binding a notification source
/// there would intercept windowd's present commands and break the channel.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_IRQ_NOTIFY_SLOT: u32 = nexus_service_topology::CTRL_SLOTS.recv;
pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    // Build provenance FIRST and RAW in every boot mode (entry already armed folding; debug_write never folds).
    let _ = nexus_abi::debug_write(crate::markers::GPUD_FEATURES_LINE.as_bytes());
    // Verdict folding: fold gpud's scattered `debug_println` bring-up markers (virgl ready/shader/
    // draw/gradient/scanout/…) into one `gpud N/N` grid line in interactive boots. Flushed at
    // GPUD_READY below; FAIL lines still print live; proof boots emit everything raw.
    nexus_abi::service_verdict_arm();
    // TASK-0251 P2: the board grants its display plane instead of a GPU. The controller needs
    // no frame clock: it scans by itself and nothing is self-presented.
    if crate::backend::dc::granted() {
        let mut display = crate::backend::dc::DcDisplay::bring_up();
        let server = bind_server()?;
        // RFC-0101 §8: the controller's vertical-blank interrupt on the idle control-reply
        // endpoint (the slot the GPU's IRQ takes on QEMU), a waitset member next to the server
        // endpoint — no timer: the display's own blank is the frame clock.
        // TASK-0329 G1 (RFC-0101 Phase 2): the board's GPU beside its display — brought online
        // before the display's clock is armed (its own notify endpoint joins the waitset), the
        // rungs `gpud: gpu glue|core|fw|mmu|fence ok (`; a FAIL leaves the display running.
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let mut gpu = crate::gpu::granted()
            .then(|| crate::gpu::Gpu::bring_up(nexus_service_topology::slots::gpud::GPU_IRQ))
            .flatten()
            .map(crate::gpu::Device::new);
        // TASK-0169B (RFC-0101 Phase 3): the first quad into a plane of the ring — the open
        // driver's two transfer jobs for this core, captured on the host, replayed through
        // `GfxDevice`: `gpud: gpu fill ok (`, `gpud: gpu quad ok (`; a FAIL leaves the display.
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        if let Some(dev) = gpu.as_mut() {
            crate::gpu::quad::rung(dev, &mut display);
            // TASK-0215 5a (RFC-0101 Phase 4): the render queue — the captured render pass with
            // the render context, free lists and dataset built here: `gpud: gpu render ok (`.
            // Its context serves the render rungs after it.
            let mut rungs = crate::gpu::render_quad::rung(dev, &mut display);
            if let Some(rc) = rungs.as_mut() {
                // TASK-0215 5c-2 P4d (RFC-0101 §7b): the compositor's pipeline library taken for
                // the context — validated, its programs placed: `gpud: gpu library ok (`.
                crate::gpu::render_library::rung(dev, rc);
                // TASK-0215 5b: dynamic geometry — three quads at the device's positions and
                // tints, the per-draw blocks and records written by the device: `gpud: gpu
                // quads ok (`.
                crate::gpu::render_quads::rung(dev, &mut display, rc);
                // TASK-0215 5c-1: the cost of a pass taken apart, one factor at a time — the GPU
                // cache policy, pixel merging, the render area: `gpud: gpu cost (` lines.
                crate::gpu::render_cost::rung(dev, &mut display, rc);
            }
            // TASK-0215 5c-3 P2: the rungs' context torn down by the firmware's cleanup, then two
            // contexts side by side, each its own space and memory: `gpud: gpu contexts ok (`.
            crate::gpu::render_contexts::rung(dev, &mut display, rungs);
        }
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let clock = {
            let (server_recv, _) = server.slots();
            let gpu_irq = gpu.as_ref().map(|g| g.gpu.irq_slot());
            let waitset = display.bind_vblank(GPU_IRQ_NOTIFY_SLOT).then(|| {
                nexus_abi::waitset_create().ok().and_then(|ws| {
                    nexus_abi::waitset_add(ws, server_recv).ok()?;
                    nexus_abi::waitset_add(ws, GPU_IRQ_NOTIFY_SLOT).ok()?;
                    if let Some(slot) = gpu_irq {
                        nexus_abi::waitset_add(ws, slot).ok()?;
                    }
                    Some(ws)
                })
            });
            crate::frame_clock::FrameClock {
                timer: None,
                waitset: waitset.flatten(),
                due: false,
                last_frame_ns: 0,
            }
        };
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let clock = crate::frame_clock::FrameClock::default();
        nexus_service_entry::ready(GPUD_READY)?;
        nexus_abi::service_verdict_flush("gpud");
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let mut gpu_wake = || {
            if let Some(g) = gpu.as_mut() {
                g.gpu.on_wake();
            }
        };
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        return service_requests(server, &mut display, clock, Some(&mut gpu_wake));
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        return service_requests(server, &mut display, clock, None);
    }
    let mut backend = open_backend_once()?;
    // Branded splash FIRST (task #122): the same glow+wordmark image the GL
    // splash shows later — the scanout switch becomes invisible and the pulse
    // animates from the very first frame. Text, then solid, as fallbacks.
    let (display_w, display_h) = (backend.display_w, backend.display_h);
    if backend.attach_bootstrap_splash_scanout(display_w, display_h).is_ok()
        || backend.attach_bootstrap_text_scanout(display_w, display_h).is_ok()
    {
        let _ = debug_println(GPUD_SCANOUT_OK);
        let _ = debug_println(GPUD_SCANOUT_MODE);
    } else if backend.attach_bootstrap_solid_scanout(display_w, display_h, [0, 0, 0, 255]).is_ok() {
        let _ = debug_println("gpud: bootstrap text unavailable, fallback solid");
        let _ = debug_println(GPUD_SCANOUT_OK);
        let _ = debug_println(GPUD_SCANOUT_MODE);
    } else {
        let _ = debug_println("gpud: bootstrap scanout skipped");
    }

    // Reactive GPU completion: route the device's ring-buffer IRQ to our idle
    // control-reply endpoint so command waits BLOCK on the interrupt instead of
    // busy-polling the used-ring (an interrupt-driven driver port). Bound after
    // the bootstrap scanout so early init keeps the simple spin path; best-effort —
    // a denied bind leaves the queues on spin+yield (never a hang). The shared
    // virtio-gpu IRQ covers both the control and cursor queues.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    let gpu_irq_reactive = {
        // The line the granted capability carries (RFC-0098 C3); 0 = none.
        let irq = nexus_abi::device_irq(GPU_MMIO_CAP_SLOT);
        let bound = irq != 0 && backend.bind_gpu_irq(irq, GPU_IRQ_NOTIFY_SLOT);
        if bound {
            let _ = debug_println("gpud: gpu irq bound");
        } else {
            let _ = debug_println("gpud: gpu irq bind skipped (spin fallback)");
        }
        bound
    };
    let server = bind_server()?;
    // TASK-0324 P7-d: the splash/build-up frame clock is gpud's OWN one-shot timer on a
    // declared notify endpoint (a synthetic vblank — the device has none), a waitset member
    // next to the server endpoint. Never a recv timeout, never a timer on the server endpoint
    // (that intercepted windowd's commands once). Outside those phases: zero idle wakes.
    #[cfg(nexus_env = "os")]
    let clock = {
        use nexus_service_topology::slots::gpud as topo;
        let timer = nexus_ipc::timer::NotifyTimer::bind(topo::TIMER).ok();
        let (server_recv, _) = server.slots();
        let waitset = nexus_abi::waitset_create().ok().and_then(|ws| {
            nexus_abi::waitset_add(ws, server_recv).ok()?;
            nexus_abi::waitset_add(ws, topo::TIMER_RECV).ok()?;
            Some(ws)
        });
        if timer.is_none() || waitset.is_none() {
            let _ =
                debug_println("gpud: FAIL waitset/timer (blocking on the server endpoint alone)");
        }
        crate::frame_clock::FrameClock { timer, waitset, due: false, last_frame_ns: 0 }
    };
    #[cfg(not(nexus_env = "os"))]
    let clock = crate::frame_clock::FrameClock::default();
    nexus_service_entry::ready(GPUD_READY)?;
    // Bring-up done — flush gpud's folded markers as one `gpud N/N OK <ms>` grid line, then stop
    // folding (later per-frame present markers print raw).
    nexus_abi::service_verdict_flush("gpud");
    // Raw (post-fold) so every boot log shows which completion-wait mode is live —
    // the folded bind marker above is invisible in a quiet boot, and a silently
    // unbound IRQ means every deferred completion costs the full 500ms net.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    let _ = debug_println(if gpu_irq_reactive {
        "gpud: completion wait reactive (irq)"
    } else {
        "gpud: completion wait spin fallback (irq unbound)"
    });
    service_requests(server, &mut backend, clock, None)
}

fn open_backend_once() -> Result<VirtioGpuBackend, nexus_abi::AbiError> {
    // RFC-0085: the kernel picks the MMIO window va (fixed 0x2020_0000 gone).
    let mmio_va = match mmio_map_auto(GPU_MMIO_CAP_SLOT, 0, GPU_MMIO_LEN) {
        Ok(va) => va,
        Err(AbiError::InvalidArgument) => return Err(AbiError::InvalidArgument),
        Err(_) => return Err(nexus_abi::AbiError::InvalidArgument),
    };
    let mut backend = VirtioGpuBackend::new(GPU_MMIO_CAP_SLOT, mmio_va, GPU_MMIO_LEN);
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

pub(crate) fn bind_server() -> Result<KernelServer, nexus_abi::AbiError> {
    // The declared server pair (TASK-0324 P4), pinned before this task runs. No route ask at
    // start-up (P7-b): an ask has no clock and init may be blocked in a synchronous exchange
    // with a service that, in turn, waits for THIS server — the ask made that a deadlock.
    KernelServer::new_with_slots(GPUD_RECV_SLOT, GPUD_SEND_SLOT)
        .map_err(|_| nexus_abi::AbiError::InvalidArgument)
}

fn service_requests(
    server: KernelServer,
    display: &mut dyn Display,
    mut clock: crate::frame_clock::FrameClock,
    // TASK-0329 G1: the board GPU's wake (its interrupt endpoint is a waitset member).
    mut gpu_wake: Option<&mut dyn FnMut()>,
) -> Result<(), nexus_abi::AbiError> {
    // 8192 bytes: large enough for full cursor upload (32×32×4 = 4096B BGRA + 9B header).
    let mut recv_frame = [0u8; 8192];
    let mut active_handoff_id: u32 = 0;
    // RFC-0093 §5: `STATUS_REVEALED` is acked exactly once per boot (see `reply::present_status`).
    let mut reveal_acked = false;
    // Persistent present buffer: reused (reload_from) for every frame so gpud
    // does NOT allocate a fresh Vec<Command> per present. gpud runs on a
    // non-freeing bump allocator; a per-frame deserialize Vec would leak and
    // exhaust the 384KB heap after a few hundred animation frames (`alloc-fail
    // svc=gpud`), which is exactly what crashed the GPU pipeline mid-animation.
    let mut scene_cb = CommittedBuffer::with_capacity(32);
    // Present-time telemetry (frame budget for 120Hz = 8333us), alloc-free, one line per window:
    // where the glass/compositor frame cost goes and the present RATE.
    let mut stats = PresentStats::default();
    // Present-chain hop trace (graphical-output bisection): emit the per-frame
    // hops once a frame gets all the way through, but keep re-tracing every frame
    // while the chain is broken so a headless run shows exactly HOW FAR we get.
    let mut chain_trace_done = false;
    // Scroll coalescing: `OP_SET_LAYER_SCROLL` requests only RECORD their row;
    // this flag makes the next recv NonBlocking so the whole queued burst drains
    // (latest row wins), and the single re-composite happens in the WouldBlock
    // arm below. A full present (`OP_PRESENT_DAMAGE`) clears it — that present
    // already composites the recorded rows.
    let mut scroll_flush_pending = false;
    loop {
        // Reactive by default: BLOCK until windowd sends a command (framebuffer VMO,
        // present damage, or animation submit) — no polling, no busy-wait; the kernel
        // wakes us on message arrival. Exception: while the display self-presents (the
        // virtio GPU's boot-splash hold, its spin demo), the frame clock paces a frame so
        // gpud re-evaluates the reveal gate itself — windowd stalls its present loop after
        // its first frame. Once revealed this reverts to Blocking (fully reactive).
        let pacing = display.pacing();
        // A recorded scroll row awaits its composite, or a frame is due: drain any further
        // queued requests first (latest wins), then act in the WouldBlock arm. Otherwise WAIT
        // on the waitset (server + frame clock) — no recv timeout.
        #[cfg(nexus_env = "os")]
        let wait =
            if scroll_flush_pending || clock.due { Wait::NonBlocking } else { clock.wait(pacing) };
        #[cfg(not(nexus_env = "os"))]
        let wait = {
            let _ = (pacing, &clock);
            if scroll_flush_pending {
                Wait::NonBlocking
            } else {
                Wait::Blocking
            }
        };
        // The display's vertical blank (RFC-0101 §8): a flip that landed acks its present now —
        // the compositor's frame clock is the display's. Drained before the requests so an ack
        // never waits behind the next present.
        while let Some((seq, status)) = display.vblank_wake() {
            crate::reply::send(
                &server,
                display,
                OP_PRESENT_DAMAGE,
                status,
                Some(seq),
                active_handoff_id,
            );
        }
        if let Some(wake) = gpu_wake.as_mut() {
            wake();
        }
        match server.recv_request_with_meta_into(wait, &mut recv_frame) {
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
                        let handoff_t0 = nsec().unwrap_or(0);
                        let handoff_id =
                            decode_handoff_id_attach(frame).unwrap_or(active_handoff_id);
                        // RFC-0098 C7: the framebuffer is gpud's (granted before); no cap moves.
                        match crate::framebuffer_grant::attach(display, moved_cap.take()) {
                            Ok(()) => {
                                active_handoff_id = handoff_id;
                                display.attached();
                                let _ = debug_println("gpud: handoff attach ack");
                                let _ = debug_println(GPUD_CURSOR_ON);
                                let _ = debug_println(GPUD_DISPLAY_READY);
                                emit_handoff_timing(
                                    (nsec().unwrap_or(handoff_t0).saturating_sub(handoff_t0)
                                        / 1_000_000) as u32,
                                );
                                (STATUS_OK, Some(active_handoff_id))
                            }
                            Err(status) => (status, Some(handoff_id)),
                        }
                    }
                    nexus_display_proto::OP_FRAMEBUFFER_REQUEST => {
                        (crate::framebuffer_grant::grant(display), None)
                    }
                    OP_PRESENT_DAMAGE => {
                        // This present composites the recorded scroll rows — the
                        // deferred flush would be a redundant second re-composite.
                        scroll_flush_pending = false;
                        // Phase 6c: carries a serialized CommittedBuffer with batched
                        // BlitSurface commands describing all damage regions.
                        let seq = nexus_display_proto::decode_present_seq(frame).unwrap_or(0);
                        display.begin_present();
                        let trace = !chain_trace_done;
                        if trace {
                            let _ = debug_println(crate::markers::GPUD_CHAIN_RECV);
                        }
                        let status = if frame.len() > PRESENT_HEADER_LEN {
                            // Reuse scene_cb (reload_from) — no per-frame heap alloc.
                            match scene_cb.reload_from(&frame[PRESENT_HEADER_LEN..]) {
                                Ok(_) => {
                                    if trace {
                                        let _ = debug_println(crate::markers::GPUD_CHAIN_PARSE_OK);
                                    }
                                    let t0 = nsec().unwrap_or(0);
                                    let st =
                                        crate::present_chain::present(display, &scene_cb, trace);
                                    if trace && st == STATUS_OK {
                                        // Whole chain reached the end: stop tracing.
                                        chain_trace_done = true;
                                        display.first_frame_shown();
                                    }
                                    stats.record(t0, nsec().unwrap_or(t0));
                                    st
                                }
                                Err(_) => {
                                    if trace {
                                        let _ =
                                            debug_println(crate::markers::GPUD_CHAIN_PARSE_FAIL);
                                    }
                                    // Fixed-rect fallback: exactly header + 16-byte rect.
                                    if frame.len() == nexus_display_proto::DAMAGE_FRAME_LEN {
                                        crate::present_chain::handle_present_damage(display, frame)
                                    } else {
                                        STATUS_MALFORMED
                                    }
                                }
                            }
                        } else {
                            crate::present_chain::handle_present_damage(display, frame)
                        };
                        // P0.3: honest present outcome — a present whose commands the device
                        // lost (while every call returned success) is NACKed, so windowd
                        // requeues the damage instead of booking a frame nobody saw.
                        let status =
                            if display.present_lost() { STATUS_DEVICE_ERROR } else { status };
                        let status =
                            crate::reply::present_status(display, status, seq, &mut reveal_acked);
                        // RFC-0101 §8: a display with a ring acks the present at the vertical
                        // blank its flip lands on (`vblank_wake` below), never before.
                        if display.present_ack_deferred(seq, status) {
                            continue;
                        }
                        (status, Some(seq))
                    }
                    OP_UPLOAD_CURSOR => {
                        let _ = debug_println("gpud: recv OP_UPLOAD_CURSOR");
                        // Frame: [op, w(4), h(4), hot_x(4), hot_y(4), bgra]. The reply's
                        // u32 payload names the cursor path (overlay, GL or software).
                        if frame.len() < 17 {
                            (STATUS_MALFORMED, None)
                        } else {
                            let w = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                            let h = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
                            let hot_x =
                                u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
                            let hot_y =
                                u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                            display.upload_cursor(&frame[17..], w, h, (hot_x, hot_y))
                        }
                    }
                    OP_UPLOAD_ICON => {
                        let _ = debug_println("gpud: recv OP_UPLOAD_ICON");
                        // Frame: [op, tex_w(4), tex_h(4), dst_x(4), dst_y(4),
                        // dst_w(4), dst_h(4), bgra]. dst_w/h is the on-screen size
                        // (the texture may be 2× → GPU-downscaled when composited).
                        if frame.len() < 25 {
                            (STATUS_MALFORMED, None)
                        } else {
                            let w = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                            let h = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
                            let dst = Rect {
                                x: u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]),
                                y: u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]),
                                width: u32::from_le_bytes([
                                    frame[17], frame[18], frame[19], frame[20],
                                ]),
                                height: u32::from_le_bytes([
                                    frame[21], frame[22], frame[23], frame[24],
                                ]),
                            };
                            let status = match display.upload_icon(&frame[25..], w, h, dst) {
                                Ok(()) => STATUS_OK,
                                Err(_) => STATUS_MALFORMED,
                            };
                            (status, None)
                        }
                    }
                    nexus_display_proto::OP_REVEAL => {
                        // RFC-0093 §5: windowd reports the desktop complete (wallpaper in
                        // Plane 0, cursor uploaded, first frame presented). Latch it — the
                        // next present reveals and is acked STATUS_REVEALED. No probe, no cap.
                        display.request_reveal();
                        (STATUS_OK, None)
                    }
                    nexus_display_proto::OP_WALLPAPER_DIRTY => {
                        display.wallpaper_dirty();
                        (STATUS_OK, None)
                    }
                    nexus_display_proto::OP_READBACK => {
                        (crate::readback::serve(display, frame, moved_cap.take()), None)
                    }
                    _ => (handle_frame(display, frame, &mut scroll_flush_pending), None),
                };
                drop(moved_cap);
                crate::reply::send(
                    &server,
                    display,
                    op,
                    status,
                    response_handoff_id,
                    active_handoff_id,
                );
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                // Deferred scroll composite: the queued burst is drained (every
                // request already recorded its row, latest wins) — re-composite
                // ONCE at the final position, then return to reactive blocking.
                if scroll_flush_pending {
                    scroll_flush_pending = false;
                    display.flush_layer_overrides();
                    continue;
                }
                // Frame due (the frame clock fired, windowd idle): the display self-presents
                // (the reveal gate re-evaluates the instant the desktop is ready; the spin demo).
                // Once nothing paces, the clock stays disarmed (a purely reactive wait).
                if !clock.due {
                    continue;
                }
                display.frame_tick(active_handoff_id != 0, &mut stats);
                clock.frame_presented(nsec().unwrap_or(0));
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
    nexus_display_proto::decode_handoff_id(frame)
}

fn handle_frame(display: &mut dyn Display, frame: &[u8], scroll_flush: &mut bool) -> u8 {
    let Some(op) = frame.first().copied() else {
        return STATUS_MALFORMED;
    };
    match op {
        OP_SUBMIT_ANIMATION_FRAME => {
            // Animation frames carry a serialized CommittedBuffer after the opcode.
            if frame.len() <= 1 {
                return STATUS_MALFORMED;
            }
            match CommittedBuffer::deserialize_from(&frame[1..]) {
                Ok((cmd, _consumed)) => {
                    let _ = display.submit(cmd);
                    STATUS_OK
                }
                Err(_) => STATUS_MALFORMED,
            }
        }
        OP_UPLOAD_CURSOR_SHAPE => {
            // Frame: [op, shape_id, w(4), h(4), hot_x(4), hot_y(4), bgra].
            // Cache fill only — arming stays OP_UPLOAD_CURSOR. 1-byte reply.
            if frame.len() < 18 {
                return STATUS_MALFORMED;
            }
            let shape_id = frame[1];
            let w = u32::from_le_bytes([frame[2], frame[3], frame[4], frame[5]]);
            let h = u32::from_le_bytes([frame[6], frame[7], frame[8], frame[9]]);
            let hot_x = u32::from_le_bytes([frame[10], frame[11], frame[12], frame[13]]);
            let hot_y = u32::from_le_bytes([frame[14], frame[15], frame[16], frame[17]]);
            match display.cache_cursor_shape(shape_id, &frame[18..], w, h, (hot_x, hot_y)) {
                Ok(()) => STATUS_OK,
                Err(_) => STATUS_MALFORMED,
            }
        }
        OP_SELECT_CURSOR_SHAPE => {
            // Frame: [op, shape_id]. Fire-and-forget hot path: swap the active
            // sprite from the cache; the next present draws the new shape.
            if frame.len() < 2 {
                return STATUS_MALFORMED;
            }
            match display.select_cursor_shape(frame[1]) {
                Ok(()) => STATUS_OK,
                Err(_) => STATUS_MALFORMED,
            }
        }
        OP_MOVE_CURSOR => {
            if frame.len() < 9 {
                return STATUS_MALFORMED;
            }
            let x = i32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let y = i32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            match display.move_cursor(x, y) {
                Ok(()) => STATUS_OK,
                Err(_) => STATUS_DEVICE_ERROR,
            }
        }
        OP_SET_LAYER_SCROLL => {
            if frame.len() < 9 {
                return STATUS_MALFORMED;
            }
            let scroll_id = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let src_row = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            // RECORD the override only — the loop drains the whole queued burst (latest row
            // wins) and re-composites ONCE when the queue is empty.
            match display.set_layer_scroll(scroll_id, src_row) {
                Ok(retained) => {
                    *scroll_flush |= retained;
                    STATUS_OK
                }
                Err(_) => STATUS_DEVICE_ERROR,
            }
        }
        OP_SET_LAYER_TRANSFORM => {
            // Track C2 (the scroll generalization): RECORD-only + the same
            // coalesced flush — presenting per request would backlog stale
            // transforms exactly like the scroll fling did.
            let Some((layer_id, dx, dy, opacity, scale_pct)) =
                nexus_display_proto::decode_set_layer_transform(frame)
            else {
                return STATUS_MALFORMED;
            };
            match display.set_layer_transform(layer_id, (dx, dy), opacity, scale_pct) {
                Ok(retained) => {
                    *scroll_flush |= retained;
                    STATUS_OK
                }
                Err(_) => STATUS_DEVICE_ERROR,
            }
        }
        OP_PRESENT_DAMAGE => {
            // A full present composites the recorded scroll/transform overrides
            // anyway — the deferred flush would be a redundant re-composite.
            *scroll_flush = false;
            crate::present_chain::handle_present_damage(display, frame)
        }
        _ => STATUS_MALFORMED,
    }
}
