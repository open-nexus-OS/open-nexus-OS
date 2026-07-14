// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host — the DSL app runtime process (TASK-0080D). Spawned by
//! execd (not a boot service), it validates + mounts a compiled `.nxir`
//! program with the SAME interpreter windowd's demo mount uses, lays the
//! scene out, renders it into its OWN surface VMO and presents through
//! windowd's client-surface wire (ADR-0042: `SURFACE_CREATE` moves the VMO
//! capability, presents are strictly sequenced). R2a: payload embedded at
//! build time (bundle GET_PAYLOAD replaces the byte source, not this code);
//! scene fills only — text lands with the shared text/painter promotion
//! (RFC-0067 P5). Falls back to the R1 solid-fill probe if the mount fails
//! (fail-closed, visibly).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: wire codecs host-tested in nexus-display-proto; the probe
//! itself is proven via QEMU markers (`APPHOST: …`).
//! ADR: docs/adr/0042-cross-process-surface-transport.md

#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), &'static str> {
    probe::run()
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("app-host: host mode - the probe runs on the OS (QEMU markers)");
}

// The DSL `EffectHost` over execd-provisioned fixed slots (TASK-0080C #16).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod effect_host;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod probe {
    use nexus_abi::{cap_clone, debug_println, nsec, vmo_create, vmo_write, yield_};

    /// Probe markers must NOT fold: `nexus-service-entry` arms verdict
    /// folding for every process it bootstraps, so `debug_println` swallows
    /// non-FAIL lines in interactive boots (recall-only). The R1 proof chain
    /// goes through the raw write syscall instead.
    fn raw_marker(line: &str) {
        let mut buf = [0u8; 96];
        let bytes = line.as_bytes();
        let n = bytes.len().min(buf.len() - 1);
        buf[..n].copy_from_slice(&bytes[..n]);
        buf[n] = b'\n';
        let _ = nexus_abi::debug_write(&buf[..n + 1]);
    }
    use nexus_display_proto::client_surface as wire;
    use nexus_ipc::{Client as _, KernelClient, Wait};

    mod anim;
    mod boot;
    mod scroll;
    mod paint;
    mod interaction;
    use boot::*;

    /// Fixed child capability slots — execd transfers these AFTER spawn
    /// (`cap_transfer_to_slot`): SEND on windowd's server endpoint into 5,
    /// RECV on windowd's shared response endpoint into 6 (the inputd slot
    /// convention). The child may run before the transfer lands, so every
    /// first use retries bounded (the #123 empty-slot lesson).
    const WINDOWD_SEND_SLOT: u32 = 5;
    const WINDOWD_RECV_SLOT: u32 = 6;
    /// The app's DEDICATED event channel (ADR-0042): windowd delivers input
    /// events AND surface acks here — the shared response endpoint (slot 6)
    /// raced with inputd's ack drain, so a tap sent there could be consumed
    /// by any receiver. Slot 6 stays as the fallback for older wiring
    /// (marked).
    const EVENTS_RECV_SLOT: u32 = 8;

    // The embedded fallback payload is DELETED (separation of concerns):
    // program bytes belong to bundlemgrd (the registry) ONLY. A missing/broken
    // payload VMO is a LOUD, VISIBLE failure (probe fill + FAIL marker below),
    // never a silently different program — an embedded fallback masked exactly
    // the payload-routing bugs it should have surfaced.

    /// Fixed child slot holding the payload VMO (execd's
    /// `CHILD_PAYLOAD_SLOT`); bundlemgrd fills it and writes the 16-byte
    /// header LAST (`nexus_abi::bundlemgrd::encode_payload_header`).
    const PAYLOAD_VMO_SLOT: u32 = 7;
    /// SEND-side clone of OUR OWN event channel (execd grants it alongside the
    /// RECV side): the app-host attaches it to windowd ITSELF, tagged with a
    /// self-minted nonce that SURFACE_CREATE repeats — windowd binds
    /// channel↔surface by nonce (deterministic under concurrent connects).
    const EVENTS_SEND_CLONE_SLOT: u32 = 14;
    /// Header-poll budget: the fetch is kicked BEFORE our ELF even loads, so
    /// the header normally beats us; the budget only bounds failure.
    const PAYLOAD_BUDGET_NS: u64 = 3_000_000_000;
    /// Upper payload bound accepted from the header (matches execd's VMO
    /// budget; anything larger is a malformed header by contract).
    const PAYLOAD_MAX_LEN: usize = 256 * 1024;

    /// Probe surface: well under the transport bounds.
    const SURFACE_W: u16 = 320;
    const SURFACE_H: u16 = 240;

    /// Solid probe color (BGRA): a saturated teal nothing else in the shell
    /// paints — unmistakable in a screenshot.
    const FILL_BGRA: [u8; 4] = [0x98, 0xA1, 0x2A, 0xFF];

    /// Bounded retry budget for the cap-transfer race + windowd bring-up.
    const SEND_RETRIES: usize = 4000;
    /// Ack wait budget in nanoseconds (windowd finishes its bring-up around
    /// 1.5s boot time; the probe may start at 0.33s — a yield-count budget
    /// expired 3ms early in boot 5, so the budget is TIME, not iterations).
    const ACK_BUDGET_NS: u64 = 30_000_000_000;

    /// A per-process address salt for the nonce (ASLR-independent uniqueness
    /// helper; the time component does the heavy lifting).
    fn payload_addr() -> usize {
        (&PAYLOAD_BUDGET_NS) as *const u64 as usize
    }

    pub(super) fn run() -> Result<(), &'static str> {
        raw_marker("apphost: start");

        // 1. windowd client + the app's DEDICATED event channel come up FIRST:
        //    the geometry handshake's content-rect reply (and later acks/input)
        //    arrive on the event channel, before any surface exists.
        let client = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
            .map_err(|_| "apphost: client slots")?;
        let events = match cap_clone(EVENTS_RECV_SLOT) {
            Ok(probe) => {
                let _ = nexus_abi::cap_close(probe);
                raw_marker("APPHOST: events source=dedicated");
                KernelClient::new_with_slots(WINDOWD_SEND_SLOT, EVENTS_RECV_SLOT)
                    .map_err(|_| "apphost: event slots")?
            }
            Err(_) => {
                raw_marker("APPHOST: events source=shared (fallback)");
                KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
                    .map_err(|_| "apphost: event slots")?
            }
        };

        // 1a. Attach OUR event channel to windowd, tagged with a self-minted
        //     nonce (repeated on SURFACE_CREATE): windowd binds channel↔surface
        //     by nonce, never by arrival order — N app-hosts may connect
        //     concurrently (the greeter/shell/counter channel-crossing bug).
        let nonce: u64 = nsec().unwrap_or(0) ^ ((payload_addr() as u64) << 16) ^ 0x9E37_79B9;
        match cap_clone(EVENTS_SEND_CLONE_SLOT) {
            Ok(clone) => {
                let frame = wire::encode_surface_events(nonce);
                let hdr = nexus_abi::MsgHeader::new(
                    clone,
                    0,
                    0,
                    nexus_abi::ipc_hdr::CAP_MOVE,
                    frame.len() as u32,
                );
                let deadline = nsec().unwrap_or(0).saturating_add(2_000_000_000);
                loop {
                    match nexus_abi::ipc_send_v1(
                        WINDOWD_SEND_SLOT,
                        &hdr,
                        &frame,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    ) {
                        Ok(_) => {
                            raw_marker("APPHOST: events attached (nonce)");
                            break;
                        }
                        Err(nexus_abi::IpcError::QueueFull) => {
                            if nsec().unwrap_or(u64::MAX) >= deadline {
                                raw_marker("APPHOST: FAIL events attach (queue)");
                                break;
                            }
                            let _ = yield_();
                        }
                        Err(_) => {
                            raw_marker("APPHOST: FAIL events attach (send)");
                            break;
                        }
                    }
                }
            }
            Err(_) => raw_marker("APPHOST: FAIL events attach (no send clone)"),
        }

        // 1b. Theme: the compositor pushes the active mode (`OP_SURFACE_THEME`)
        //     when the event channel attaches — capture it BEFORE mount so the
        //     app renders with the same tokens as the desktop.
        let (mut theme_mode, mut shell_profile) = wait_for_boot_pushes(&events);

        // 2. The DSL payload + its window intent → the geometry handshake. The
        //    WM owns geometry: a desktop/full-screen surface asks windowd for
        //    its content rect (`chrome = intent ⟂ policy`); a normal app uses
        //    the probe default. Fail-soft — if windowd does not answer, default.
        // No payload = LOUD visible failure: mount(&[]) fails and the probe
        // fill renders (never a silently substituted program).
        let payload = resolve_payload().unwrap_or(&[]);
        let (style, level, mode) = read_window_intent_tags(payload);
        // Declared resize intent: floating windows are resizable; a desktop/
        // fullscreen surface is not (the presentation resolver enforces this
        // WM-side too). Carried atomically on SURFACE_CREATE.
        let resizable = level != wire::WIN_LEVEL_DESKTOP && mode != wire::WIN_MODE_FULLSCREEN;
        let (mut surf_w, mut surf_h) = if level == wire::WIN_LEVEL_DESKTOP
            || mode == wire::WIN_MODE_FULLSCREEN
        {
            request_content_rect(&client, &events, style, level, mode, nonce)
                .unwrap_or((SURFACE_W as u32, SURFACE_H as u32))
        } else {
            (SURFACE_W as u32, SURFACE_H as u32)
        };
        // Content rect arriving DURING an ack wait (windowd's corrective push
        // after a small create) — stashed by `recv_ack` instead of dropped,
        // applied by the event loop as if it had just been received.
        let mut pending_rect: Option<(u16, u16)> = None;

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
        } else if mode == wire::WIN_MODE_FULLSCREEN {
            255
        } else {
            // Frosted floating window: the page base leaves ~1/3 of the blurred
            // backdrop visible (the material look lives or dies on this — 190
            // read as a solid slab; opaque ELEMENTS still paint fully on top).
            168
        };
        let mut app =
            DslApp::mount(payload, surf_w, surf_h, theme_mode, shell_profile, base_alpha);
        // WebRender scroll band geometry — ONLY for a floating windowed app that
        // actually scrolls (desktop/fullscreen surfaces keep the plain path; the
        // desktop uses a separate windowd path that ignores the scroll band).
        let band: Option<(u32, u32, u32)> =
            if level != wire::WIN_LEVEL_DESKTOP && mode != wire::WIN_MODE_FULLSCREEN {
                app.as_ref().and_then(|d| d.band_geometry())
            } else {
                None
            };
        let (create_content_h, create_header_h, create_footer_h) =
            band.map_or((0u16, 0u16, 0u16), |(h, f, c)| {
                (c.min(u16::MAX as u32) as u16, h.min(u16::MAX as u32) as u16, f.min(u16::MAX as u32) as u16)
            });
        // VMO height: the packed band (header + footer + content) when banded,
        // else the VISIBLE surface height (create `height` field stays VISIBLE).
        let vmo_h = band.map_or(surf_h, |(h, f, c)| h + f + c);
        if let Some(dsl) = app.as_mut() {
            dsl.banded = band.is_some();
            dsl.alloc_band_h = vmo_h;
        }

        // 3. The app's own surface VMO. Sized TALL for a banded surface so the
        //    whole resident scroll content lives in it ONCE; visible-sized
        //    otherwise. Mutable: a WM resize re-creates it at the new size.
        let mut vmo = vmo_create(surf_w as usize * vmo_h as usize * 4)
            .map_err(|_| "apphost: vmo create failed")?;

        let first_render_ok = app
            .as_mut()
            .map(|dsl| if dsl.banded { dsl.render_band(vmo) } else { dsl.render(vmo) })
            .unwrap_or(false);
        match &app {
            Some(_) if first_render_ok => raw_marker("APPHOST: dsl frame rendered"),
            _ => {
                app = None;
                raw_marker("APPHOST: FAIL dsl mount (probe fill fallback)");
                let row_bytes = surf_w as usize * 4;
                let mut row = alloc::vec![0u8; row_bytes];
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&FILL_BGRA);
                }
                // app == None ⇒ band == None ⇒ vmo_h == surf_h (visible fill).
                for y in 0..vmo_h as usize {
                    vmo_write(vmo, y * row_bytes, &row).map_err(|_| "apphost: vmo fill failed")?;
                }
            }
        }
        raw_marker("apphost: vmo filled");

        // 5. SURFACE_CREATE — a CLONE of the VMO cap moves with the message
        //    (the gpud-attach pattern); the original stays ours for redraws. The
        //    create `height` field is the VISIBLE frame height; the scroll band
        //    (content_h/header_h/footer_h) rides atomically so windowd allocs the
        //    tall atlas band up front (0,0,0 = non-scrollable, unchanged).
        let clone = cap_clone(vmo).map_err(|_| "apphost: cap clone failed")?;
        let create =
            wire::encode_surface_create(
            surf_w as u16,
            surf_h as u16,
            wire::FORMAT_BGRA8888,
            style,
            level,
            mode,
            resizable,
            nonce,
            create_content_h,
            create_header_h,
            create_footer_h,
        );
        send_retry_cap(&client, &create, clone)?;
        let mut surface_id = recv_ack(&events, wire::OP_SURFACE_CREATE, &mut pending_rect)?;
        raw_marker("APPHOST: surface created");

        // 6. SURFACE_PRESENT seq=1, full damage — strictly one in flight.
        let mut damage = [wire::DamageRect { x: 0, y: 0, width: surf_w as u16, height: surf_h as u16 }];
        let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
        let len = wire::encode_surface_present(surface_id, 1, &damage, &mut buf);
        send_retry(&client, &buf[..len])?;
        let _ = recv_ack(&events, wire::OP_SURFACE_PRESENT, &mut pending_rect)?;
        raw_marker("APPHOST: probe surface presented");
        // R1 layer seam: declare the initial glass regions to windowd.
        if let Some(dsl) = app.as_ref() {
            dsl.submit_layers(&client, surface_id);
            // Mount-time `.transition` enter animations were seeded in
            // `anim_sync`: arm the frame pulse so they play from the first
            // frame (value/effect tokens are inert until a state change).
            if dsl.anim_active() {
                let req = wire::encode_surface_frame_req(surface_id);
                let _ = client.send(&req, Wait::NonBlocking);
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
        // (mount/tap/resize/theme), `Some((y0, y1))` = only that row span is
        // re-rendered + presented (hover washes). Spans from coalesced events
        // union; any full request wins.
        let mut dirty_rows: Option<(i32, i32)> = None;
        raw_marker("APPHOST: event loop armed");
        loop {
            // A rect stashed during an ack wait (`recv_ack`) is replayed here
            // as if it had just been received — same resize path, no drop.
            let len = if let Some((rw, rh)) = pending_rect.take() {
                let f = wire::encode_surface_rect(0, 0, rw, rh);
                event_frame[..f.len()].copy_from_slice(&f);
                f.len()
            } else {
                // Scroll physics pacing: while the ease/fling is animating,
                // recv with a short timeout so ticks advance even when no
                // event arrives — the timeout path repaints the viewport span
                // (apple-smooth decay instead of notch jumps).
                // Self-pace fallback ONLY for BOUNDED motion (scroll momentum,
                // a tap-triggered fade): they converge, so a dropped pulse
                // costs at most a few self-paced frames. Continuous loops
                // (widget breathe) ride the compositor frame pulse EXCLUSIVELY
                // — windowd owns pacing + visibility (a self-paced loop kept
                // rendering hidden windows at ~80Hz forever).
                let animating = app
                    .as_ref()
                    .map(|d| d.momentum_active() || d.anim_transient_active())
                    .unwrap_or(false);
                let wait = if animating {
                    Wait::Timeout(core::time::Duration::from_millis(12))
                } else {
                    Wait::Blocking
                };
                match events.recv_into(wait, &mut event_frame) {
                Ok(len) => {
                    recv_err_marked = false;
                    len
                }
                Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {
                    if let Some(dsl) = app.as_mut() {
                        let (span, end) = dsl.momentum_tick();
                        if let Some(span) = span {
                            dirty_rows = match (dirty, dirty_rows) {
                                (true, None) => None,
                                (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                                (false, None) => Some(span),
                            };
                            dirty = true;
                        }
                        if end && dsl.fire_end_reached() {
                            dirty = true;
                            dirty_rows = None;
                        }
                        // DSL animation physics also advance on the self-paced
                        // tick — same union-span damage as the frame-pulse arm.
                        if let Some(span) = dsl.anim_tick() {
                            dirty_rows = match (dirty, dirty_rows) {
                                (true, None) => None,
                                (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                                (false, None) => Some(span),
                            };
                            dirty = true;
                        }
                        if dirty && !present_in_flight {
                            // Fall through to the present block via a zero-len
                            // sentinel is not possible here — render inline.
                            let ok = match dirty_rows {
                                Some((y0, y1)) => dsl.render_rows(vmo, y0, y1),
                                None => dsl.render(vmo),
                            };
                            if ok {
                                seq = seq.wrapping_add(1);
                                let pd = match dirty_rows {
                                    Some((y0, y1)) => [wire::DamageRect {
                                        x: 0,
                                        y: y0.max(0) as u16,
                                        width: surf_w as u16,
                                        height: (y1 - y0).max(0) as u16,
                                    }],
                                    None => [wire::DamageRect {
                                        x: 0,
                                        y: 0,
                                        width: surf_w as u16,
                                        height: surf_h as u16,
                                    }],
                                };
                                let plen =
                                    wire::encode_surface_present(surface_id, seq, &pd, &mut buf);
                                if send_retry(&client, &buf[..plen]).is_ok() {
                                    present_in_flight = true;
                                }
                                dirty = false;
                                dirty_rows = None;
                            }
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
            // Classify the frame: present-ack (flow control) vs input vs theme vs other.
            if wire::decode_surface_ack(&event_frame[..len], wire::OP_SURFACE_PRESENT).is_some() {
                present_in_flight = false;
            } else if let Some(p) = wire::decode_surface_profile(&event_frame[..len]) {
                // Live shell-mode switch (Control Center Desktop/Tablet): the
                // platform override arms re-select — a deliberate re-mount,
                // same contract as a live re-theme.
                if p != shell_profile {
                    shell_profile = p;
                    // Same drop-first bracketing as the re-theme arm (see there).
                    app = None;
                    raw_marker("apphost: profile old app dropped");
                    app = DslApp::mount(
                        payload,
                        surf_w,
                        surf_h,
                        theme_mode,
                        shell_profile,
                        base_alpha,
                    );
                    if let Some(dsl) = app.as_mut() {
                        if dsl.render(vmo) {
                            seq = seq.wrapping_add(1);
                            let plen =
                                wire::encode_surface_present(surface_id, seq, &damage, &mut buf);
                            if send_retry(&client, &buf[..plen]).is_ok() {
                                present_in_flight = true;
                                raw_marker("APPHOST: profile remounted");
                            }
                            dsl.submit_layers(&client, surface_id);
                            // A remount re-seeds continuous loops/transitions:
                            // re-arm the frame pulse or they stay frozen.
                            if dsl.anim_active() {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::NonBlocking);
                            }
                        }
                    }
                }
            } else if let Some(mode) = wire::decode_surface_theme(&event_frame[..len]) {
                // Live re-theme: re-mount with the new tokens (state is rebuilt
                // from the payload — a theme toggle is rare; per-token re-emit
                // without a remount is a later refinement) and repaint.
                if mode != theme_mode {
                    theme_mode = mode;
                    // Drop the old app BEFORE mounting the new one: the drop
                    // walks every runtime collection — if a heap clobber
                    // corrupted them, the panic fires HERE (bracketed by the
                    // markers) and not ambiguously inside the new mount.
                    app = None;
                    raw_marker("apphost: re-theme old app dropped");
                    app = DslApp::mount(
                        payload,
                        surf_w,
                        surf_h,
                        theme_mode,
                        shell_profile,
                        base_alpha,
                    );
                    if let Some(dsl) = app.as_mut() {
                        let _ = dsl.render(vmo);
                        // Remount re-seeded loops/transitions: re-arm the pulse.
                        if dsl.anim_active() {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::NonBlocking);
                        }
                    }
                    dirty = true;
                    raw_marker("apphost: re-themed");
                }
            } else if let Some((_, _, rw, rh)) = wire::decode_surface_rect(&event_frame[..len]) {
                // WM resize (the compositor owns geometry): re-create the surface
                // at the new size so the CONTENT grows with the frame — not just
                // the shadow. Destroy the old (windowd is one-slot), make a new
                // VMO, re-layout (state-preserving) + render, re-create + present.
                let (nw, nh) = (u32::from(rw), u32::from(rh));
                if nw > 0 && nh > 0 && (nw, nh) != (surf_w, surf_h) {
                    surf_w = nw;
                    surf_h = nh;
                    // Re-layout at the new VISIBLE size, then recompute the scroll
                    // band (a resize re-sends CREATE, so the tall band is
                    // re-negotiated). Banded ⇒ tall VMO + render_band; else visible.
                    let band2: Option<(u32, u32, u32)> = if let Some(dsl) = app.as_mut() {
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
                        if level != wire::WIN_LEVEL_DESKTOP && mode != wire::WIN_MODE_FULLSCREEN {
                            dsl.band_geometry()
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let nvmo_h = band2.map_or(surf_h, |(h, f, c)| h + f + c);
                    let (rc_content_h, rc_header_h, rc_footer_h) =
                        band2.map_or((0u16, 0u16, 0u16), |(h, f, c)| {
                            (
                                c.min(u16::MAX as u32) as u16,
                                h.min(u16::MAX as u32) as u16,
                                f.min(u16::MAX as u32) as u16,
                            )
                        });
                    if let Ok(nv) = vmo_create(surf_w as usize * nvmo_h as usize * 4) {
                        let _ = send_retry(&client, &wire::encode_surface_destroy(surface_id));
                        let _ = nexus_abi::cap_close(vmo);
                        vmo = nv;
                        if let Some(dsl) = app.as_mut() {
                            dsl.banded = band2.is_some();
                            dsl.alloc_band_h = nvmo_h;
                            if dsl.banded {
                                let _ = dsl.render_band(vmo);
                            } else {
                                let _ = dsl.render(vmo);
                            }
                        }
                        if let Ok(clone) = cap_clone(vmo) {
                            let create = wire::encode_surface_create(
                                surf_w as u16,
                                surf_h as u16,
                                wire::FORMAT_BGRA8888,
                                style,
                                level,
                                mode,
                                resizable,
                                nonce,
                                rc_content_h,
                                rc_header_h,
                                rc_footer_h,
                            );
                            if send_retry_cap(&client, &create, clone).is_ok() {
                                if let Ok(id) = recv_ack(&events, wire::OP_SURFACE_CREATE, &mut pending_rect) {
                                    surface_id = id;
                                    damage = [wire::DamageRect {
                                        x: 0,
                                        y: 0,
                                        width: surf_w as u16,
                                        height: surf_h as u16,
                                    }];
                                    // The fresh surface's seq restarts at 0 on the
                                    // windowd side (strict last_seq+1). Reset ours
                                    // so the next present is seq=1 — otherwise it's
                                    // rejected BAD_SEQ and the resized frame never
                                    // shows.
                                    seq = 0;
                                    present_in_flight = false;
                                    dirty = true;
                                    raw_marker("apphost: resized");
                                    // Fresh surface id: any pulse parked at
                                    // windowd for the OLD id is gone — re-arm
                                    // so continuous loops survive a resize.
                                    if app.as_ref().map(|d| d.anim_active()).unwrap_or(false) {
                                        let req = wire::encode_surface_frame_req(surface_id);
                                        let _ = client.send(&req, Wait::NonBlocking);
                                    }
                                }
                            }
                        }
                    } else {
                        raw_marker("apphost: FAIL resize vmo");
                    }
                }
            } else if wire::decode_surface_frame(&event_frame[..len]).is_some() {
                // Compositor frame pulse (Choreographer): advance the scroll
                // physics AND the DSL animation physics one REAL frame, and
                // re-arm while either is still in motion.
                if let Some(dsl) = app.as_mut() {
                    let (span, end) = dsl.momentum_tick();
                    if let Some(span) = span {
                        dirty_rows = match (dirty, dirty_rows) {
                            (true, None) => None,
                            (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                            (false, None) => Some(span),
                        };
                        dirty = true;
                    }
                    if end && dsl.fire_end_reached() {
                        dirty = true;
                        dirty_rows = None;
                    }
                    // Animation tick: damage EXACTLY the animated nodes' union
                    // row span (old ∪ new transformed AABB) — the 120Hz damage
                    // contract; a full repaint per breathe tick starved the
                    // input path. Unions with any scroll span; a pending full
                    // request still wins.
                    if let Some(span) = dsl.anim_tick() {
                        dirty_rows = match (dirty, dirty_rows) {
                            (true, None) => None,
                            (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                            (false, None) => Some(span),
                        };
                        dirty = true;
                    }
                    if dsl.momentum_active() || dsl.anim_active() {
                        let req = wire::encode_surface_frame_req(surface_id);
                        let _ = client.send(&req, Wait::NonBlocking);
                    }
                }
            } else if let Some((_, kind, x, y)) = wire::decode_surface_input(&event_frame[..len]) {
                if kind == wire::INPUT_KIND_MOVE {
                    // Frame-aligned hover: paint-only, and only the union row
                    // span of the old+new hovered boxes (never a re-layout,
                    // never a full-frame repaint — the damage contract).
                    if let Some(dsl) = app.as_mut() {
                        if let Some(span) = dsl.hover(i32::from(x), i32::from(y)) {
                            dirty_rows = match (dirty, dirty_rows) {
                                (true, None) => None, // full repaint already pending
                                (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                                (false, None) => Some(span),
                            };
                            dirty = true;
                            // Hover started interaction springs (grow/shrink):
                            // arm the frame pulse so they tick.
                            if dsl.anim_active() {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::NonBlocking);
                            }
                        }
                    }
                } else if kind == wire::INPUT_KIND_LEAVE {
                    if let Some(dsl) = app.as_mut() {
                        if let Some(span) = dsl.hover_clear() {
                            dirty_rows = match (dirty, dirty_rows) {
                                (true, None) => None,
                                (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                                (false, None) => Some(span),
                            };
                            dirty = true;
                            // The un-hover spring needs pulses too.
                            if dsl.anim_active() {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::NonBlocking);
                            }
                        }
                    }
                } else if kind == wire::INPUT_KIND_WHEEL {
                    if wheel_rx_markers < 40 {
                        wheel_rx_markers += 1;
                        let d = wire::wheel_delta_from_wire(y);
                        raw_marker(&alloc::format!(
                            "APPHOST: wheel rx n={wheel_rx_markers} d={d}"
                        ));
                    }
                    // WebRender compositor-scroll: a banded surface does NOT
                    // self-scroll — windowd owns the scroll (it shifts the gpud
                    // layer `src_row`) and pushes `INPUT_KIND_SCROLL_POS`. It also
                    // won't forward WHEEL to a banded slot, so this is defensive.
                    if !app.as_ref().map(|d| d.banded).unwrap_or(false) {
                        // Wheel: an IMPULSE into the scroll physics (paint-only
                        // over the retained boxes). The momentum decays in the
                        // loop's timeout ticks; every repaint is the VIEWPORT
                        // span only, and nearing the end fires the declarative
                        // `EndReached` handler (lazy loading).
                        if let Some(dsl) = app.as_mut() {
                            let delta = wire::wheel_delta_from_wire(y);
                            let (span, end) = dsl.scroll_wheel(delta);
                            if let Some(span) = span {
                                dirty_rows = match (dirty, dirty_rows) {
                                    (true, None) => None,
                                    (_, Some((a0, a1))) => Some((a0.min(span.0), a1.max(span.1))),
                                    (false, None) => Some(span),
                                };
                                dirty = true;
                            }
                            if end && dsl.fire_end_reached() {
                                dirty = true;
                                dirty_rows = None; // model changed: full repaint
                            }
                            // Choreographer contract: while the ease/fling is
                            // live, ask the compositor for ONE frame pulse — the
                            // physics ticks on the REAL frame cadence.
                            if dsl.momentum_active() {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::NonBlocking);
                            }
                        }
                    }
                } else if kind == wire::INPUT_KIND_SCROLL_POS {
                    // Compositor owns the scroll (WebRender path): mirror the
                    // pushed ABSOLUTE offset for hit-test/EndReached WITHOUT a
                    // re-render. Only a LoadMore (content change) re-renders the
                    // tall band — the content change is the sole repaint.
                    if let Some(dsl) = app.as_mut() {
                        if dsl.scroll_pos(i32::from(y)) {
                            dirty = true;
                            dirty_rows = None; // model changed: full band repaint
                        }
                    }
                } else if kind == wire::INPUT_KIND_TAP {
                    if let Some(dsl) = app.as_mut() {
                        if dsl.tap(i32::from(x), i32::from(y)) {
                            dirty = true;
                            dirty_rows = None; // model change: full repaint
                            // A tap may have started an animation (`.animate`/
                            // `.effect` on the changed state): arm the frame
                            // pulse so the physics ticks on the real cadence.
                            if dsl.anim_active() {
                                let req = wire::encode_surface_frame_req(surface_id);
                                let _ = client.send(&req, Wait::NonBlocking);
                            }
                        } else if tap_miss_markers < 8 {
                            // No handler hit / no visible change: report the
                            // first few WITH VALUES (coordinate-mapping bugs
                            // look like this) + a one-time handler-box dump so
                            // one boot log shows where taps land vs. where the
                            // interactive boxes are.
                            tap_miss_markers += 1;
                            raw_marker(&alloc::format!(
                                "apphost: input tap miss at ({x},{y})"
                            ));
                            if tap_miss_markers == 1 {
                                if let Some(dsl) = app.as_ref() {
                                    dsl.dump_handler_boxes();
                                }
                            }
                        }
                    }
                }
            } else if odd_frame_markers < 8 {
                // Unrelated frame — bounded marker, never silent.
                odd_frame_markers += 1;
                raw_marker("apphost: event frame skipped (not input)");
            }
            // Coalesced present: render + present the latest model once the
            // previous present is acked. Runs in the same iteration an ack
            // clears the in-flight slot, so a tap that arrived mid-present is
            // shown without waiting for the next input.
            if dirty && !present_in_flight {
                let Some(dsl) = app.as_mut() else { continue };
                // A banded (compositor-scroll) surface only ever repaints on a
                // CONTENT change (LoadMore/theme/resize) — always the WHOLE tall
                // band; scroll itself never repaints (windowd shifts src_row).
                let span = if dsl.banded { None } else { dirty_rows };
                let ok = if dsl.banded {
                    dsl.render_band(vmo)
                } else {
                    match span {
                        Some((y0, y1)) => dsl.render_rows(vmo, y0, y1),
                        None => dsl.render(vmo),
                    }
                };
                if !ok {
                    raw_marker("apphost: FAIL interactive render");
                    dirty = false;
                    dirty_rows = None;
                    continue;
                }
                seq = seq.wrapping_add(1);
                let present_damage = match span {
                    // Partial (hover): present exactly the re-rendered rows so
                    // windowd blits + composites only that band.
                    Some((y0, y1)) => [wire::DamageRect {
                        x: 0,
                        y: y0.max(0) as u16,
                        width: damage[0].width,
                        height: (y1 - y0).max(0) as u16,
                    }],
                    // Full present (banded band re-render included): the VISIBLE
                    // window damage — windowd blits the whole tall band on dirty.
                    None => damage,
                };
                let plen = wire::encode_surface_present(surface_id, seq, &present_damage, &mut buf);
                if send_retry(&client, &buf[..plen]).is_err() {
                    raw_marker("apphost: FAIL interactive present");
                    continue;
                }
                present_in_flight = true;
                dirty = false;
                dirty_rows = None;
                if span.is_none() {
                    raw_marker("APPHOST: interactive frame presented");
                    // Re-declare glass regions: a re-layout may have moved/
                    // resized them. Paint-only spans keep the layout — skip.
                    dsl.submit_layers(&client, surface_id);
                }
            }
        }
    }

    /// The mounted DSL app: interpreter view + current layout + text runs.
    /// Owned state so the event loop can re-layout/re-render after taps.
    struct DslApp {
        view: nexus_dsl_runtime::View<'static>,
        symbols: alloc::vec::Vec<alloc::string::String>,
        keys: alloc::vec::Vec<u32>,
        layout: nexus_layout::LayoutResult,
        texts: alloc::vec::Vec<(usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4])>,
        /// The service seam: `svc.*` effects (tap handlers AND the root
        /// initial-load effects) call through this over the provisioned slots.
        host: crate::effect_host::AppEffectHost,
        /// Base (page background) alpha: OPAQUE for a desktop/fullscreen
        /// surface (it IS the base layer — the shell/greeter owns every
        /// pixel; a translucent base let the wallpaper — or its solid-blue
        /// fallback — bleed through), frosted-translucent for floating
        /// windows (the glass material over the blurred backdrop).
        base_alpha: u8,
        /// Surface dimensions (the WM-composed content rect, or the probe
        /// default). Layout width + render bounds derive from these — a
        /// full-screen shell lays out at the display size, a windowed app at its
        /// own size.
        w: u32,
        h: u32,
        /// Active theme mode (`THEME_*`, pushed by windowd). Selects the token
        /// set for every render so the app matches the compositor.
        theme_mode: u8,
        /// Active shell profile (`PROFILE_*`, pushed by windowd). The device
        /// env every mount/interaction passes to the runtime.
        shell_profile: u8,
        /// The `node_id` of the interactive box under the pointer (windowd
        /// MOVE events → `hover()`), washed at PAINT time. Presentation-only
        /// state: hover never re-runs layout (pretext), only a repaint.
        hovered: Option<usize>,
        /// Reused render row buffer (width×4). The bump allocator NEVER
        /// frees: a per-render `vec!` leaked ~5KB per hover repaint until the
        /// heap page-faulted (the "nothing clickable after mousing around"
        /// crash). One allocation at mount, resized on WM resize.
        row_scratch: alloc::vec::Vec<u8>,
        /// Scroll offsets of the page's `.scroll(...)` viewport (paint-time
        /// state like `hovered`: scrolling NEVER re-runs layout and NEVER
        /// allocates — the retained boxes are repainted shifted).
        scroll_x: i32,
        scroll_y: i32,
        /// The vertical scroll PHYSICS (SSOT `animation::ScrollMomentum`):
        /// wheel notches extend a target the position eases toward; the loop
        /// ticks it while `is_animating` — apple-smooth, never a hard jump.
        momentum: animation::ScrollMomentum,
        /// Last physics tick (ns) for dt integration.
        momentum_last_ns: u64,
        /// The DSL animation subsystem (`.animate`/`.transition`/`.effect`):
        /// the `AnimationDriver` physics + per-node paint transforms, ticked on
        /// the compositor frame pulse. Host owns the clock (the DSL stays pure);
        /// see `probe/anim.rs`.
        anim: anim::AnimState,
        /// EndReached latch: fired once per approach to the content end;
        /// re-armed whenever layout re-runs (content grew/shrank).
        end_fired: bool,
        /// Reused per-picked-box animation index (parallel to `vis_pick`;
        /// -1 = none) — resolved ONCE per repaint so the painter never scans
        /// the anims slice per box per row (the hover slowdown).
        vis_anim: alloc::vec::Vec<i16>,
        /// Reused visibility index (box indices intersecting the repaint
        /// span) — per-row paint cost follows what is ON SCREEN, not the
        /// page's total box count (the 1000-message transcript contract).
        vis_pick: alloc::vec::Vec<u32>,
        /// Reused (box index, texts index) pairs for the span's text runs.
        vis_text: alloc::vec::Vec<(u32, u32)>,
        /// WebRender compositor-scroll: this surface renders a TALL packed band
        /// (fixed header + fixed footer + the whole resident scroll content) ONCE
        /// and windowd/gpud shift the source row per scroll. When set, wheel is
        /// owned by the compositor: the app never self-scrolls/re-renders on a
        /// notch — it only mirrors the pushed `INPUT_KIND_SCROLL_POS` and
        /// re-renders the band on a content change (LoadMore). `false` = the
        /// legacy paint-time-`dy` scroll (unchanged).
        banded: bool,
        /// The tall VMO/band height (rows) allocated at create — render_band
        /// clamps to it so a LoadMore that grows content never overflows the VMO
        /// (the compositor band is the same fixed size; `tail(…)` keeps it finite).
        alloc_band_h: u32,
        /// Reused pick buffer for render_band's unclipped (fixed header/footer)
        /// region — recycled like `vis_pick`, never allocated per render.
        band_pick: alloc::vec::Vec<u32>,
    }

    /// Monotonic now (ns) for physics dt; 0 on ABI failure (tick clamps dt).
    fn nsec_now() -> u64 {
        #[cfg(nexus_env = "os")]
        {
            nexus_abi::nsec().unwrap_or(0)
        }
        #[cfg(not(nexus_env = "os"))]
        {
            0
        }
    }

    impl DslApp {
        /// Validates + mounts the program bytes and lays them out at
        /// surface size. `None` on any failure (fail-closed; caller shows
        /// the probe fill).
        fn mount(
            nxir: &'static [u8],
            w: u32,
            h: u32,
            theme_mode: u8,
            shell_profile: u8,
            base_alpha: u8,
        ) -> Option<Self> {
            use nexus_dsl_runtime::{IdentityLocale, View};

            let runtime = nexus_dsl_runtime::Runtime::mount(nxir).ok()?;
            let symbols = runtime.symbols().to_vec();
            emit_mounted_hash_marker(nxir);
            emit_window_intent_marker(nxir);
            let keys: alloc::vec::Vec<u32> =
                match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir)
                    .and_then(|r| {
                        r.root().map(|root| {
                            root.get_i18n_keys()
                                .map(|l| l.iter().map(|k| k.get_key()).collect())
                        })
                    }) {
                    Ok(Ok(keys)) => keys,
                    _ => alloc::vec::Vec::new(),
                };
            let tokens = tokens_for(theme_mode);
            // The pushed shell profile IS the device env: `device.profile`
            // selects the platform override arms (tablet base / desktop).
            let device = device_for(shell_profile, w);
            let mut view = {
                let locale = IdentityLocale { symbols: &symbols, keys: &keys };
                View::mount(nxir, tokens, &device, &locale).ok()?
            };
            // Declarative initial load (principles.md §5): an `@effect` event
            // dispatched by NOTHING is a ROOT — it runs once at mount. Fire the
            // roots BEFORE the first layout so the frame reflects service-loaded
            // data (e.g. the shell's `bundlemgr.enumerate` app grid). No
            // lifecycle hook; the runtime derives roots from the dataflow.
            let mut host = crate::effect_host::AppEffectHost::new(&symbols);
            {
                let locale = IdentityLocale { symbols: &symbols, keys: &keys };
                match view.run_initial_effects(tokens, &device, &locale, &mut host) {
                    Ok(_) => raw_marker("APPHOST: initial effects ran"),
                    Err(_) => raw_marker("apphost: FAIL initial effects"),
                }
            }
            let engine = nexus_layout::LayoutEngine::new();
            let layout = engine
                .layout_with_viewport(
                    view.scene(),
                    nexus_layout_types::FxPx::new(w as i32),
                    // Bounded viewport: the surface height — Spacer/flex_grow
                    // children distribute it, so DSL centering works (an
                    // unbounded root hugged everything to the top-left).
                    Some(nexus_layout_types::FxPx::new(h as i32)),
                    &nexus_text_baked::measure_text::BakedTextMeasure,
                )
                .ok()?;
            let mut texts = alloc::vec::Vec::new();
            collect_texts(view.scene(), &mut 0, &mut texts);
            let mut app = Self {
                view,
                symbols,
                keys,
                layout,
                texts,
                host,
                base_alpha,
                w,
                h,
                theme_mode,
                shell_profile,
                hovered: None,
                row_scratch: alloc::vec![0u8; w as usize * 4],
                scroll_x: 0,
                scroll_y: 0,
                momentum: animation::ScrollMomentum::new(animation::ScrollConfig::default()),
                momentum_last_ns: 0,
                anim: anim::AnimState::new(),
                end_fired: false,
                vis_pick: alloc::vec::Vec::new(),
                vis_anim: alloc::vec::Vec::new(),
                vis_text: alloc::vec::Vec::new(),
                banded: false,
                alloc_band_h: 0,
                band_pick: alloc::vec::Vec::new(),
            };
            // Seed the animation state from the mounted scene: resting
            // transforms for value-tracked nodes, enter transitions for
            // `.transition` nodes (the first present's frame pulse plays them).
            app.anim_sync();
            Some(app)
        }

    }

    // Static theme token sets (ZSTs) → a runtime-selectable `&'static dyn Tokens`.
    static BASE_TOKENS: nexus_dsl_runtime::theme_tokens::BaseTokens =
        nexus_dsl_runtime::theme_tokens::BaseTokens;
    static DARK_TOKENS: nexus_dsl_runtime::theme_tokens::DarkTokens =
        nexus_dsl_runtime::theme_tokens::DarkTokens;
    static LIGHT_TOKENS: nexus_dsl_runtime::theme_tokens::LightTokens =
        nexus_dsl_runtime::theme_tokens::LightTokens;

    /// The token set for a wire theme mode — so the app renders with the SAME
    /// tokens the compositor pushed (dark desktop ⇒ dark app).
    fn tokens_for(mode: u8) -> &'static dyn nexus_dsl_runtime::theme_tokens::Tokens {
        match mode {
            wire::THEME_DARK => &DARK_TOKENS,
            wire::THEME_LIGHT => &LIGHT_TOKENS,
            _ => &BASE_TOKENS,
        }
    }

    /// The width class of the TOUCH axis (design_handoff_launcher: mode ⟂
    /// width — `desktopMode` is an explicit toggle, width only picks between
    /// the touch layouts). Mobile-first breakpoints, `device.sizeClass`:
    /// compact = phone (<640), regular = tablet portrait (<1024), wide =
    /// landscape (≥1024).
    fn size_class_for(w: u32) -> &'static str {
        if w < 640 {
            "compact"
        } else if w < 1024 {
            "regular"
        } else {
            "wide"
        }
    }

    /// The device environment for a pushed shell profile — what the DSL's
    /// `device.profile` reads, so `ui/platform/<profile>/` override arms
    /// select to the environment's windowing policy. Touch profiles derive
    /// `device.sizeClass` from the REAL surface width (the handoff's `vw`
    /// axis); desktop mode ignores width (one taskbar layout).
    fn device_for(profile: u8, w: u32) -> nexus_dsl_runtime::FixtureEnv {
        use nexus_dsl_runtime::FixtureEnv;
        match profile {
            wire::PROFILE_DESKTOP => FixtureEnv::desktop(),
            profile => {
                let mut env = if profile == wire::PROFILE_PHONE {
                    FixtureEnv::phone("portrait")
                } else {
                    // Our display is landscape 1280×800 (touch-landscape).
                    FixtureEnv::tablet("landscape")
                };
                env.size_class = size_class_for(w);
                env
            }
        }
    }

    /// Pre-order text collection (index parallels `LayoutBox::node_id` − 1;
    /// the same three-consumer numbering as windowd's demo mount — do not
    /// reorder emission).
    fn collect_texts(
        node: &nexus_layout_types::LayoutNode,
        index: &mut usize,
        out: &mut alloc::vec::Vec<(usize, alloc::string::String, nexus_text_baked::FontSize, [u8; 4])>,
    ) {
        use nexus_layout_types::LayoutNode as N;
        *index += 1;
        match node {
            N::Text(text, _) => {
                let font = if text.style.font_size.0 >= 15 {
                    nexus_text_baked::FontSize::Body
                } else {
                    nexus_text_baked::FontSize::Small
                };
                let c = text.style.color;
                out.push((
                    *index,
                    alloc::string::String::from(text.content.as_str()),
                    font,
                    [c.b, c.g, c.r, c.a],
                ));
            }
            N::Stack(_, _, children) | N::Grid(_, _, children) => {
                for child in children {
                    collect_texts(child, index, out);
                }
            }
            _ => {}
        }
    }

}
