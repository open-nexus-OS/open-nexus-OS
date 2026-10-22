// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the app-host probe — bring-up, payload fetch, mount, the present
//! loop and the input/scroll/locale/clock plumbing around it. Lifted out of
//! `main.rs`, where it lived as a ~1000-line INLINE `mod probe { … }` beside
//! the `probe/` directory its own submodules already used (structure-gate).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers (`APPHOST: …`)

use nexus_abi::{cap_clone, debug_println, nsec, vmo_create, vmo_write, yield_};

/// Probe markers must NOT fold: `nexus-service-entry` arms verdict
/// folding for every process it bootstraps, so `debug_println` swallows
/// non-FAIL lines in interactive boots (recall-only). The R1 proof chain
/// goes through the raw write syscall instead.
mod marker;
pub(crate) use marker::raw_marker;
use nexus_display_proto::client_surface as wire;

use nexus_ipc::{Client as _, KernelClient, Wait};

mod anim;
mod band;
mod boot;
mod clock;
mod drag;
mod env;
mod frame_arena;
mod interaction;
mod layers;
mod locale;
mod mount;
mod overlay;
mod paint;
mod presentation;
mod scroll;
mod state;
mod windows; // RFC-0086 feed intake // RFC-0095: hover, leave and the drag gesture
use boot::*;
pub(crate) use env::{device_for, size_class_for, tokens_for};
pub(crate) use interaction::TapOutcome;
pub(crate) use locale::app_locale;
use paint::collect_texts;
use state::DslApp;

/// Fixed child capability slots — execd transfers these AFTER spawn
/// (`cap_transfer_to_slot`): SEND on windowd's server endpoint into 5,
/// RECV on windowd's shared response endpoint into 6 (the inputd slot
/// convention). The child may run before the transfer lands, so every
/// first use retries bounded (the #123 empty-slot lesson).
const WINDOWD_SEND_SLOT: u32 = nexus_service_topology::slots::app_child::WINDOWD.send;
const WINDOWD_RECV_SLOT: u32 = nexus_service_topology::slots::app_child::WINDOWD.recv;
/// The app's DEDICATED event channel (ADR-0042): windowd delivers input
/// events AND surface acks here — the shared response endpoint (slot 6)
/// raced with inputd's ack drain, so a tap sent there could be consumed
/// by any receiver. Slot 6 stays as the fallback for older wiring
/// (marked).
const EVENTS_RECV_SLOT: u32 = nexus_service_topology::slots::app_child::EVENTS_RECV;
/// The app's timer-notify pair (TASK-0324 P7-d, minted by execd): the clock's one-shot
/// timer fires a frame on RECV, a waitset member next to the event channel.
/// The app's declared timer-notify pair (TASK-0324 P7-d): its clock's one-shot rides here.
const TIMER: nexus_service_topology::SlotPair = nexus_service_topology::slots::app_child::TIMER;

// The embedded fallback payload is DELETED (separation of concerns):
// program bytes belong to bundlemgrd (the registry) ONLY. A missing/broken
// payload VMO is a LOUD failure in the log (FAIL markers below) and shows
// NO window at all — never a silently different program, and no longer the
// teal probe fill either (a fail-open ghost window titled "App").

/// Fixed child slot holding the payload VMO (execd's
/// `CHILD_PAYLOAD_SLOT`); bundlemgrd fills it and writes the 16-byte
/// header LAST (`nexus_abi::payload_vmo::encode_header`).
const PAYLOAD_VMO_SLOT: u32 = nexus_service_topology::slots::app_child::PAYLOAD_VMO;
/// SEND-side clone of OUR OWN event channel (execd grants it alongside the
/// RECV side): the app-host attaches it to windowd ITSELF, tagged with a
/// self-minted nonce that SURFACE_CREATE repeats — windowd binds
/// channel↔surface by nonce (deterministic under concurrent connects).
const EVENTS_SEND_CLONE_SLOT: u32 = nexus_service_topology::slots::app_child::EVENTS_SEND;
/// Header-poll budget: the fetch is kicked BEFORE our ELF even loads, so
/// the header normally beats us; the budget only bounds failure. 3s→8s
/// (RFC-0075 Phase 8d): the CJK-atlas-grown image lengthened early-boot
/// loads enough that the grant lagged 3s on busy boots (same class as
/// the content-rect budget below).
const PAYLOAD_BUDGET_NS: u64 = 8_000_000_000;
/// Upper payload bound accepted from the header (matches execd's VMO
/// budget; anything larger is a malformed header by contract). Raised with
/// execd's `PAYLOAD_VMO_BYTES` 256K→512K — settings' NXLC container hit 93%
/// of the old bound.
const PAYLOAD_MAX_LEN: usize = 512 * 1024;

