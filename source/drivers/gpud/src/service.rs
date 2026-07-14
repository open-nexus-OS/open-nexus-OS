// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite gpud service entry for the QEMU virtio-gpu proof path.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

#[cfg(all(nexus_env = "os", feature = "virgl"))]
use core::time::Duration;
use nexus_abi::{debug_println, debug_write, mmio_map, nsec, yield_, AbiError};
use nexus_ipc::{KernelServer, Server as _, Wait};

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use crate::backend::VirtioGpuBackend;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::backend::IRQ_DEADLINE_EXPIRED_COUNT;
use crate::markers::{
    GPUD_CURSOR_ON, GPUD_DISPLAY_READY, GPUD_MMIO_FAULT, GPUD_NO_DEVICE, GPUD_READY,
    GPUD_SCANOUT_MODE, GPUD_SCANOUT_OK, GPUD_VIRTIO_GPU_PROBED,
};

pub const ROUTE_NAME: &str = "gpud";
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
/// Self-paced re-present interval for the build-up spin-blur demo (~120 Hz). Used
/// as the gpud server-recv timeout: an idle recv wakes here to re-present.
#[cfg(all(nexus_env = "os", feature = "virgl"))]
const SPIN_DEMO_PERIOD_NS: u64 = 8_333_333;
/// Reply payloads for OP_UPLOAD_CURSOR (magic-tagged — distinguishable from
/// present acks, whose u32 slot carries a small handoff id).
pub const CURSOR_REPLY_HW: u32 = nexus_display_proto::CURSOR_REPLY_HW;
pub const CURSOR_REPLY_SW: u32 = nexus_display_proto::CURSOR_REPLY_SW;
/// virgl GL scanout: the build-up present draws a *procedural* cursor at
/// `cursor_ox/oy` each frame (no resource transfer — safe on the GL scanout,
/// unlike the HW overlay). windowd must ship `OP_MOVE_CURSOR` on every move
/// AND a present so the procedural arrow re-renders at the new position.
pub const CURSOR_REPLY_GL: u32 = nexus_display_proto::CURSOR_REPLY_GL;
pub const STATUS_OK: u8 = nexus_display_proto::STATUS_OK;
pub const STATUS_MALFORMED: u8 = nexus_display_proto::STATUS_MALFORMED;
pub const STATUS_DEVICE_ERROR: u8 = nexus_display_proto::STATUS_DEVICE_ERROR;

const GPU_MMIO_CAP_SLOT: u32 = 48;
const GPU_MMIO_VA: usize = 0x2020_0000;
const GPU_MMIO_LEN: usize = 0x1000;
const GPUD_RECV_SLOT: u32 = 3;
const GPUD_SEND_SLOT: u32 = 4;
/// virtio-mmio GPU PLIC interrupt source. The GPU sits at MMIO 0x1000_8000 on the
/// QEMU virt machine = virtio-mmio slot 7 (0x1000_1000 + 7·0x1000), and QEMU wires
/// slot N to PLIC source N+1 → source 8. Same convention as virtio-input (slots
/// 2/3 → IRQ 3/4 in hidrawd). Drives the reactive GPU ring-buffer completion wait.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_IRQ_SOURCE: u32 = 8;
/// Endpoint cap slot the GPU IRQ is routed to: gpud's idle control-reply endpoint
/// (slot 2 — the same idle endpoint hidrawd reuses for input IRQs). Deliberately
/// NOT the windowd↔gpud server endpoint (slot 3): binding a notification source
/// there would intercept windowd's present commands and break the channel.
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_IRQ_NOTIFY_SLOT: u32 = 2;
/// Display framebuffer dimensions matching windowd's VISIBLE_BOOTSTRAP_WIDTH/HEIGHT.
/// On QEMU virtio-gpu with `-display gtk`, the GTK window resizes to match this scanout.
const DISPLAY_WIDTH: u32 = 1280;
const DISPLAY_HEIGHT: u32 = 800;
// 9600 rows: 4 display planes (wallpaper/retained/slot-A/slot-B, 3200) + surface
// atlas (4000) for the retained-surface compositor's cached layers — grown by a
// full display frame so full-screen system overlays (login greeter, later lock
// screen) fit as ONE atlas-band layer. MUST match windowd
// `crate::atlas::RESOURCE_HEIGHT` (separate crate, no shared dep).
const RESOURCE_HEIGHT: u32 = 9600;
/// Display plane row within the resource (fixed 4-plane layout). Matches
/// `backend::DISPLAY_PLANE_ROW` and windowd's `DISPLAY_ROW_OFFSET`.
const DISPLAY_PLANE_ROW: u32 = 1600;
pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    // Verdict folding: fold gpud's scattered `debug_println` bring-up markers (virgl ready/shader/
    // draw/gradient/scanout/…) into one `gpud N/N` grid line in interactive boots. Flushed at
    // GPUD_READY below; FAIL lines still print live; proof boots emit everything raw.
    nexus_abi::service_verdict_arm();
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
    } else if backend
        .attach_bootstrap_solid_scanout(display_w, display_h, [0, 0, 0, 255])
        .is_ok()
    {
        let _ = debug_println("gpud: bootstrap text unavailable, fallback solid");
        let _ = debug_println(GPUD_SCANOUT_OK);
        let _ = debug_println(GPUD_SCANOUT_MODE);
    } else {
        let _ = debug_println("gpud: bootstrap scanout skipped");
    }

    // GPU-only architecture: gpud is a pure driver, not a display owner.
    // It probes the device and becomes IPC-ready. The scanout is set only
    // when windowd (the sole display owner) sends a framebuffer VMO via
    // OP_SET_FRAMEBUFFER_VMO. No boot splash, no startup create_resource.
    // Register in the global IPC registry BEFORE emitting the ready marker.
    // Windowd's KernelClient::new_for("gpud") depends on this registration.
    //
    // Reactive GPU completion: route the device's ring-buffer IRQ to our idle
    // control-reply endpoint so command waits BLOCK on the interrupt instead of
    // busy-polling the used-ring (an interrupt-driven driver port). Bound after
    // the bootstrap scanout so early init keeps the simple spin path; best-effort —
    // a denied bind leaves the queues on spin+yield (never a hang). The shared
    // virtio-gpu IRQ covers both the control and cursor queues.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    let gpu_irq_reactive = {
        let bound = backend.bind_gpu_irq(GPU_IRQ_SOURCE, GPU_IRQ_NOTIFY_SLOT);
        if bound {
            let _ = debug_println("gpud: gpu irq bound");
        } else {
            let _ = debug_println("gpud: gpu irq bind skipped (spin fallback)");
        }
        bound
    };
    let server = bind_server()?;
    debug_println(GPUD_READY)?;
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
    service_requests(server, backend)
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
    if let Ok(server) = KernelServer::new_for(ROUTE_NAME) {
        let _ = debug_println("gpud: route connected");
        return Ok(server);
    }
    let _ = debug_println("gpud: route fallback slots");
    KernelServer::new_with_slots(GPUD_RECV_SLOT, GPUD_SEND_SLOT)
        .map_err(|_| nexus_abi::AbiError::InvalidArgument)
}

