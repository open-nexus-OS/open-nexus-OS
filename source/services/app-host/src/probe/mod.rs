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

use nexus_abi::{cap_clone, debug_println, nsec, yield_};

/// Probe markers must NOT fold: `nexus-service-entry` arms verdict
/// folding for every process it bootstraps, so `debug_println` swallows
/// non-FAIL lines in interactive boots (recall-only). The R1 proof chain
/// goes through the raw write syscall instead.
mod marker;
pub(crate) use marker::raw_marker;
use nexus_display_proto::client_surface as wire;
use nexus_display_proto::surface_text;

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
mod motion_layers;
mod mount;
mod overlay;
mod paint;
mod persist;
mod presentation;
mod pressure;
mod pulse;
mod reconnect;
mod scroll;
mod state;
mod store;
mod surface_mem;
mod windows; // RFC-0086 feed intake // RFC-0095: hover, leave and the drag gesture
use boot::*;
pub(crate) use env::{device_for, size_class_for, tokens_for};
pub(crate) use interaction::TapOutcome;
pub(crate) use locale::app_locale;
use paint::collect_texts;
use state::DslApp;

// The child's fixed slots and payload budgets (`probe/slots.rs`, re-sourced from the topology).
mod slots;
use slots::*;

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
    // The moved cap is this app's EVENT channel — data, not a reply inbox (`reconnect::attach`).
    match cap_clone(EVENTS_SEND_CLONE_SLOT) {
        Ok(clone) if reconnect::attach(clone, nonce) => {
            raw_marker("APPHOST: events attached (nonce)");
            // RFC-0079: relinquish our OWN send cap to our event inbox — windowd is the sole
            // sender now; a leftover SEND keeps the last-sender scan non-zero and the EOF of
            // the compositor's death never comes.
            let _ = nexus_abi::cap_close(EVENTS_SEND_CLONE_SLOT);
        }
        Ok(clone) => {
            let _ = nexus_abi::cap_close(clone);
            raw_marker("APPHOST: FAIL events attach (send)");
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
    let (style, level, mode, size) = read_window_intent_tags(locale::payload_nxir(payload));
    // Declared resize intent: floating windows are resizable; a desktop/
    // fullscreen surface is not (the presentation resolver enforces this
    // WM-side too). Carried atomically on SURFACE_CREATE.
    let resizable = level != wire::WIN_LEVEL_DESKTOP
        && level != wire::WIN_LEVEL_OVERLAY
        && mode != wire::WIN_MODE_FULLSCREEN;
    let mut win = WindowIntent { style, level, mode, nonce, size };
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
    let base_alpha = boot::base_alpha(level, mode); // the desktop's is the wallpaper's
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
    // FAIL-CLOSED: no program, no window — the failure stays LOUD in the log (marker below +
    // the payload-path FAIL markers above); no surface is claimed, and the process exits for
    // execd to reap.
    let Some(dsl) = app.as_mut() else {
        raw_marker("APPHOST: FAIL dsl mount (no window, fail-closed)");
        return Err("apphost: dsl mount failed");
    };
    // 3. The surface: its base rows and its STORE (RFC-0109) — the wash patch and every scroll
    //    layer's band, planned from the page's layers. windowd has gpud make its buffer for this
    //    app (RFC-0114) and moves it here with the answer: drawn in place, read by the GPU where
    //    it lies. A WM resize (or a store that outgrew it) re-creates it.
    let plan = dsl.plan_store();
    let (mut surface_id, mut mem) = boot::create_surface(
        (&client, &events),
        ((surf_w, surf_h), &win, resizable),
        dsl,
        plan,
        (&mut pending_rect, &mut boot_region),
    )?;
    dsl.set_surface_id(surface_id);
    dsl.taps_are_keystrokes = level == wire::WIN_LEVEL_OVERLAY;
    raw_marker("APPHOST: surface created");
    boot_env::mark_stage(2, start_ns);
    boot_env::mark_stage(3, start_ns);
    // The first frame, into slot 0: the base, the patch, every band's visible window.
    let mut first = band::PresentDamage::default();
    mem.begin_frame(true);
    if !dsl.redraw(&mut mem, None, &mut first) {
        raw_marker("APPHOST: FAIL dsl frame (render)");
        return Err("apphost: first render failed");
    }
    raw_marker("APPHOST: dsl frame rendered");
    boot_env::mark_stage(1, start_ns);

    // 6. The structure BEFORE the first present (windowd clears it at create) — the first
    //    composite is the whole window with its layers — then SURFACE_PRESENT seq=1 of slot 0
    //    naming every row drawn, strictly one in flight, and the motion (offsets, no wash yet).
    let slot = mem.end_frame(&first);
    dsl.send_tree(&client, surface_id);
    dsl.send_present(&client, surface_id, (1, slot), &first)?;
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
    // Every frame windowd sends fits (RFC-0113: 64 bytes cut long text and candidate pages).
    let mut event_frame = [0u8; nexus_display_proto::surface_text::CLIENT_EVENT_FRAME_MAX];
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
    // RFC-0112: the compositor died — the loop rejoins the next one (`reconnect`); until the
    // surface is made again, nothing of this window is sent.
    let (mut rejoin_due, mut surface_lost) = (false, false);
    let mut replay: Option<reconnect::Replay> = None;
    raw_marker("APPHOST: event loop armed");
    // The app is up: fold the bring-up markers into their verdict line.
    nexus_abi::service_verdict_flush("app-host");
    loop {
        if rejoin_due || reconnect::back_kept() {
            rejoin_due = false;
            let Some(back) = reconnect::rejoin(&client, &events, &mut win) else {
                raw_marker("APPHOST: FAIL rejoin (event channel gone) - exiting");
                return Err("apphost: event channel gone");
            };
            // The rect first — it re-creates the surface — then the attach's snapshot.
            pending_rect = Some(back.rect);
            replay = back.snapshot;
            (surface_lost, present_in_flight, last_settings_gen) = (true, false, None);
            raw_marker("APPHOST: compositor back (reattached)");
            continue;
        }
        // A rect stashed during an ack wait (`recv_ack`) is replayed here
        // as if it had just been received — same resize path, no drop.
        let len = if let Some((inset, rw, rh)) = pending_rect.take() {
            // Replay the stashed rect VERBATIM — dropping `inset` here would
            // lose the safe area on exactly the create-time corrective push
            // that this stash exists to rescue.
            let f = wire::encode_surface_rect(0, inset, rw, rh);
            event_frame[..f.len()].copy_from_slice(&f);
            f.len()
        } else if let Some(frame) = replay.take() {
            frame.copy_into(&mut event_frame)
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
            match events.recv_into_with_cap(wait, &mut event_frame, true) {
                Ok((len, cap)) => {
                    recv_err_marked = false;
                    let len = len.min(event_frame.len());
                    // Only a create's answer moves a capability (`boot::create_surface` waits
                    // for it) — and a BACK, kept for the rejoin; any other is closed.
                    if reconnect::take_cap(&event_frame[..len], cap) {
                        continue;
                    }
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
                            dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
                        }
                        // A grown store waits for the re-create (a pulse wakes the loop's tail).
                        if surface_lost {
                            // Nothing to show it on until the rejoin made the surface again.
                        } else if dsl.store_replan {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::Blocking);
                        } else if dirty && !present_in_flight {
                            match dsl.present_frame(
                                &client,
                                surface_id,
                                &mut mem,
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
                    // RFC-0112: the channel ran dry with no CLOSED before it — the compositor
                    // died (a window closed on purpose says so first, below). The durable fields
                    // go to the app's own record (TASK-0333 6e-1: statefsd answers, not the
                    // compositor), then the loop waits for the next compositor.
                    persist::save_app(app.as_ref(), persist::Reason::Compositor);
                    raw_marker("apphost: compositor gone (waiting)");
                    rejoin_due = true;
                    continue;
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
        // RFC-0112: the compositor closed the window on purpose. The app's lifetime IS its
        // window: the durable fields go to its own record, then the process ends for execd to
        // reap (#29) — spinning on the closed channel would burn the core instead.
        if nexus_display_proto::surface_restart::is_surface_closed(&event_frame[..len]) {
            persist::save_app(app.as_ref(), persist::Reason::Close);
            raw_marker("APPHOST: window closed - exiting");
            return Ok(());
        }
        // Shared surface re-create trigger (WM resize / band change / a rejoin).
        let mut recreate_surface = false;
        // Classify the frame: present-ack (flow control) vs input vs theme vs other.
        if let Some((status, acked)) =
            wire::decode_surface_ack(&event_frame[..len], wire::OP_SURFACE_PRESENT)
        {
            // The ack of the present in flight is the release of the other slot (RFC-0114 §5):
            // the next frame may be drawn there. Any other seq names no present of this surface.
            if acked == seq {
                present_in_flight = false;
            }
            marker::present_nack(status, acked, &mut odd_frame_markers);
        } else if let Some((_, inset, rw, rh)) = wire::decode_surface_rect(&event_frame[..len]) {
            // WM resize (the compositor owns geometry): re-layout at the
            // new size, then run the SHARED surface re-create below — after a rejoin at any size.
            recreate_surface = surface_lost;
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
            // The compositor's frame pulse (RFC-0110): the scroll and the DSL animations advance
            // to the time the next frame is shown (`probe/pulse.rs`).
            if let Some(dsl) = app.as_mut() {
                let at = scroll::frame_time(&event_frame[..len]);
                dsl.on_frame_pulse(at, &client, surface_id, &mut dirty, &mut dirty_rows);
            }
        } else if let Some((_, kind, x, y, facts, motion)) =
            drag::decode_pointer(&event_frame[..len])
        {
            let rx_ns = nsec_now();
            if matches!(
                kind,
                wire::INPUT_KIND_MOVE
                    | wire::INPUT_KIND_LEAVE
                    | wire::INPUT_KIND_DRAG
                    | wire::INPUT_KIND_RELEASE
            ) {
                // Hover, leave and the drag gesture (`probe/drag.rs`).
                if let Some(dsl) = app.as_mut() {
                    if let Some(motion) = &motion {
                        dsl.note_motion(kind, motion);
                    }
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
                    // TASK-0346 P2: the caret where the press landed on the focused field.
                    if dsl.press_text(i32::from(x), i32::from(y), facts) {
                        dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
                    }
                    // The press may have started a dip or a layer fade, the tap an animation or
                    // a pager glide: they tick on the frame pulse, whatever the tap repainted.
                    if dsl.anim_active() || dsl.momentum_active() || dsl.layers_live() {
                        let req = wire::encode_surface_frame_req(surface_id);
                        let _ = client.send(&req, Wait::Blocking);
                    }
                    // ONLY a repaint is dirty. A tap a handler absorbed
                    // without one (`PanelNoop`, or a control whose effect
                    // lands in another service) is NOT a miss; calling it
                    // one sent readers hunting a hit-test bug for days.
                    if outcome == TapOutcome::Repainted {
                        // Damage-bounded structural repaint (`layout_diff`):
                        // full-frame rasters per click are what made menus
                        // feel like they never opened on the TCG boot.
                        dsl.merge_tap_damage(&mut dirty, &mut dirty_rows);
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
            if let Some(dsl) = app.as_mut() {
                dsl.note_input_rx(rx_ns, dirty);
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
        } else if surface_text::decode_surface_text(&event_frame[..len]).is_some() {
            if let Some(dsl) = app.as_mut() {
                dsl.text_frame(&event_frame[..len], &mut dirty, &mut dirty_rows);
            }
        } else if surface_text::decode_ime_state(&event_frame[..len]).is_some() {
            // Composition-strip push (RFC-0075 Phase 3, ime-ui only —
            // apps without the event ignore it).
            if app.as_mut().is_some_and(|d| d.apply_ime_state(&event_frame[..len])) {
                dirty = true;
                dirty_rows = None;
            }
        } else if persist::absorb_seen(&event_frame[..len], app.as_ref()) {
            // TASK-0333 6e-1 (ADR-0086): the compositor's sighting of THIS window — hidden wrote
            // the `@persist` fields to the app's own record (persist.rs). Nothing to repaint.
        } else if let Some(handled) = pressure::absorb(&event_frame[..len], app.as_mut()) {
            // TASK-0333 6e-3 (RFC-0100 §3): the memory authority's level, raised into the program
            // as the host event `MemoryPressure` (pressure.rs); a handled one changed the view.
            if handled {
                dirty = true;
                dirty_rows = None;
            }
        } else {
            marker::odd_frame(&event_frame[..len], &mut odd_frame_markers);
        }
        // A layer set whose bands outgrew the surface's rows (planned at the relayout)
        // re-creates the surface (same flow as a WM resize, same size).
        let mut store_only = false;
        if app.as_mut().is_some_and(|dsl| core::mem::take(&mut dsl.store_replan)) {
            (store_only, recreate_surface) = (!recreate_surface, true);
        }
        // SHARED surface re-create (WM resize / a store that outgrew it): plan the store, destroy
        // + create — a new buffer from gpud (RFC-0114), the old one unmapped once the new one is
        // here (windowd shows the old surface's last frame until the new one presents) — then
        // the structure and a full present.
        if recreate_surface {
            if let Some(dsl) = app.as_mut() {
                let plan = dsl.plan_for_create();
                // A lost surface died with its compositor: nothing to destroy.
                if !surface_lost {
                    let _ = send_wait(&client, &wire::encode_surface_destroy(surface_id));
                }
                let mut late_region: Option<boot::RegionPush> = None;
                let made = boot::create_surface(
                    (&client, &events),
                    ((surf_w, surf_h), &win, resizable),
                    dsl,
                    plan,
                    (&mut pending_rect, &mut late_region),
                );
                if let Some(r) = late_region.take() {
                    let _ = dsl.apply_region(r.hour_fmt, &r.locale, &r.tz, &r.keymap);
                    last_region = Some(r);
                }
                match made {
                    Ok((id, mut new_mem)) => {
                        // Only the store grew, at the same size: what was drawn comes over.
                        let carried =
                            (store_only && !surface_lost).then(|| new_mem.carry_from(&mem));
                        surface_id = id;
                        mem = new_mem;
                        dsl.set_surface_id(surface_id);
                        // The new surface declares its layers anew (windowd cleared them).
                        dsl.tree_dirty = true;
                        dsl.motion_dirty = true;
                        // The fresh surface's seq restarts at 0 on the windowd side (strict
                        // last_seq+1): ours too, or the next present is rejected BAD_SEQ.
                        seq = 0;
                        present_in_flight = false;
                        match carried.flatten() {
                            Some(rows) => dsl.carried = Some(rows), // the pending change stays
                            None => (dirty, dirty_rows) = (true, None), // everything, anew
                        }
                        if core::mem::take(&mut surface_lost) {
                            // The new compositor routes no key here until it hears the focus.
                            dsl.reannounce_text_focus(&client, surface_id);
                            raw_marker("apphost: surface re-created");
                        } else {
                            raw_marker("apphost: resized");
                        }
                        // Fresh surface id: re-arm parked pulses.
                        if dsl.anim_active() {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::Blocking);
                        }
                    }
                    // The compositor went (again): the rejoin waits for the next one.
                    Err(boot::CreateFailed::Gone) => surface_lost = true,
                    Err(boot::CreateFailed::Closed) => {
                        persist::save_app(Some(dsl), persist::Reason::Close);
                        raw_marker("APPHOST: window closed - exiting");
                        return Ok(());
                    }
                    Err(boot::CreateFailed::Refused(m)) if surface_lost => {
                        // No window any more — the launch's fail-closed rule.
                        persist::save_app(Some(dsl), persist::Reason::Close);
                        raw_marker("APPHOST: FAIL surface after rejoin - exiting");
                        return Err(m);
                    }
                    Err(boot::CreateFailed::Refused(m)) => raw_marker(m),
                }
            }
        }
        // Coalesced present: render + present the latest model once the
        // previous present is acked. Runs in the same iteration an ack
        // clears the in-flight slot, so a tap that arrived mid-present is
        // shown without waiting for the next input. A held drag computes
        // only its newest position, here (`probe/drag.rs`).
        if let (false, Some(dsl)) = (present_in_flight || surface_lost, app.as_mut()) {
            dsl.drag_flush(&client, surface_id, &mut dirty, &mut dirty_rows);
        }
        if let (false, Some(dsl)) = (present_in_flight || surface_lost, app.as_mut()) {
            if dirty || dsl.bands_pending() || dsl.tree_dirty {
                let change = core::mem::take(&mut dirty).then_some(dirty_rows.take());
                let full = change == Some(None);
                match dsl.present_frame(&client, surface_id, &mut mem, &mut seq, change) {
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
        if let (false, Some(dsl)) = (surface_lost, app.as_mut()) {
            if dsl.motion_dirty {
                dsl.send_motion(&client, surface_id);
            }
        }
    }
}