mod boot_env;
use boot_env::{map_atlas_base, nsec_now, payload_addr};

pub(super) fn run() -> Result<(), &'static str> {
    let start_ns = boot_env::mark_start();
    // RFC-0080: install the shared atlas BEFORE any text renders.
    map_atlas_base();

    // 1. windowd client + the app's DEDICATED event channel come up FIRST:
    //    the geometry handshake's content-rect reply (and later acks/input)
    //    arrive on the event channel, before any surface exists.
    let client = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
        .map_err(|_| "apphost: client slots")?;
    let (events, events_recv_slot) = match cap_clone(EVENTS_RECV_SLOT) {
        Ok(probe) => {
            let _ = nexus_abi::cap_close(probe);
            raw_marker("APPHOST: events source=dedicated");
            let c = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, EVENTS_RECV_SLOT)
                .map_err(|_| "apphost: event slots")?;
            (c, EVENTS_RECV_SLOT)
        }
        Err(_) => {
            raw_marker("APPHOST: events source=shared (fallback)");
            let c = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
                .map_err(|_| "apphost: event slots")?;
            (c, WINDOWD_RECV_SLOT)
        }
    };
    // TASK-0324 P7-d: ONE waitset over the event channel and the timer-notify endpoint. The
    // clock (minute boundary) is a kernel one-shot timer armed at the exact deadline;
    // animations ride the compositor's frame pulse EXCLUSIVELY (a pulse request is a waited
    // send, never dropped) — no 12 ms self-pace, no recv timeout. A closed window (RFC-0079
    // EOF) reaches the waitset like any receiver: the member reads ready, the recv says so.
    let mut timer = nexus_ipc::timer::NotifyTimer::bind(TIMER).ok();
    let waitset = nexus_abi::waitset_create().ok().and_then(|ws| {
        nexus_abi::waitset_add(ws, events_recv_slot).ok()?;
        nexus_abi::waitset_add(ws, TIMER.recv).ok()?;
        Some(ws)
    });
    if timer.is_none() || waitset.is_none() {
        raw_marker("apphost: FAIL waitset/timer (blocking on the event channel alone)");
    }

    // 1a. Attach OUR event channel to windowd, tagged with a self-minted
    //     nonce (repeated on SURFACE_CREATE): windowd binds channel↔surface
    //     by nonce, never by arrival order — N app-hosts may connect
    //     concurrently (the greeter/shell/counter channel-crossing bug).
    let nonce: u64 = nsec().unwrap_or(0) ^ ((payload_addr() as u64) << 16) ^ 0x9E37_79B9;
    match cap_clone(EVENTS_SEND_CLONE_SLOT) {
        Ok(clone) => {
            let frame = wire::encode_surface_events(nonce);
            // The moved cap is this app's EVENT channel — data, not a reply inbox; windowd
            // binds it to the surface by nonce and pushes into it (TASK-0054C P2-d). WAIT for
            // queue space — no clock; a dead windowd ends the wait.
            match nexus_ipc::exchange::send_with_cap(WINDOWD_SEND_SLOT, &frame, clone) {
                Ok(_) => {
                    raw_marker("APPHOST: events attached (nonce)");
                    // RFC-0079: relinquish our OWN send cap to our event
                    // inbox — windowd is the sole sender now. Otherwise
                    // this leftover SEND cap keeps the last-sender scan
                    // non-zero and the window-close EOF never fires.
                    let _ = nexus_abi::cap_close(EVENTS_SEND_CLONE_SLOT);
                }
                Err(_) => raw_marker("APPHOST: FAIL events attach (send)"),
            }
        }
        Err(_) => raw_marker("APPHOST: FAIL events attach (no send clone)"),
    }

    // 1b. Theme: the compositor pushes the active mode (`OP_SURFACE_THEME`)
    //     when the event channel attaches — capture it BEFORE mount so the
    //     app renders with the same tokens as the desktop.
    // Attach-time region push: stashed by the pre-mount drains, applied
    // right after mount (windowd re-pushes only on change).
    let mut boot_region: Option<boot::RegionPush> = None;
    let (mut theme_mode, mut shell_profile, boot_settings_gen) =
        wait_for_boot_pushes(&events, &mut boot_region);

    // 2. The DSL payload + its window intent → the geometry handshake. The
    //    WM owns geometry: a desktop/full-screen surface asks windowd for
    //    its content rect (`chrome = intent ⟂ policy`); a normal app uses
    //    the probe default. Fail-soft — if windowd does not answer, default.
    // No payload = LOUD visible failure: mount(&[]) fails and the probe
    // fill renders (never a silently substituted program).
    let payload = resolve_payload().unwrap_or(&[]);
    // RFC-0077: the payload may be an NXLC container — the intent tags
    // live in the NXIR inside (raw container bytes read as defaults).
    let (style, level, mode) = read_window_intent_tags(locale::payload_nxir(payload));
    // Declared resize intent: floating windows are resizable; a desktop/
    // fullscreen surface is not (the presentation resolver enforces this
    // WM-side too). Carried atomically on SURFACE_CREATE.
    let resizable = level != wire::WIN_LEVEL_DESKTOP
        && level != wire::WIN_LEVEL_OVERLAY
        && mode != wire::WIN_MODE_FULLSCREEN;
    let win = WindowIntent { style, level, mode, nonce };
    // The WM owns every window's geometry (TASK-0336): an `auto` window asks too — the ask
    // shows the launch's starting surface.
    let (mut safe_top, mut surf_w, mut surf_h) =
        compositor_owned_geometry(&client, &events, &win, &mut boot_region)?;
    // Content rect arriving DURING an ack wait (windowd's corrective push
    // after a small create) — stashed by `recv_ack` instead of dropped,
    // applied by the event loop as if it had just been received.
    let mut pending_rect: Option<(u16, u16, u16)> = None;
    // The last-APPLIED region push (locale/tz/keymap/hour). REMOUNTS
    // (theme toggle, profile switch) rebuild the DslApp from the payload
    // and must re-apply it — or a tablet/theme switch silently falls
    // back to the baked English catalog (windowd re-pushes only on
    // CHANGE, never on remount).
    let mut last_region: Option<boot::RegionPush> = None;
    // RFC-0083: last applied snapshot gen (dedupe only; apply is
    // per-field idempotent), seeded from the consumed boot snapshot.
    let mut last_settings_gen: Option<u32> = boot_settings_gen;

    // 4. Mount the DSL program FIRST (before the VMO) so its scroll-region
    //    geometry decides the VMO size. The DSL lays out at the VISIBLE
    //    surface size; a windowed page with a scroll region then uses the
    //    WebRender compositor-scroll path (a TALL packed band, rendered once).
    // Declarative base alpha: a DESKTOP surface paints a fully
    // TRANSPARENT base — windowd alpha-blends the band over the retained
    // wallpaper plane, so empty desktop area IS the wallpaper (elements
    // paint their own fills; the shell must not lay `.bg()` over the whole
    // page). A fullscreen FLOATING surface (kiosk app) stays opaque — it
    // owns every pixel. Normal floating windows keep the frosted glass.
    let base_alpha: u8 = if level == wire::WIN_LEVEL_DESKTOP {
        0
    } else if level == wire::WIN_LEVEL_OVERLAY || mode == wire::WIN_MODE_FULLSCREEN {
        // The OSK band (overlay) paints opaque — keys must never blend
        // with the content below (glass material = follow-up).
        255
    } else {
        // Frosted floating window: the page base leaves ~1/3 of the blurred
        // backdrop visible (the material look lives or dies on this — 190
        // read as a solid slab; opaque ELEMENTS still paint fully on top).
        168
    };
    let mut app = DslApp::mount(payload, surf_w, surf_h, theme_mode, shell_profile, base_alpha);
    boot_env::mark_stage(0, start_ns);
    if let Some(dsl) = app.as_mut() {
        dsl.apply_safe_area_top(safe_top);
    }
    // Drain the still-queued attach-burst REGION frame before the first
    // render (see `boot::drain_region` — the launched-chat "English
    // despite de-DE" gap).
    boot::drain_region(&events, &mut boot_region);
    if let (Some(dsl), Some(r)) = (app.as_mut(), boot_region.take()) {
        let _ = dsl.apply_region(r.hour_fmt, &r.locale, &r.tz, &r.keymap);
        last_region = Some(r);
    }
    // 3. The app's own surface VMO: its base rows and its STORE (RFC-0109) — the wash patch
    //    and every scroll layer's band, planned from the page's layers. Mutable: a WM resize (or
    //    a store that outgrew it) re-creates it. No device reads it (windowd copies it with
    //    `vmo_read`): an anonymous object.
    let plan = app.as_mut().map_or(band::StorePlan { rows: surf_h, width: 0 }, |d| d.plan_store());
    let rows = plan.rows;
    let mut vmo = boot_env::surface_vmo(surf_w as usize * rows as usize * 4, start_ns)
        .map_err(|_| "apphost: vmo create failed")?;
    // The first frame: the base, the patch, every band's visible window.
    let mut first = band::PresentDamage::default();
    let first_render_ok = app.as_mut().is_some_and(|dsl| {
        dsl.store_rows = rows;
        dsl.redraw(vmo, None, &mut first)
    });
    if !(app.is_some() && first_render_ok) {
        // FAIL-CLOSED: no program, no window. The failure stays LOUD in the log (marker below +
        // the payload-path FAIL markers above); the surface stays unclaimed so windowd shows
        // nothing, and the process exits for execd to reap.
        raw_marker("APPHOST: FAIL dsl mount (no window, fail-closed)");
        return Err("apphost: dsl mount failed");
    }
    raw_marker("APPHOST: dsl frame rendered");
    boot_env::mark_stage(1, start_ns);
    raw_marker("apphost: vmo filled");

    // 5. SURFACE_CREATE — a CLONE of the VMO cap moves with the message (the gpud-attach
    //    pattern); the original stays ours for redraws. The create carries the visible size and
    //    the surface's rows (base + store) — windowd allocates the atlas region up front.
    let clone = cap_clone(vmo).map_err(|_| "apphost: cap clone failed")?;
    send_wait_cap(&client, &boot::create_frame((surf_w, surf_h), &win, resizable, plan), clone)?;
    let ack = recv_ack(&events, wire::OP_SURFACE_CREATE, &mut pending_rect, &mut boot_region)?;
    let mut surface_id = ack.value;
    if let Some(dsl) = app.as_mut() {
        dsl.set_surface_id(surface_id);
        dsl.taps_are_keystrokes = level == wire::WIN_LEVEL_OVERLAY;
        // No store placed (`NO_STORE`): the page again, its scroll layers flat.
        if dsl.created(ack.store, plan) && !dsl.redraw(vmo, None, &mut first) {
            return Err("apphost: flat redraw failed");
        }
    }
    raw_marker("APPHOST: surface created");
    boot_env::mark_stage(2, start_ns);

    // 6. The structure BEFORE the first present (windowd clears it at create: after the ack) —
    //    the first composite is the whole window with its layers — then SURFACE_PRESENT seq=1 of
    //    every row drawn, strictly one in flight, and the motion (offsets, no wash yet).
    if let Some(dsl) = app.as_mut() {
        dsl.send_tree(&client, surface_id);
        dsl.send_present(&client, surface_id, 1, &first)?;
    }
    let _ = recv_ack(&events, wire::OP_SURFACE_PRESENT, &mut pending_rect, &mut boot_region)?;
    if let Some(dsl) = app.as_mut() {
        dsl.send_motion(&client, surface_id);
    }
    boot_env::mark_first_frame(start_ns);
    if let Some(dsl) = app.as_ref() {
        // Mount-time `.transition` enter animations were seeded in
        // `anim_sync`: arm the frame pulse so they play from the first
        // frame (value/effect tokens are inert until a state change).
        if dsl.anim_active() || dsl.bands_pending() {
            let req = wire::encode_surface_frame_req(surface_id);
            let _ = client.send(&req, Wait::Blocking);
        }
    }

    // 5. The event loop (R3): ONE unified BLOCKING recv on the app event
    //    channel. windowd delivers BOTH body taps (`OP_SURFACE_INPUT`,
    //    surface-local coordinates) AND present-acks here.
    //
    //    The earlier design did `dsl.tap → render → present → recv_ack`,
    //    where `recv_ack` blocked the loop draining the SAME channel and
    //    DISCARDED any input frame that interleaved with the ack ("keep
    //    waiting"). Result: the first tap worked, every tap arriving
    //    during a present's ack-wait was silently dropped — the "+ reacts
    //    only once" bug (counter repro 2026-07-07). It also stalled 30s
    //    when the ack raced behind queued taps.
    //
    //    Fixed design — never drop a tap, decouple the present:
    //    * every tap is applied to the MODEL immediately (the counter
    //      increments even if the display lags);
    //    * a present-ack is pure flow control (clears `present_in_flight`);
    //    * at most one present is outstanding; taps that arrive while one
    //      is in flight set `dirty`, and the next ack triggers a single
    //      coalesced present of the latest state.
    //    Plain blocking recv (P0.2): the sender-wake of an exec'd child in
    //    blocking recv is proven every boot by the recv-wake gate.
    let mut seq: u32 = 1;
    let mut event_frame = [0u8; 64];
    let mut recv_err_marked = false;
    let mut odd_frame_markers: u32 = 0;
    let mut tap_miss_markers: u32 = 0;
    let mut wheel_rx_markers: u32 = 0;
    let mut present_in_flight = false;
    let mut dirty = false;
    // Damage discipline (5K/120Hz contract): `None` = full repaint
    // (mount/resize/theme), `Some((y0, y1))` = only those LAYOUT rows are
    // redrawn — in the base and in the bands that hold them (RFC-0109).
    // Spans from coalesced events union; any full request wins.
    let mut dirty_rows: Option<(i32, i32)> = None;
    raw_marker("APPHOST: event loop armed");
    // The app is up: fold the bring-up markers into their verdict line.
    nexus_abi::service_verdict_flush("app-host");
    loop {
        // A rect stashed during an ack wait (`recv_ack`) is replayed here
        // as if it had just been received — same resize path, no drop.
        let len = if let Some((inset, rw, rh)) = pending_rect.take() {
            // Replay the stashed rect VERBATIM — dropping `inset` here would
            // lose the safe area on exactly the create-time corrective push
            // that this stash exists to rescue.
            let f = wire::encode_surface_rect(0, inset, rw, rh);
            event_frame[..f.len()].copy_from_slice(&f);
            f.len()
        } else {
            // The wait — no clock: the timer for the clock (if the app declares one), then
            // the waitset; the recv that follows is non-blocking and EOF-opted.
            if let (Some(t), Some(dsl)) = (timer.as_mut(), app.as_ref()) {
                t.arm_at(dsl.timer_deadline_ns());
            }
            let wait = match waitset {
                Some(ws) => {
                    let _ = nexus_abi::waitset_wait(ws, 0);
                    Wait::NonBlocking
                }
                None => Wait::Blocking,
            };
            match events.recv_into_eof(wait, &mut event_frame) {
                Ok(len) => {
                    recv_err_marked = false;
                    len
                }
                Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {
                    // Not an event: the one-shot fired (drained; the kernel disarmed it) — or
                    // a spurious wake, which is nothing. `timer_fired` runs the due deadline(s).
                    if !timer.as_mut().is_some_and(|t| t.drain()) {
                        continue;
                    }
                    if let Some(dsl) = app.as_mut() {
                        if dsl.timer_fired() {
                            dirty = true;
                            dirty_rows = None;
                        }
                        // A grown store waits for the re-create (a pulse wakes the loop's tail).
                        if dsl.store_replan {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::Blocking);
                        } else if dirty && !present_in_flight {
                            match dsl.present_frame(
                                &client,
                                surface_id,
                                vmo,
                                &mut seq,
                                Some(dirty_rows),
                            ) {
                                Ok(sent) => present_in_flight |= sent,
                                Err(m) => raw_marker(m),
                            }
                            dirty = false;
                            dirty_rows = None;
                        }
                    }
                    continue;
                }
                Err(nexus_ipc::IpcError::Disconnected)
                | Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)) => {
                    // The compositor released our event channel: the window is
                    // gone (user close). The app's lifetime IS its window —
                    // exit cleanly so the kernel frees the process (the
                    // app-side half of the reaper, #29). Spinning on the dead
                    // channel would burn the core forever instead.
                    raw_marker("APPHOST: window closed - exiting");
                    return Ok(());
                }
                Err(_) => {
                    if !recv_err_marked {
                        recv_err_marked = true;
                        raw_marker("apphost: FAIL event recv (yield pacing)");
                    }
                    let _ = yield_();
                    continue;
                }
            }
        };
        // Shared surface re-create trigger (WM resize / band change).
        let mut recreate_surface = false;
        // Classify the frame: present-ack (flow control) vs input vs theme vs other.
        if wire::decode_surface_ack(&event_frame[..len], wire::OP_SURFACE_PRESENT).is_some() {
            present_in_flight = false;
        } else if let Some((_, inset, rw, rh)) = wire::decode_surface_rect(&event_frame[..len]) {
            // WM resize (the compositor owns geometry): re-layout at the
            // new size, then run the SHARED surface re-create below.
            let (nw, nh) = (u32::from(rw), u32::from(rh));
            // The safe area rides the SAME frame as the size, so a maximize
            // never shows one without the other.
            if u32::from(inset) != safe_top {
                safe_top = u32::from(inset);
                if let Some(dsl) = app.as_mut() {
                    dsl.apply_safe_area_top(safe_top);
                }
                recreate_surface = true;
            }
            if nw > 0 && nh > 0 && (nw, nh) != (surf_w, surf_h) {
                surf_w = nw;
                surf_h = nh;
                if let Some(dsl) = app.as_mut() {
                    // Mobile-first breakpoints: a resize that crosses a
                    // width class (compact/regular/wide) changes the
                    // PAGE STRUCTURE (`if device.sizeClass` arms) — a
                    // plain relayout keeps the old arm, so re-emit the
                    // scene at the new class first. State survives (same
                    // View, no remount); the relayout below reflows it.
                    if size_class_for(dsl.w) != size_class_for(nw) {
                        dsl.reemit_for_size_class(nw);
                    }
                    dsl.resize(surf_w, surf_h);
                }
                recreate_surface = true;
            }
        } else if wire::decode_surface_frame(&event_frame[..len]).is_some() {
            // Compositor frame pulse (Choreographer): advance the scroll
            // physics AND the DSL animation physics one REAL frame, and
            // re-arm while either is still in motion.
            if let Some(dsl) = app.as_mut() {
                // The scroll layers' physics: offsets committed as motion, never a raster; an
                // `EndReached` that loaded more is a model change.
                if dsl.momentum_tick() {
                    dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
                }
                // Animation tick: damage EXACTLY the animated nodes' union
                // row span (old ∪ new transformed AABB) — the 120Hz damage
                // contract; a full repaint per breathe tick starved the
                // input path. Unions with any scroll span; a pending full
                // request still wins.
                if let Some(span) = dsl.anim_tick() {
                    crate::layout_diff::merge_span(&mut dirty, &mut dirty_rows, span);
                }
                if dsl.momentum_active() || dsl.anim_active() || dsl.bands_pending() {
                    let req = wire::encode_surface_frame_req(surface_id);
                    let _ = client.send(&req, Wait::Blocking);
                }
            }
        } else if let Some((_, kind, x, y)) = wire::decode_surface_input(&event_frame[..len]) {
            if matches!(
                kind,
                wire::INPUT_KIND_MOVE
                    | wire::INPUT_KIND_LEAVE
                    | wire::INPUT_KIND_DRAG
                    | wire::INPUT_KIND_RELEASE
            ) {
                // Hover, leave and the drag gesture (`probe/drag.rs`).
                if let Some(dsl) = app.as_mut() {
                    let (x, y) = (i32::from(x), i32::from(y));
                    dsl.pointer_frame(&client, surface_id, kind, x, y, &mut dirty, &mut dirty_rows);
                }
            } else if kind == wire::INPUT_KIND_WHEEL {
                // Wheel impulse into the physics of the scroll layer under the pointer: motion,
                // never a raster (`probe/scroll.rs`). A page trigger is a model change.
                if let Some(dsl) = app.as_mut() {
                    if dsl.wheel_event(&client, surface_id, y, &mut wheel_rx_markers) {
                        dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
                    }
                }
            } else if kind == wire::INPUT_KIND_TAP {
                if let Some(dsl) = app.as_mut() {
                    let outcome = dsl.tap(i32::from(x), i32::from(y));
                    dsl.announce_text_focus(&client, surface_id, i32::from(x), i32::from(y));
                    // ONLY a repaint is dirty. A tap a handler absorbed
                    // without one (`PanelNoop`, or a control whose effect
                    // lands in another service) is NOT a miss; calling it
                    // one sent readers hunting a hit-test bug for days.
                    if outcome == TapOutcome::Repainted {
                        // Damage-bounded structural repaint (`layout_diff`):
                        // full-frame rasters per click are what made menus
                        // feel like they never opened on the TCG boot.
                        dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
                        // Tap may have started an animation or pager glide.
                        if dsl.anim_active() || dsl.momentum_active() {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::Blocking);
                        }
                    } else if outcome == TapOutcome::NoHandler && tap_miss_markers < 8 {
                        tap_miss_markers += 1;
                        raw_marker(&alloc::format!("apphost: input tap miss at ({x},{y})"));
                        if tap_miss_markers == 1 {
                            if let Some(dsl) = app.as_ref() {
                                dsl.dump_handler_boxes();
                            }
                        }
                    }
                }
            }
        } else if app.as_mut().is_some_and(|d| d.absorb_shell_push(&event_frame[..len])) {
            if let Some(dsl) = app.as_mut() {
                dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
            }
        } else if let Some(snap) =
            nexus_display_proto::surface_settings::decode_surface_settings(&event_frame[..len])
        {
            // RFC-0083: the versioned presentation snapshot. Every change
            // is a REEMIT (probe/presentation.rs), never a remount.
            if presentation::absorb_snapshot(
                app.as_mut(),
                &snap,
                &mut last_settings_gen,
                &mut theme_mode,
                &mut shell_profile,
                &mut last_region,
            ) {
                dirty = true;
                dirty_rows = None; // presentation change: full repaint
            }
        } else if let Some(code) =
            nexus_display_proto::surface_windows::decode_surface_tile_preview(&event_frame[..len])
        {
            // TASK-0066: the zone a window drag would tile into — the shell draws it.
            if app.as_mut().is_some_and(|d| d.apply_tile_preview(code)) {
                dirty = true;
                dirty_rows = None;
            }
        } else if nexus_display_proto::surface_text::decode_surface_text(&event_frame[..len])
            .is_some()
        {
            if app.as_mut().is_some_and(|d| d.apply_surface_text(&event_frame[..len])) {
                dirty = true;
                dirty_rows = None; // model change: full repaint
            }
        } else if nexus_display_proto::surface_text::decode_ime_state(&event_frame[..len]).is_some()
        {
            // Composition-strip push (RFC-0075 Phase 3, ime-ui only —
            // apps without the event ignore it).
            if app.as_mut().is_some_and(|d| d.apply_ime_state(&event_frame[..len])) {
                dirty = true;
                dirty_rows = None;
            }
        } else if odd_frame_markers < 8 {
            // Unrelated frame — bounded marker WITH IDENTITY (magic/op/
            // len): seven anonymous skips cost a debugging session.
            odd_frame_markers += 1;
            let (m0, m1, op) = (
                event_frame.first().copied().unwrap_or(0),
                event_frame.get(1).copied().unwrap_or(0),
                event_frame.get(3).copied().unwrap_or(0),
            );
            raw_marker(&alloc::format!(
                "apphost: event frame skipped m={m0:02x}{m1:02x} op={op} len={len}"
            ));
        }
        // A layer set whose bands outgrew the surface's rows (planned at the relayout)
        // re-creates the surface (same flow as a WM resize, same size).
        if app.as_mut().is_some_and(|dsl| core::mem::take(&mut dsl.store_replan)) {
            recreate_surface = true;
        }
        // SHARED surface re-create (WM resize / a store that outgrew it): plan the store, new
        // VMO, destroy + create + the structure + a full present.
        if recreate_surface && app.is_some() {
            let flat = band::StorePlan { rows: surf_h, width: 0 };
            let plan = app.as_mut().map_or(flat, |d| d.plan_for_create());
            if let Ok(nv) = vmo_create(surf_w as usize * plan.rows as usize * 4) {
                let _ = send_wait(&client, &wire::encode_surface_destroy(surface_id));
                let _ = nexus_abi::cap_close(vmo);
                vmo = nv;
                if let Ok(clone) = cap_clone(vmo) {
                    let create = boot::create_frame((surf_w, surf_h), &win, resizable, plan);
                    if send_wait_cap(&client, &create, clone).is_ok() {
                        let mut late_region: Option<boot::RegionPush> = None;
                        let ack = recv_ack(
                            &events,
                            wire::OP_SURFACE_CREATE,
                            &mut pending_rect,
                            &mut late_region,
                        );
                        if let (Some(dsl), Some(r)) = (app.as_mut(), late_region.take()) {
                            let _ = dsl.apply_region(r.hour_fmt, &r.locale, &r.tz, &r.keymap);
                            last_region = Some(r);
                        }
                        if let Ok(ack) = ack {
                            surface_id = ack.value;
                            if let Some(dsl) = app.as_mut() {
                                dsl.set_surface_id(surface_id);
                                // Everything is drawn again below — flat without a store.
                                let _ = dsl.created(ack.store, plan);
                                // The new surface declares its layers anew (windowd cleared
                                // them at create).
                                dsl.tree_dirty = true;
                                dsl.motion_dirty = true;
                            }
                            // The fresh surface's seq restarts at 0 on the windowd side (strict
                            // last_seq+1): ours too, or the next present is rejected BAD_SEQ.
                            seq = 0;
                            present_in_flight = false;
                            (dirty, dirty_rows) = (true, None); // everything, into the new rows
                            raw_marker("apphost: resized");
                            // Fresh surface id: re-arm parked pulses.
                            if app.as_ref().map(|d| d.anim_active()).unwrap_or(false) {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::Blocking);
                            }
                        }
                    }
                }
            } else {
                raw_marker("apphost: FAIL resize vmo");
            }
        }
        // Coalesced present: render + present the latest model once the
        // previous present is acked. Runs in the same iteration an ack
        // clears the in-flight slot, so a tap that arrived mid-present is
        // shown without waiting for the next input. A held drag computes
        // only its newest position, here (`probe/drag.rs`).
        if let (false, Some(dsl)) = (present_in_flight, app.as_mut()) {
            dsl.drag_flush(&client, surface_id, &mut dirty, &mut dirty_rows);
        }
        if let (false, Some(dsl)) = (present_in_flight, app.as_mut()) {
            if dirty || dsl.bands_pending() || dsl.tree_dirty {
                let change = core::mem::take(&mut dirty).then_some(dirty_rows.take());
                let full = change == Some(None);
                match dsl.present_frame(&client, surface_id, vmo, &mut seq, change) {
                    Ok(sent) => present_in_flight = sent,
                    Err(m) => raw_marker(m),
                }
                if core::mem::take(&mut dsl.tap_present_owed) {
                    raw_marker("apphost: tap settled");
                }
                // Once per process: typing presents too (the privacy rule).
                if full && crate::proof_line::INTERACTIVE_PRESENT.claim() {
                    raw_marker("APPHOST: interactive frame presented");
                }
                // `.autofocus(true)` (TASK-0067B): the shown layout names the field to focus.
                dsl.apply_autofocus(&client, surface_id);
            }
        }
        // The motion — offsets, the wash — in effect at once (after any structure it names).
        if let Some(dsl) = app.as_mut() {
            if dsl.motion_dirty {
                dsl.send_motion(&client, surface_id);
            }
        }
    }
}