fn service_requests(
    server: KernelServer,
    mut backend: VirtioGpuBackend,
) -> Result<(), nexus_abi::AbiError> {
    // 8192 bytes: large enough for full cursor upload (32×32×4 = 4096B BGRA + 9B header).
    let mut recv_frame = [0u8; 8192];
    let mut active_handoff_id: u32 = 0;
    // One-shot flag for the hold-tick liveness marker below (diagnosis: do the
    // recv-timeout self-ticks actually fire while the boot splash is held?).
    #[cfg(all(nexus_env = "os", feature = "virgl"))]
    let mut hold_tick_logged = false;
    // Rate limiter for the 2D bootstrap-splash pulse (~30Hz redraw of the title
    // band; the recv timeout ticks faster than the curve needs).
    #[cfg(all(nexus_env = "os", feature = "virgl"))]
    let mut last_splash_pulse_ns: u64 = 0;
    // Persistent present buffer: reused (reload_from) for every frame so gpud
    // does NOT allocate a fresh Vec<Command> per present. gpud runs on a
    // non-freeing bump allocator; a per-frame deserialize Vec would leak and
    // exhaust the 384KB heap after a few hundred animation frames (`alloc-fail
    // svc=gpud`), which is exactly what crashed the GPU pipeline mid-animation.
    let mut scene_cb = CommittedBuffer::with_capacity(32);
    // Present-time telemetry (frame budget for 120Hz = 8333us). Accumulated over
    // a window and emitted as a no-alloc marker every PRESENT_STATS_WINDOW
    // presents — gpud runs on a non-freeing bump allocator, so no per-frame
    // format!/heap. Lets us measure where the glass/compositor frame cost goes.
    const PRESENT_STATS_WINDOW: u32 = 120;
    let mut present_count: u32 = 0;
    let mut present_ns_sum: u64 = 0;
    let mut present_ns_max: u64 = 0;
    // Present-chain hop trace (graphical-output bisection): emit the per-frame
    // hops once a frame gets all the way through, but keep re-tracing every frame
    // while the chain is broken so a headless run shows exactly HOW FAR we get.
    let mut chain_trace_done = false;
    // Build-up spin-blur demo: when active, the main recv below uses a frame-paced
    // timeout (SPIN_DEMO_PERIOD_NS) so an idle gpud re-presents the orbiting build-up
    // every ~8.33ms (120Hz), recomputing the GPU blur/shadow and driving the reactive
    // ring-buffer IRQ. It is a *recv deadline* — woken by the kernel idle-loop's
    // IpcRecv-deadline scan — NOT a timer cap on our server endpoint (an earlier
    // timer-cap attempt intercepted windowd's commands and OOM'd the present channel).
    #[cfg(all(nexus_env = "os", feature = "virgl"))]
    let spin_demo_active =
        crate::gl_scanout::COMPOSITOR_BUILDUP && crate::gl_scanout::BUILDUP_SPIN_DEMO;
    // Scroll coalescing: `OP_SET_LAYER_SCROLL` requests only RECORD their row;
    // this flag makes the next recv NonBlocking so the whole queued burst drains
    // (latest row wins), and the single re-composite happens in the WouldBlock
    // arm below. A full present (`OP_PRESENT_DAMAGE`) clears it — that present
    // already composites the recorded rows.
    let mut scroll_flush_pending = false;
    loop {
        // Reactive by default: BLOCK until windowd sends a command (framebuffer VMO,
        // present damage, or animation submit) — no polling, no busy-wait; the kernel
        // wakes us on message arrival. Exception: while the boot splash is still held (or
        // the spin demo runs), wake on a frame-paced timeout so gpud self-re-presents and
        // re-evaluates the reveal gate. windowd stalls its present loop after its first
        // frame, so gpud must drive the reveal itself rather than block until windowd
        // recovers (seconds later). Once revealed, this reverts to Blocking (fully reactive).
        #[cfg(all(nexus_env = "os", feature = "virgl"))]
        let wait = if scroll_flush_pending {
            // A recorded scroll row awaits its composite: drain any further queued
            // requests first (latest wins), then flush in the WouldBlock arm.
            Wait::NonBlocking
        } else if spin_demo_active
            || backend.is_holding_boot_splash()
            || backend.bootstrap_splash_active()
        {
            Wait::Timeout(Duration::from_nanos(SPIN_DEMO_PERIOD_NS))
        } else {
            Wait::Blocking
        };
        #[cfg(not(all(nexus_env = "os", feature = "virgl")))]
        let wait = if scroll_flush_pending { Wait::NonBlocking } else { Wait::Blocking };
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
                        match moved_cap.take() {
                            Some(cap) => match backend.attach_external_framebuffer(
                                cap.slot(),
                                DISPLAY_WIDTH,
                                RESOURCE_HEIGHT,
                            ) {
                                Ok(()) => {
                                    active_handoff_id = handoff_id;
                                    let _ = backend.move_cursor(0, 0);
                                    let _ = debug_println("gpud: handoff attach ack");
                                    let _ = debug_println(GPUD_CURSOR_ON);
                                    let _ = debug_println(GPUD_DISPLAY_READY);
                                    emit_handoff_timing(
                                        (nsec()
                                            .unwrap_or(handoff_t0)
                                            .saturating_sub(handoff_t0)
                                            / 1_000_000) as u32,
                                    );
                                    // The GL scanout now exists, so the recv-timeout
                                    // path may re-present the build-up (spin-blur demo).
                                    #[cfg(all(nexus_env = "os", feature = "virgl"))]
                                    if spin_demo_active {
                                        let _ = debug_println("gpud: spin-blur demo armed (120Hz)");
                                    }
                                    (STATUS_OK, Some(active_handoff_id))
                                }
                                Err(e) => {
                                    let _ = debug_println("gpud: ERROR attach framebuffer failed");
                                    let _ = debug_println(
                                        "gpud: ERROR attach framebuffer resource create failed",
                                    );
                                    let _ = e;
                                    (STATUS_DEVICE_ERROR, Some(handoff_id))
                                }
                            },
                            None => {
                                let _ = debug_println("gpud: ERROR no cap in VMO message");
                                (STATUS_MALFORMED, Some(handoff_id))
                            }
                        }
                    }
                    OP_PRESENT_DAMAGE => {
                        // This present composites the recorded scroll rows — the
                        // deferred flush would be a redundant second re-composite.
                        scroll_flush_pending = false;
                        // Phase 6c: carries a serialized CommittedBuffer with batched
                        // BlitSurface commands describing all damage regions.
                        let handoff_id =
                            decode_handoff_id_present(frame).unwrap_or(active_handoff_id);
                        // P0.3 present truth: snapshot the ring's deadline-expiry
                        // counter around the whole present. The ring's degraded
                        // recovery (reset/abandon after GPU_WAIT_DEADLINE_NS)
                        // deliberately returns success so the loop never wedges —
                        // but a present that lost commands that way must NOT be
                        // acked as shown. The counter delta catches every such
                        // case, including error paths swallowed inside optional
                        // draws (`let _ =`), at the one seam they all share.
                        #[cfg(all(feature = "os-lite", target_os = "none"))]
                        let deadline_expiries_before =
                            IRQ_DEADLINE_EXPIRED_COUNT.load(core::sync::atomic::Ordering::Relaxed);
                        let trace = !chain_trace_done;
                        if trace {
                            let _ = debug_println(crate::markers::GPUD_CHAIN_RECV);
                        }
                        let status = if frame.len() > 1 {
                            // Reuse scene_cb (reload_from) — no per-frame heap alloc.
                            match scene_cb.reload_from(&frame[1..]) {
                                Ok(_) => {
                                    if trace {
                                        let _ = debug_println(crate::markers::GPUD_CHAIN_PARSE_OK);
                                    }
                                    let damage_rect = damage_rect_from_cb(&scene_cb, backend.display_w, backend.display_h);
                                    // Lift the save-under cursor so scene blits land on
                                    // a cursor-free plane, present, then re-apply it on
                                    // top so the pointer always stays visible.
                                    let t0 = nsec().unwrap_or(0);
                                    backend.cursor_before_present();
                                    // present_committed's result was previously discarded;
                                    // capture it so a failed composite is no longer silent.
                                    match backend.present_committed(&scene_cb) {
                                        Ok(_) => {
                                            if trace {
                                                let _ = debug_println(
                                                    crate::markers::GPUD_CHAIN_EXEC_OK,
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            let _ =
                                                debug_println(crate::markers::GPUD_CHAIN_EXEC_FAIL);
                                            let _ = debug_println(gfx_error_label(e));
                                        }
                                    }
                                    let st = present_scanout_damage(&mut backend, damage_rect);
                                    backend.cursor_after_present();
                                    if trace {
                                        if st == STATUS_OK {
                                            let _ = debug_println(
                                                crate::markers::GPUD_CHAIN_SCANOUT_OK,
                                            );
                                            // Whole chain reached the end: stop tracing.
                                            chain_trace_done = true;
                                            // P0.3 display truth (one-shot): read the LIVE
                                            // scanout RT back from the host GPU. A black
                                            // sample with a green marker chain = the silent
                                            // scanout class — now loud, from inside.
                                            #[cfg(feature = "virgl")]
                                            {
                                                match backend.scanout_sample() {
                                                    Some(px)
                                                        if (px[0] as u32
                                                            + px[1] as u32
                                                            + px[2] as u32)
                                                            > 24 =>
                                                    {
                                                        let _ = debug_println(
                                                            "gpud: scanout sample ok",
                                                        );
                                                        // P0.3c: MEASURED display
                                                        // truth (host-GPU readback
                                                        // of the live scanout RT),
                                                        // not a compositor claim —
                                                        // #98 discipline.
                                                        let _ = debug_println(
                                                            "SELFTEST: display nonblack ok",
                                                        );
                                                    }
                                                    Some(_) => {
                                                        let _ = debug_println(
                                                            "gpud: FAIL scanout black",
                                                        );
                                                    }
                                                    None => {
                                                        let _ = debug_println(
                                                            "gpud: scanout sample unavailable",
                                                        );
                                                    }
                                                }
                                            }
                                        } else {
                                            let _ = debug_println(
                                                crate::markers::GPUD_CHAIN_SCANOUT_FAIL,
                                            );
                                        }
                                    }
                                    let dt = nsec().unwrap_or(t0).saturating_sub(t0);
                                    present_ns_sum += dt;
                                    present_ns_max = present_ns_max.max(dt);
                                    present_count += 1;
                                    if present_count >= PRESENT_STATS_WINDOW {
                                        emit_present_stats(
                                            (present_ns_sum / present_count as u64 / 1000) as u32,
                                            (present_ns_max / 1000) as u32,
                                            present_count,
                                        );
                                        present_count = 0;
                                        present_ns_sum = 0;
                                        present_ns_max = 0;
                                    }
                                    st
                                }
                                Err(_) => {
                                    if trace {
                                        let _ =
                                            debug_println(crate::markers::GPUD_CHAIN_PARSE_FAIL);
                                    }
                                    // Only fall back to legacy damage-rect format when the
                                    // frame is exactly 17 bytes (opcode + 16-byte rect).
                                    if frame.len() == 17 {
                                        handle_present_damage(&mut backend, frame)
                                    } else {
                                        STATUS_MALFORMED
                                    }
                                }
                            }
                        } else {
                            handle_present_damage(&mut backend, frame)
                        };
                        // P0.3: honest present outcome — commands that ran into the
                        // 500ms deadline net were abandoned by the ring's degraded
                        // recovery; the frame is (partially) lost even though every
                        // call above returned "success". NACK it so windowd requeues
                        // the damage instead of booking a black frame as presented.
                        #[cfg(all(feature = "os-lite", target_os = "none"))]
                        let status = {
                            let expired = IRQ_DEADLINE_EXPIRED_COUNT
                                .load(core::sync::atomic::Ordering::Relaxed)
                                .wrapping_sub(deadline_expiries_before);
                            if expired > 0 {
                                emit_present_deadline_fail(expired);
                                STATUS_DEVICE_ERROR
                            } else {
                                status
                            }
                        };
                        if status == STATUS_OK {
                            active_handoff_id = handoff_id;
                        }
                        (status, Some(handoff_id))
                    }
                    OP_UPLOAD_CURSOR => {
                        let _ = debug_println("gpud: recv OP_UPLOAD_CURSOR");
                        // Frame: [op, w(4), h(4), hot_x(4), hot_y(4), bgra]. The reply's
                        // u32 payload reports the active cursor path: 1 = hardware
                        // overlay (cursor queue), 0 = software BlendCursor fallback.
                        if frame.len() < 17 {
                            (STATUS_MALFORMED, None)
                        } else {
                            let w = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                            let h = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
                            let hot_x =
                                u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
                            let hot_y =
                                u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                            let bgra = &frame[17..];
                            arm_cursor(&mut backend, bgra, w, h, hot_x, hot_y)
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
                            let dx = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
                            let dy = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                            let dw = u32::from_le_bytes([frame[17], frame[18], frame[19], frame[20]]);
                            let dh = u32::from_le_bytes([frame[21], frame[22], frame[23], frame[24]]);
                            let bgra = &frame[25..];
                            let status = match backend.store_icon_sprite(bgra, w, h, dx, dy, dw, dh) {
                                Ok(()) => STATUS_OK,
                                Err(_) => STATUS_MALFORMED,
                            };
                            (status, None)
                        }
                    }
                    nexus_display_proto::OP_WALLPAPER_DIRTY => {
                        // windowd rewrote the wallpaper SOURCE plane (theme
                        // swap): re-upload the wallpaper texture on the next
                        // buildup present (self-tick picks it up).
                        #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
                        {
                            backend.wallpaper_reupload_pending = true;
                        }
                        (STATUS_OK, None)
                    }
                    nexus_display_proto::OP_GET_DISPLAY_MODE => {
                        // The VISIBLE mode resolved at probe (GET_DISPLAY_INFO,
                        // clamped to the fixed resource budget). Reply payload
                        // rides the 5-byte status+u32 frame: LE u32 = w | h<<16
                        // — byte-identical to `encode_display_mode_reply`.
                        (
                            STATUS_OK,
                            Some(backend.display_w | (backend.display_h << 16)),
                        )
                    }
                    _ => (handle_frame(&mut backend, frame, &mut scroll_flush_pending), None),
                };
                drop(moved_cap);
                if let Some(handoff_id) = response_handoff_id {
                    let mut response = [0u8; 5];
                    response[0] = status;
                    response[1..5].copy_from_slice(&handoff_id.to_le_bytes());
                    let _ = server.send(&response, Wait::Blocking);
                } else {
                    let response = [status];
                    let _ = server.send(&response, Wait::Blocking);
                }
                // Reveal kick: a cursor upload while the boot splash is held is
                // exactly the signal the reveal gate waits for — re-present now
                // instead of waiting for the next self-tick (~250 ms observed),
                // so the desktop appears the moment it is ready. Reply was sent
                // first, so windowd is never blocked behind this present.
                // One line per boot: names WHICH branch ran, so a boot log pins
                // why a late reveal happened without another instrumentation loop.
                // Gate on `is_holding_boot_splash()` (GL scanout up + splash still
                // held) — NOT on `active_handoff_id`: the running handoff flow's
                // attach frame is the 1-byte id-less form, so the id stays 0 and
                // had silently disabled this kick (and the hold tick) in every boot.
                #[cfg(all(nexus_env = "os", feature = "virgl"))]
                if op == OP_UPLOAD_CURSOR {
                    let armed = status == STATUS_OK && backend.is_holding_boot_splash();
                    let _ = debug_println(match (armed, status == STATUS_OK) {
                        (true, _) => "gpud: reveal kick",
                        (false, false) => "gpud: reveal kick skipped (cursor status)",
                        (false, _) => "gpud: reveal kick skipped (not holding)",
                    });
                    if armed {
                        let _ = backend.present_scanout_damage(Rect {
                            x: 0,
                            y: 0,
                            width: backend.display_w,
                            height: backend.display_h,
                        });
                        if backend.is_holding_boot_splash() {
                            let _ = debug_println(if backend.cursor_tex_ready() {
                                "gpud: reveal kick held (plane0 empty)"
                            } else {
                                "gpud: reveal kick held (cursor tex not ready)"
                            });
                        }
                    }
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                // Deferred scroll composite: the queued burst is drained (every
                // request already recorded its row, latest wins) — re-composite
                // ONCE at the final position, then return to reactive blocking.
                if scroll_flush_pending {
                    scroll_flush_pending = false;
                    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
                    {
                        let _ = backend.flush_layer_scroll();
                    }
                    continue;
                }
                // Frame-paced tick (recv timed out, windowd idle): re-present so the reveal
                // gate re-evaluates and the desktop appears the instant the wallpaper +
                // cursor are ready — gpud drives this itself because windowd stalls its
                // present loop after the first frame. Also serves the spin-blur demo. Once
                // the desktop is revealed `is_holding_boot_splash()` goes false and gpud
                // stops self-ticking (back to a blocking, reactive recv).
                // One-shot liveness proof: boots showed reveals riding ONLY on windowd
                // presents, so pin whether this timeout path fires at all while holding.
                #[cfg(all(nexus_env = "os", feature = "virgl"))]
                if !hold_tick_logged && backend.is_holding_boot_splash() {
                    hold_tick_logged = true;
                    let _ = debug_println("gpud: hold tick alive");
                }
                // The hold-phase tick gates on `is_holding_boot_splash()` alone —
                // holding implies the GL scanout is attached (gl_scanout_active),
                // which is what the old `active_handoff_id != 0` guard was meant to
                // prove. The id stays 0 in the running id-less handoff flow, so
                // that guard had silently disabled every hold tick (reveals only
                // ever rode on windowd presents). The spin demo keeps the id gate.
                #[cfg(all(nexus_env = "os", feature = "virgl"))]
                let presented = if backend.bootstrap_splash_active() {
                    // 2D text phase (before windowd's handoff): breathe the title
                    // line so the very first thing on screen already lives. ~30Hz
                    // redraw is plenty for the slow curve; the wall-clock pulse
                    // stays continuous into the GL splash after the switch.
                    let now = nsec().unwrap_or(0);
                    if now.saturating_sub(last_splash_pulse_ns) >= 33_000_000 {
                        last_splash_pulse_ns = now;
                        let _ =
                            backend.pulse_bootstrap_splash(crate::backend::splash_pulse_q8(now));
                    }
                    true
                } else {
                    ((spin_demo_active && active_handoff_id != 0)
                        || backend.is_holding_boot_splash())
                        && {
                            present_buildup_tick(
                                &mut backend,
                                &mut present_count,
                                &mut present_ns_sum,
                                &mut present_ns_max,
                                PRESENT_STATS_WINDOW,
                            );
                            true
                        }
                };
                #[cfg(not(all(nexus_env = "os", feature = "virgl")))]
                let presented = false;
                if !presented {
                    let _ = yield_();
                }
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

/// Emit `gpud: present us avg=A max=M n=N` without heap allocation (gpud's
/// bump allocator never frees). 120Hz budget = 8333us; this surfaces the
/// per-present compositor cost so glass/layer optimisations can be measured.
/// Human-readable reason for a present-chain hop failure (G3 exec). Static
/// strings only — no alloc on gpud's bump heap.
fn gfx_error_label(e: GfxError) -> &'static str {
    match e {
        GfxError::DeviceNotFound => "gpud: chain reason: device not found",
        GfxError::MmioFault => "gpud: chain reason: mmio fault",
        GfxError::CommandRejected => "gpud: chain reason: command rejected",
        GfxError::ResourceExhausted => "gpud: chain reason: resource exhausted (bump heap?)",
        GfxError::Unsupported => "gpud: chain reason: unsupported command",
        GfxError::InvalidArgument => "gpud: chain reason: invalid argument",
    }
}

/// Re-present the orbiting build-up panel once (spin-blur demo tick) and fold the
/// GPU/blur cost into the present-stats window. Driven by the recv-timeout path so
/// an idle gpud keeps the GPU pipeline + reactive ring-buffer IRQ exercised.
#[cfg(all(nexus_env = "os", feature = "virgl"))]
fn present_buildup_tick(
    backend: &mut VirtioGpuBackend,
    present_count: &mut u32,
    present_ns_sum: &mut u64,
    present_ns_max: &mut u64,
    window: u32,
) {
    let t0 = nsec().unwrap_or(0);
    let _ = backend.present_scanout_damage(Rect {
        x: 0,
        y: 0,
        width: backend.display_w,
        height: backend.display_h,
    });
    let dt = nsec().unwrap_or(t0).saturating_sub(t0);
    *present_ns_sum = present_ns_sum.saturating_add(dt);
    *present_ns_max = (*present_ns_max).max(dt);
    *present_count += 1;
    if *present_count >= window {
        emit_present_stats(
            (*present_ns_sum / *present_count as u64 / 1000) as u32,
            (*present_ns_max / 1000) as u32,
            *present_count,
        );
        *present_count = 0;
        *present_ns_sum = 0;
        *present_ns_max = 0;
    }
}

/// One-shot boot diagnostic: wall-clock of the framebuffer handoff → display-ready
/// processing (attach backing + GL scanout + first textured/wallpaper present). This is the
/// `tail_ms` the init boot table attributes to the display chain; emitting it here localizes
/// whether that time is gpud's GL work vs gpud blocked waiting on present completion. No alloc.
fn emit_handoff_timing(ms: u32) {
    let mut buf = [0u8; 48];
    let mut p = 0usize;
    let put = |buf: &mut [u8; 48], p: &mut usize, s: &[u8]| {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    };
    let put_dec = |buf: &mut [u8; 48], p: &mut usize, mut v: u32| {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            if *p < buf.len() {
                buf[*p] = tmp[n];
                *p += 1;
            }
        }
    };
    put(&mut buf, &mut p, b"gpud: timing handoff_to_ready_ms=");
    put_dec(&mut buf, &mut p, ms);
    put(&mut buf, &mut p, b"\n");
    let _ = debug_write(&buf[..p]);
}

/// P0.3 honest-present marker: `gpud: FAIL present deadline (cmd=N)` — N
/// completion waits ran into the `GPU_WAIT_DEADLINE_NS` net during ONE present,
/// so its commands were abandoned by the ring's degraded recovery and the frame
/// is (partially) lost. The present is NACKed; windowd requeues the damage.
/// No-alloc (bump heap, degraded path may repeat).
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn emit_present_deadline_fail(expired: u32) {
    let mut buf = [0u8; 64];
    let mut p = 0usize;
    let put = |buf: &mut [u8; 64], p: &mut usize, s: &[u8]| {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    };
    let put_dec = |buf: &mut [u8; 64], p: &mut usize, mut v: u32| {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            if *p < buf.len() {
                buf[*p] = tmp[n];
                *p += 1;
            }
        }
    };
    put(&mut buf, &mut p, b"gpud: FAIL present deadline (cmd=");
    put_dec(&mut buf, &mut p, expired);
    put(&mut buf, &mut p, b")\n");
    let _ = debug_write(&buf[..p]);
}

fn emit_present_stats(avg_us: u32, max_us: u32, n: u32) {
    let mut buf = [0u8; 96];
    let mut p = 0usize;
    let put = |buf: &mut [u8; 96], p: &mut usize, s: &[u8]| {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    };
    let put_dec = |buf: &mut [u8; 96], p: &mut usize, mut v: u32| {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            if *p < buf.len() {
                buf[*p] = tmp[n];
                *p += 1;
            }
        }
    };
    put(&mut buf, &mut p, b"gpud: present us avg=");
    put_dec(&mut buf, &mut p, avg_us);
    put(&mut buf, &mut p, b" max=");
    put_dec(&mut buf, &mut p, max_us);
    put(&mut buf, &mut p, b" n=");
    put_dec(&mut buf, &mut p, n);
    // Reactive-completion health: waits woken by the GPU ring-buffer IRQ vs.
    // waits that ran into the 500ms deadline net. A healthy boot has dlx=0;
    // dlx climbing while irqw stays 0 = the IRQ path is wedged/unbound again.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    {
        put(&mut buf, &mut p, b" irqw=");
        put_dec(
            &mut buf,
            &mut p,
            crate::backend::IRQ_WAKE_COUNT.load(core::sync::atomic::Ordering::Relaxed),
        );
        put(&mut buf, &mut p, b" dlx=");
        put_dec(
            &mut buf,
            &mut p,
            crate::backend::IRQ_DEADLINE_EXPIRED_COUNT.load(core::sync::atomic::Ordering::Relaxed),
        );
    }
    put(&mut buf, &mut p, b"\n");
    let _ = debug_write(&buf[..p]);
}

fn decode_handoff_id_attach(frame: &[u8]) -> Option<u32> {
    nexus_display_proto::decode_handoff_id(frame)
}

fn decode_handoff_id_present(frame: &[u8]) -> Option<u32> {
    nexus_display_proto::decode_present_handoff_id(frame)
}

/// Extract bounding damage rect from ALL command types.
fn damage_rect_from_cb(cb: &CommittedBuffer, display_w: u32, display_h: u32) -> Rect {
    let mut min_x = display_w;
    let mut min_y = display_h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for cmd in cb.commands() {
        let (x, y, w, h) = match cmd {
            Command::BlitSurface { dst_x, dst_y, width, height, .. } => {
                (*dst_x, *dst_y, *width, *height)
            }
            // Absolute blits that target the display plane (e.g. the chat layer
            // composite, sidebar/button blur-cache restores) MUST contribute to
            // the present damage, or their region is written to the backing but
            // never transferred/flushed to the host. Convert the absolute dst row
            // back to screen-relative; ignore blits aimed elsewhere (atlas/cache).
            Command::BlitAbsolute { dst_x, dst_y_abs, width, height, .. } => {
                if *dst_y_abs >= DISPLAY_PLANE_ROW
                    && *dst_y_abs < DISPLAY_PLANE_ROW + display_h
                {
                    (*dst_x, dst_y_abs - DISPLAY_PLANE_ROW, *width, *height)
                } else {
                    continue;
                }
            }
            Command::FillSdfRoundedRect { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::FillSdfGradient { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::CompositeLayer {
                width,
                height,
                dst_x,
                dst_y,
                shadow_blur,
                shadow_offset_y,
                ..
            } => {
                // Damage the layer rect plus its shadow halo (blur + offset).
                let pad = *shadow_blur + shadow_offset_y.unsigned_abs();
                let x0 = dst_x.saturating_sub(pad);
                let y0 = dst_y.saturating_sub(pad);
                let x1 = (dst_x + width).saturating_add(pad);
                let y1 = (dst_y + height).saturating_add(pad);
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::DropShadow { rect, blur, offset_x, offset_y, .. } => {
                // The painted halo extends past the casting rect by blur,
                // shifted by the offset — damage the full extent (clamped).
                let pad = *blur as i32;
                let x0 = (rect.x as i32 + offset_x - pad).max(0) as u32;
                let y0 = (rect.y as i32 + offset_y - pad).max(0) as u32;
                let x1 = ((rect.x + rect.width) as i32 + offset_x + pad).max(0) as u32;
                let y1 = ((rect.y + rect.height) as i32 + offset_y + pad).max(0) as u32;
                (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
            }
            Command::BlurBackdrop { rect, .. } => (rect.x, rect.y, rect.width, rect.height),
            Command::BlendCursor { x, y, width, height } => (*x, *y, *width, *height),
            _ => continue,
        };
        let ex = x.saturating_add(w);
        let ey = y.saturating_add(h);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(ex);
        max_y = max_y.max(ey);
        found = true;
    }
    if found {
        // Clamp to the display plane — halo-style commands (DropShadow) may
        // extend past the screen edges.
        let min_x = min_x.min(display_w);
        let min_y = min_y.min(display_h);
        let max_x = max_x.min(display_w);
        let max_y = max_y.min(display_h);
        Rect {
            x: min_x,
            y: min_y,
            width: max_x.saturating_sub(min_x).max(1),
            height: max_y.saturating_sub(min_y).max(1),
        }
    } else {
        Rect { x: 0, y: 0, width: display_w, height: display_h }
    }
}

/// Handle an `OP_UPLOAD_CURSOR` payload: on the CPU/mmio scanout, arm the virtio-gpu
/// **hardware cursor overlay** (cursor virtqueue) so the host composites the pointer at
/// scanout — cursor moves then never touch windowd's present pipeline (reactive, decoupled).
/// Reply `CURSOR_REPLY_HW` so windowd suppresses its software BlendCursor.
///
/// On the virgl GL scanout `upload_cursor`'s `transfer_to_host` blanks the present, so there
/// (and if arming the overlay fails for any reason) we fall back to storing the sprite for
/// windowd's BlendCursor and reply `CURSOR_REPLY_SW` — preserving the prior behaviour.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn arm_cursor(
    backend: &mut VirtioGpuBackend,
    bgra: &[u8],
    w: u32,
    h: u32,
    hot_x: u32,
    hot_y: u32,
) -> (u8, Option<u32>) {
    #[cfg(not(feature = "virgl"))]
    if backend.upload_cursor(bgra, w, h, hot_x, hot_y).is_ok() {
        let _ = debug_println("gpud: hw cursor armed");
        return (STATUS_OK, Some(CURSOR_REPLY_HW));
    }
    // virgl GL scanout, or HW arm failed: the GL/SW draw subtracts the
    // hotspot, so record it (resize shapes center it at 16,16).
    backend.set_cursor_hot(hot_x, hot_y);
    // On virgl the build-up present owns the scanout and draws a procedural
    // cursor at `cursor_ox/oy` — reply GL so windowd ships moves + a present
    // (its software BlendCursor into the VMO would be ignored here). Elsewhere
    // (HW arm failed) fall back to windowd's BlendCursor (SW).
    #[cfg(feature = "virgl")]
    const NON_HW_REPLY: u32 = CURSOR_REPLY_GL;
    #[cfg(not(feature = "virgl"))]
    const NON_HW_REPLY: u32 = CURSOR_REPLY_SW;
    match backend.store_cursor_sprite(bgra, w, h) {
        Ok(()) => {
            // Pointer-shape switch (TASK-0070 Phase 3): if the GL cursor
            // texture is already live, refresh it from the new sprite now
            // (outside any present batch) so the shape changes immediately.
            #[cfg(feature = "virgl")]
            let _ = backend.cursor_tex_refresh();
            let _ = debug_println("gpud: cursor uploaded");
            (STATUS_OK, Some(NON_HW_REPLY))
        }
        Err(_) => (STATUS_DEVICE_ERROR, None),
    }
}

fn present_scanout_damage(backend: &mut VirtioGpuBackend, rect: Rect) -> u8 {
    match backend.present_scanout_damage(rect) {
        Ok(()) => STATUS_OK,
        Err(e) => {
            let _ = debug_println("gpud: present scanout damage FAIL");
            match e {
                GfxError::InvalidArgument => {
                    let _ = debug_println("gpud: scanout InvalidArgument (no scanout resource?)");
                }
                GfxError::ResourceExhausted => {
                    let _ = debug_println("gpud: scanout ResourceExhausted");
                }
                _ => {}
            }
            STATUS_DEVICE_ERROR
        }
    }
}

fn handle_present_damage(backend: &mut VirtioGpuBackend, frame: &[u8]) -> u8 {
    if frame.len() < 17 {
        return STATUS_MALFORMED;
    }
    let x = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
    let y = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
    let width = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
    let height = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
    present_scanout_damage(backend, Rect { x, y, width, height })
}

fn handle_frame(backend: &mut VirtioGpuBackend, frame: &[u8], scroll_flush: &mut bool) -> u8 {
    let Some(op) = frame.first().copied() else {
        return STATUS_MALFORMED;
    };
    match op {
        OP_SUBMIT_ANIMATION_FRAME => {
            // Animation frames carry a serialized CommittedBuffer after the opcode.
            // Deserialize and submit to the GPU backend for execution.
            if frame.len() <= 1 {
                return STATUS_MALFORMED;
            }
            match CommittedBuffer::deserialize_from(&frame[1..]) {
                Ok((cmd, _consumed)) => {
                    let _ = backend.submit(cmd);
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
            match backend.cache_cursor_shape(shape_id, &frame[18..], w, h, hot_x, hot_y) {
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
            match backend.select_cursor_shape(frame[1]) {
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
            // Record the pointer position for the GL-scanout fallback cursor (the
            // Stage-4 build-up draws a procedural arrow at cursor_ox/oy each present
            // — no transfer_to_host, so it is safe on the virgl GL scanout, unlike
            // the hardware-cursor overlay whose resource transfer blanks the GL
            // present). windowd also sends OP_PRESENT_DAMAGE on move, re-rendering.
            backend.set_pointer_pos(x, y);
            // HW cursor overlay armed → reposition via the cursor virtqueue
            // (submit-no-response): no scanout re-render, no present, no per-move log.
            // This is the reactive hot path — cursor moves are fully decoupled from
            // compositing.
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            if backend.hw_cursor_active() && x >= 0 && y >= 0 {
                return match backend.move_hw_cursor(x as u32, y as u32) {
                    Ok(()) => STATUS_OK,
                    Err(_) => STATUS_DEVICE_ERROR,
                };
            }
            // Legacy save-under SW path (no-op while cursor ownership is unclaimed).
            if backend.cursor_move(x, y).is_err() {
                return STATUS_DEVICE_ERROR;
            }
            STATUS_OK
        }
        OP_SET_LAYER_SCROLL => {
            if frame.len() < 9 {
                return STATUS_MALFORMED;
            }
            let scroll_id = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
            let src_row = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            // RECORD the override only — the service loop drains the whole queued
            // burst (latest row wins) and re-composites ONCE via
            // `flush_layer_scroll` when the queue is empty. Presenting per request
            // turned a fling into a backlog of full re-composites of stale
            // positions (seconds of dead UI).
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            {
                return match backend.record_layer_scroll(scroll_id, src_row) {
                    Ok(()) => {
                        *scroll_flush = true;
                        STATUS_OK
                    }
                    Err(_) => STATUS_DEVICE_ERROR,
                };
            }
            #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
            {
                let _ = (backend, scroll_id, src_row, scroll_flush);
                STATUS_OK
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
            #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
            {
                let t = crate::backend::LayerTransform { dx, dy, opacity, scale_pct };
                return match backend.record_layer_transform(layer_id, t) {
                    Ok(()) => {
                        *scroll_flush = true;
                        STATUS_OK
                    }
                    Err(_) => STATUS_DEVICE_ERROR,
                };
            }
            #[cfg(not(all(feature = "virgl", feature = "os-lite", target_os = "none")))]
            {
                let _ = (backend, layer_id, dx, dy, opacity, scale_pct, scroll_flush);
                STATUS_OK
            }
        }
        OP_PRESENT_DAMAGE => {
            // A full present composites the recorded scroll/transform overrides
            // anyway — the deferred flush would be a redundant re-composite.
            *scroll_flush = false;
            handle_present_damage(backend, frame)
        }
        _ => STATUS_MALFORMED,
    }
}
