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

    /// Resolves the program bytes: the granted payload VMO when present and
    /// well-formed (leaked once — the app-host process IS one app instance,
    /// so the payload lives for the process), otherwise the embedded
    /// fallback. Marked on both paths (`APPHOST: payload source=…`).
    fn resolve_payload() -> Option<&'static [u8]> {
        use nexus_abi::{bundlemgrd as wire, cap_clone, cap_close, vmo_read};
        let start = nsec().unwrap_or(0);
        // Slot presence probe: cap_clone+close (cap_query answers only for a
        // subset of kinds — the established probe pattern).
        loop {
            match cap_clone(PAYLOAD_VMO_SLOT) {
                Ok(probe) => {
                    let _ = cap_close(probe);
                    break;
                }
                Err(_) => {
                    if nsec().unwrap_or(u64::MAX).saturating_sub(start) > PAYLOAD_BUDGET_NS {
                        raw_marker("APPHOST: FAIL payload (no vmo)");
                        return None;
                    }
                    let _ = yield_();
                }
            }
        }
        // Header poll: bundlemgrd writes the header AFTER the payload bytes
        // (header-last release ordering), so a decodable header means the
        // payload is complete.
        let mut hdr = [0u8; wire::PAYLOAD_DATA_OFFSET];
        loop {
            if vmo_read(PAYLOAD_VMO_SLOT, 0, &mut hdr).is_ok() {
                if let Some((status, len)) = wire::decode_payload_header(&hdr) {
                    if status != wire::PAYLOAD_STATUS_OK
                        || len == 0
                        || len as usize > PAYLOAD_MAX_LEN
                        || len % 8 != 0
                    {
                        raw_marker("APPHOST: FAIL payload (header status)");
                        return None;
                    }
                    let mut buf = nexus_dsl_ir::read::AlignedBytes::zeroed(len as usize);
                    if vmo_read(PAYLOAD_VMO_SLOT, wire::PAYLOAD_DATA_OFFSET, buf.as_bytes_mut())
                        .is_err()
                    {
                        raw_marker("APPHOST: FAIL payload (vmo read)");
                        return None;
                    }
                    raw_marker("APPHOST: payload source=bundle");
                    return Some(alloc::boxed::Box::leak(alloc::boxed::Box::new(buf)).as_bytes());
                }
            }
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > PAYLOAD_BUDGET_NS {
                raw_marker("APPHOST: FAIL payload (header timeout)");
                return None;
            }
            let _ = yield_();
        }
    }

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
        let mut theme_mode = wait_for_theme(&events);

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

        // 3. The app's own surface VMO, sized to the content rect (ADR-0037).
        //    Mutable: a WM resize (`OP_SURFACE_RECT`) re-creates it at the new
        //    size so the CONTENT grows with the frame (not just the shadow).
        let mut vmo = vmo_create(surf_w as usize * surf_h as usize * 4)
            .map_err(|_| "apphost: vmo create failed")?;

        // 4. Mount + render the DSL program into the VMO; the solid fill stays
        //    as the fail-closed VISIBLE fallback.
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
            190
        };
        let mut app = DslApp::mount(payload, surf_w, surf_h, theme_mode, base_alpha);
        match &app {
            Some(dsl) if dsl.render(vmo) => raw_marker("APPHOST: dsl frame rendered"),
            _ => {
                app = None;
                raw_marker("APPHOST: FAIL dsl mount (probe fill fallback)");
                let row_bytes = surf_w as usize * 4;
                let mut row = alloc::vec![0u8; row_bytes];
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&FILL_BGRA);
                }
                for y in 0..surf_h as usize {
                    vmo_write(vmo, y * row_bytes, &row).map_err(|_| "apphost: vmo fill failed")?;
                }
            }
        }
        raw_marker("apphost: vmo filled");

        // 5. SURFACE_CREATE — a CLONE of the VMO cap moves with the message
        //    (the gpud-attach pattern); the original stays ours for redraws.
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
            dsl.submit_layers(&client);
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
                match events.recv_into(Wait::Blocking, &mut event_frame) {
                Ok(len) => {
                    recv_err_marked = false;
                    len
                }
                Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {
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
            // Classify the frame: present-ack (flow control) vs input vs theme vs other.
            if wire::decode_surface_ack(&event_frame[..len], wire::OP_SURFACE_PRESENT).is_some() {
                present_in_flight = false;
            } else if let Some(mode) = wire::decode_surface_theme(&event_frame[..len]) {
                // Live re-theme: re-mount with the new tokens (state is rebuilt
                // from the payload — a theme toggle is rare; per-token re-emit
                // without a remount is a later refinement) and repaint.
                if mode != theme_mode {
                    theme_mode = mode;
                    app = DslApp::mount(payload, surf_w, surf_h, theme_mode, base_alpha);
                    if let Some(dsl) = app.as_ref() {
                        let _ = dsl.render(vmo);
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
                    if let Ok(nv) = vmo_create(nw as usize * nh as usize * 4) {
                        let _ = send_retry(&client, &wire::encode_surface_destroy(surface_id));
                        let _ = nexus_abi::cap_close(vmo);
                        vmo = nv;
                        surf_w = nw;
                        surf_h = nh;
                        if let Some(dsl) = app.as_mut() {
                            dsl.resize(surf_w, surf_h);
                            let _ = dsl.render(vmo);
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
                                }
                            }
                        }
                    } else {
                        raw_marker("apphost: FAIL resize vmo");
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
                        }
                    }
                } else if kind == wire::INPUT_KIND_TAP {
                    if let Some(dsl) = app.as_mut() {
                        if dsl.tap(i32::from(x), i32::from(y)) {
                            dirty = true;
                            dirty_rows = None; // model change: full repaint
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
                let span = dirty_rows;
                let ok = match span {
                    Some((y0, y1)) => dsl.render_rows(vmo, y0, y1),
                    None => dsl.render(vmo),
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
                    dsl.submit_layers(&client);
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
        /// The `node_id` of the interactive box under the pointer (windowd
        /// MOVE events → `hover()`), washed at PAINT time. Presentation-only
        /// state: hover never re-runs layout (pretext), only a repaint.
        hovered: Option<usize>,
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
            base_alpha: u8,
        ) -> Option<Self> {
            use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

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
            let device = FixtureEnv::default();
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
            Some(Self {
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
                hovered: None,
            })
        }

        /// Runs the interpreter's hit-testing for a body tap; on visible
        /// damage re-lays-out + refreshes the text runs. Returns whether a
        /// re-render is needed.
        fn tap(&mut self, x: i32, y: i32) -> bool {
            use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale};
            let tokens = tokens_for(self.theme_mode);
            let device = FixtureEnv::default();
            let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
            let damage = self
                .view
                .pointer(
                    tokens,
                    &device,
                    &locale,
                    &mut self.host,
                    &self.layout.boxes,
                    "Tap",
                    nexus_layout_types::FxPx::new(x),
                    nexus_layout_types::FxPx::new(y),
                )
                .ok()
                .flatten();
            if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
                return false;
            }
            // Pretext discipline: ONLY layout-class damage re-runs the engine
            // (widget props — including text content — record Layout deps).
            // A paint-only change re-renders from the RETAINED boxes: the
            // pre-measured text + kept layout make that the cheap path.
            if matches!(damage, Some(Damage::Layout)) {
                let engine = nexus_layout::LayoutEngine::new();
                let Ok(layout) = engine.layout_with_viewport(
                    self.view.scene(),
                    nexus_layout_types::FxPx::new(self.w as i32),
                    Some(nexus_layout_types::FxPx::new(self.h as i32)),
                    &nexus_text_baked::measure_text::BakedTextMeasure,
                ) else {
                    return false;
                };
                self.layout = layout;
                self.texts.clear();
                collect_texts(self.view.scene(), &mut 0, &mut self.texts);
            }
            true
        }

        /// Pointer motion (`INPUT_KIND_MOVE`): re-resolve the hovered
        /// interactive box (same hit-test the Tap routing uses). Returns the
        /// union ROW SPAN of the old+new hovered boxes when the target
        /// changed (`None` = no change) — a PAINT-only change: the caller
        /// re-renders exactly that span; layout and boxes stay retained.
        fn hover(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
            let target = self.view.hover_box_id(
                &self.layout.boxes,
                "Tap",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
            );
            if target == self.hovered {
                return None;
            }
            let old = core::mem::replace(&mut self.hovered, target);
            self.hover_span(old, target)
        }

        /// Pointer left the surface (`INPUT_KIND_LEAVE`): clear the wash.
        /// Returns the cleared box's row span for the partial repaint.
        fn hover_clear(&mut self) -> Option<(i32, i32)> {
            let old = self.hovered.take();
            self.hover_span(old, None)
        }

        /// Union row span (y0, y1 exclusive; surface-clamped) of two hover
        /// anchors' boxes — the exact rows a hover change repaints.
        fn hover_span(&self, a: Option<usize>, b: Option<usize>) -> Option<(i32, i32)> {
            let mut span: Option<(i32, i32)> = None;
            for id in [a, b].into_iter().flatten() {
                if let Some(bx) = self.layout.boxes.iter().find(|bb| bb.node_id == id) {
                    let y0 = bx.rect.y.0.max(0);
                    let y1 = (bx.rect.y.0 + bx.rect.height.0).min(self.h as i32);
                    if y0 < y1 {
                        span = Some(match span {
                            Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                            None => (y0, y1),
                        });
                    }
                }
            }
            span
        }

        /// WM resize (`OP_SURFACE_RECT`): re-lay-out the current view at the new
        /// surface size — WITHOUT resetting store state (a remount would). Both
        /// width AND height take effect (the scene reflows to `w`; the render
        /// bound uses `h`). The caller re-renders into the freshly-sized VMO.
        fn resize(&mut self, w: u32, h: u32) {
            self.w = w;
            self.h = h;
            // Box geometry moves under the pointer; the next MOVE re-resolves.
            self.hovered = None;
            let engine = nexus_layout::LayoutEngine::new();
            if let Ok(layout) = engine.layout_with_viewport(
                self.view.scene(),
                nexus_layout_types::FxPx::new(w as i32),
                Some(nexus_layout_types::FxPx::new(h as i32)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            ) {
                self.layout = layout;
                self.texts.clear();
                collect_texts(self.view.scene(), &mut 0, &mut self.texts);
            }
        }

        /// R1 layer seam: submit the material-tagged glass regions of the current
        /// layout to windowd (`OP_SURFACE_LAYERS`). Each `LayoutBox` whose
        /// `.material()` is glass becomes a `LayerDesc` (surface-local rect +
        /// level + radius + shadow); windowd composites each as a real frosted
        /// `nexus-gfx` layer over the wallpaper. Re-sent whenever the layout
        /// changes (mount + re-layout). No glass nodes ⇒ empty list ⇒ windowd
        /// composites the surface with the default treatment (unchanged).
        fn submit_layers(&self, client: &KernelClient) {
            use nexus_layout_types::{GlassLevel, SurfaceMaterial};
            let clamp = |v: i32| v.max(0).min(u16::MAX as i32) as u16;
            let mut layers = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
            let mut n = 0;
            for b in &self.layout.boxes {
                if n >= wire::MAX_SURFACE_LAYERS {
                    break;
                }
                let glass_level = match b.visual.material {
                    SurfaceMaterial::Glass(GlassLevel::Panel) => wire::GLASS_PANEL,
                    SurfaceMaterial::Glass(GlassLevel::Card) => wire::GLASS_CARD,
                    SurfaceMaterial::Glass(GlassLevel::Subtle) => wire::GLASS_SUBTLE,
                    SurfaceMaterial::Glass(GlassLevel::Window) => wire::GLASS_WINDOW,
                    SurfaceMaterial::Opaque => continue,
                };
                layers[n] = wire::LayerDesc {
                    x: clamp(b.rect.x.0),
                    y: clamp(b.rect.y.0),
                    w: clamp(b.rect.width.0),
                    h: clamp(b.rect.height.0),
                    material: wire::MATERIAL_GLASS,
                    glass_level,
                    radius: b.visual.corner_radius.top_left.0.clamp(0, 255) as u8,
                    shadow_alpha: if b.visual.shadow.is_some() { 80 } else { 0 },
                };
                n += 1;
            }
            let mut buf = [0u8; wire::SURFACE_LAYERS_MAX_LEN];
            let len = wire::encode_surface_layers(&layers[..n], &mut buf);
            let _ = client.send(&buf[..len], Wait::NonBlocking);
            raw_marker(&alloc::format!("apphost: submitted {n} layers"));
        }

        /// Writes the current scene (fills + glyph runs) into the VMO. The
        /// page base is the theme's Surface token — the scene's own boxes
        /// (surfaceVariant buttons, onSurface text) are specified against it.
        /// One-time diagnostic: where the interactive (handler) boxes are.
        fn dump_handler_boxes(&self) {
            for (box_id, _) in self.view.handlers().iter().take(8) {
                if let Some(b) = self.layout.boxes.iter().find(|b| b.node_id == *box_id) {
                    raw_marker(&alloc::format!(
                        "apphost: handler box id={} x={} y={} w={} h={}",
                        box_id,
                        b.rect.x.as_i32(),
                        b.rect.y.as_i32(),
                        b.rect.width.as_i32(),
                        b.rect.height.as_i32()
                    ));
                }
            }
        }

        fn render(&self, vmo: u32) -> bool {
            self.render_rows(vmo, 0, self.h as i32)
        }

        /// Renders only rows `[y0, y1)` into the VMO — the damage-limited
        /// path (hover washes re-render two box spans, not 1280×800). The
        /// full render is `render()` = the whole surface span.
        fn render_rows(&self, vmo: u32, y0: i32, y1: i32) -> bool {
            use nexus_dsl_runtime::theme_tokens::ColorToken;
            let s = tokens_for(self.theme_mode).color(ColorToken::Surface);
            // Page base = the theme Surface token: OPAQUE for a desktop/
            // fullscreen surface (the base layer), frosted-translucent for
            // floating windows (`base_alpha`).
            let base = [s.b, s.g, s.r, self.base_alpha];
            // Paint-time hover wash (nexus-style convention): the foreground
            // at Hover wash alpha — darkens on light themes, lightens on dark.
            let hover = self.hovered.map(|node_id| {
                let fg = tokens_for(self.theme_mode).color(ColorToken::OnSurface);
                nexus_scene_raster::HoverWash {
                    node_id,
                    color: nexus_layout_types::Rgba8::new(
                        fg.r,
                        fg.g,
                        fg.b,
                        nexus_style::InteractionState::Hover.wash_alpha(),
                    ),
                }
            });
            let surf_w = self.w as usize;
            let row_bytes = surf_w * 4;
            let mut row = alloc::vec![0u8; row_bytes];
            let y_start = y0.max(0);
            let y_end = y1.min(self.h as i32);
            for y in y_start..y_end {
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&base);
                }
                // Scene fills: the ONE promoted painter (`nexus-scene-raster`,
                // golden-verified) — rounded corners, circles, vector shapes,
                // borders, src-over glass. On-device pixels match the design
                // goldens by construction (the flat rect spans this replaces
                // were the "buttons are square" report).
                {
                    let mut canvas = nexus_scene_raster::RowCanvas {
                        buf: &mut row,
                        y,
                        width: self.w as i32,
                    };
                    nexus_scene_raster::paint_row_hover(&mut canvas, &self.layout.boxes, hover);
                }
                // Glyph pass: the shared text SSOT (same blender windowd uses)
                // blends each run's slice intersecting this row.
                for b in &self.layout.boxes {
                    let (bx, by, bw, bh) =
                        (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                    if bw <= 0 || bh <= 0 || y < by || y >= by + bh {
                        continue;
                    }
                    if let Some((_, content, font, color)) =
                        self.texts.iter().find(|(id, _, _, _)| *id == b.node_id)
                    {
                        nexus_text_baked::draw_text_row(
                            &mut row,
                            y as u32,
                            by,
                            bx.max(0) as u32,
                            self.w,
                            content.chars(),
                            *font,
                            *color,
                        );
                    }
                }
                if vmo_write(vmo, y as usize * row_bytes, &row).is_err() {
                    return false;
                }
            }
            true
        }
    }

    /// `APPHOST: mounted hash=<first-16-hex>` — the R2 DoD marker.
    fn emit_mounted_hash_marker(nxir: &[u8]) {
        let hash_prefix: u64 = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir)
            .ok()
            .and_then(|r| {
                r.root().ok().map(|root| {
                    root.get_program_hash().ok().map(|h| {
                        let mut v = [0u8; 8];
                        let n = h.len().min(8);
                        v[..n].copy_from_slice(&h[..n]);
                        u64::from_be_bytes(v)
                    })
                })
            })
            .flatten()
            .unwrap_or(0);
        let mut line = [0u8; 64];
        let prefix = b"APPHOST: mounted hash=";
        line[..prefix.len()].copy_from_slice(prefix);
        let mut pos = prefix.len();
        for i in 0..16 {
            let nibble = ((hash_prefix >> (60 - i * 4)) & 0xF) as u8;
            line[pos] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
            pos += 1;
        }
        line[pos] = b'\n';
        let _ = nexus_abi::debug_write(&line[..pos + 1]);
    }

    /// `apphost: window intent style=… mode=… level=… resizable=…` — the app's
    /// declared window intent read from the payload (TASK-0080C #17 Slice 1a).
    /// This is the app-owned axis of `chrome = intent ⟂ policy`
    /// (docs/dev/ui/patterns/windowing/window-intent.md); windowd composes the
    /// frame from it under the active windowing policy (Slice 1b). Absent
    /// `Window {}` decodes to the defaults (titlebar/auto/normal).
    fn emit_window_intent_marker(nxir: &[u8]) {
        use nexus_dsl_ir::ui_ir_capnp::{WindowLevel, WindowMode, WindowStyle};
        let Ok(reader) = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir) else {
            return;
        };
        let Ok(root) = reader.root() else { return };
        let Ok(win) = root.get_window() else { return };
        let style = match win.get_style() {
            Ok(WindowStyle::Titlebar) => "titlebar",
            Ok(WindowStyle::HiddenTitlebar) => "hiddenTitlebar",
            Ok(WindowStyle::Plain) => "plain",
            Err(_) => "?",
        };
        let mode = match win.get_mode() {
            Ok(WindowMode::Auto) => "auto",
            Ok(WindowMode::Freeform) => "freeform",
            Ok(WindowMode::Fullscreen) => "fullscreen",
            Err(_) => "?",
        };
        let level = match win.get_level() {
            Ok(WindowLevel::Normal) => "normal",
            Ok(WindowLevel::Desktop) => "desktop",
            Ok(WindowLevel::Overlay) => "overlay",
            Err(_) => "?",
        };
        raw_marker(&alloc::format!(
            "apphost: window intent style={style} mode={mode} level={level} resizable={}",
            win.get_resizable()
        ));
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

    /// Bounded wait for windowd's initial theme push (`OP_SURFACE_THEME`, sent
    /// when the event channel attaches — before we mount). Defaults to dark (the
    /// compositor default) if none arrives; the app still renders, just possibly
    /// not theme-matched.
    fn wait_for_theme(events: &KernelClient) -> u8 {
        let start = nsec().unwrap_or(0);
        let mut frame = [0u8; 64];
        loop {
            if let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
                if let Some(mode) = wire::decode_surface_theme(&frame[..len]) {
                    raw_marker("APPHOST: theme received");
                    return mode;
                }
            }
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > 500_000_000 {
                return wire::THEME_DARK;
            }
            let _ = yield_();
        }
    }

    /// Reads the app's window intent from the payload as the `WIN_*` wire tags
    /// (style, level, mode). Absent `Window {}` ⇒ the ordinary defaults.
    fn read_window_intent_tags(nxir: &[u8]) -> (u8, u8, u8) {
        use nexus_dsl_ir::ui_ir_capnp::{WindowLevel, WindowMode, WindowStyle};
        let default = (wire::WIN_STYLE_TITLEBAR, wire::WIN_LEVEL_NORMAL, wire::WIN_MODE_AUTO);
        let Ok(reader) = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir) else {
            return default;
        };
        let Ok(root) = reader.root() else { return default };
        let Ok(win) = root.get_window() else { return default };
        let style = match win.get_style() {
            Ok(WindowStyle::HiddenTitlebar) => wire::WIN_STYLE_HIDDEN_TITLEBAR,
            Ok(WindowStyle::Plain) => wire::WIN_STYLE_PLAIN,
            _ => wire::WIN_STYLE_TITLEBAR,
        };
        let level = match win.get_level() {
            Ok(WindowLevel::Desktop) => wire::WIN_LEVEL_DESKTOP,
            Ok(WindowLevel::Overlay) => wire::WIN_LEVEL_OVERLAY,
            _ => wire::WIN_LEVEL_NORMAL,
        };
        let mode = match win.get_mode() {
            Ok(WindowMode::Freeform) => wire::WIN_MODE_FREEFORM,
            Ok(WindowMode::Fullscreen) => wire::WIN_MODE_FULLSCREEN,
            _ => wire::WIN_MODE_AUTO,
        };
        (style, level, mode)
    }

    /// Geometry handshake: send the window intent (`OP_SURFACE_INTENT`) and wait
    /// (bounded) for windowd's composed content rect (`OP_SURFACE_RECT`) on the
    /// event channel. `None` if windowd never answers (older WM) — the caller
    /// falls back to the probe default. The WM owns geometry; the app sizes its
    /// VMO to whatever rect it gets.
    fn request_content_rect(
        client: &KernelClient,
        events: &KernelClient,
        style: u8,
        level: u8,
        mode: u8,
        nonce: u64,
    ) -> Option<(u32, u32)> {
        // Nonce-correlated: windowd answers on OUR event channel — without it,
        // concurrent mounts stole each other's rect and every app fell back.
        let intent = wire::encode_surface_intent(style, level, mode, false, nonce);
        let mut sent = false;
        for _ in 0..SEND_RETRIES {
            if client.send(&intent, Wait::NonBlocking).is_ok() {
                sent = true;
                break;
            }
            let _ = yield_();
        }
        if !sent {
            return None;
        }
        let start = nsec().unwrap_or(0);
        let mut frame = [0u8; 64];
        loop {
            if let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
                if let Some((_, _, w, h)) = wire::decode_surface_rect(&frame[..len]) {
                    raw_marker("APPHOST: content rect received");
                    return Some((u32::from(w), u32::from(h)));
                }
            }
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > 2_000_000_000 {
                raw_marker("apphost: no content rect (fallback)");
                return None;
            }
            let _ = yield_();
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

    /// Sends with bounded retries: the fixed slots may not be populated yet
    /// (execd transfers after spawn) and windowd may still be booting.
    fn send_retry(client: &KernelClient, frame: &[u8]) -> Result<(), &'static str> {
        for _ in 0..SEND_RETRIES {
            match client.send(frame, Wait::NonBlocking) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        let _ = debug_println("apphost: FAIL send retries exhausted");
        Err("apphost: send failed")
    }

    fn send_retry_cap(
        client: &KernelClient,
        frame: &[u8],
        cap: u32,
    ) -> Result<(), &'static str> {
        for _ in 0..SEND_RETRIES {
            match client.send_with_cap_move_wait(frame, cap, Wait::NonBlocking) {
                Ok(()) => return Ok(()),
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        let _ = debug_println("apphost: FAIL create send retries exhausted");
        Err("apphost: create send failed")
    }

    /// Receives the matching ack (skips unrelated frames on the shared
    /// response channel). Budgeted by TIME — windowd's bring-up decides when
    /// the ack arrives, not our iteration speed. Returns the ack value on OK.
    fn recv_ack(
        client: &KernelClient,
        op: u8,
        pending_rect: &mut Option<(u16, u16)>,
    ) -> Result<u32, &'static str> {
        let mut frame = [0u8; 64];
        let start = nsec().unwrap_or(0);
        loop {
            match client.recv_into(Wait::NonBlocking, &mut frame) {
                Ok(len) => {
                    if let Some((status, value)) =
                        wire::decode_surface_ack(&frame[..len], op)
                    {
                        if status == wire::SURFACE_STATUS_OK {
                            return Ok(value);
                        }
                        let _ = debug_println("apphost: FAIL surface ack status");
                        return Err("apphost: ack status");
                    }
                    // A content rect interleaving with the ack (windowd pushes
                    // it INSIDE create handling, so it precedes the create-ack
                    // on this channel): stash the LATEST for the event loop.
                    // Dropping it left the surface at the probe size forever.
                    if let Some((_, _, w, h)) = wire::decode_surface_rect(&frame[..len]) {
                        *pending_rect = Some((w, h));
                        continue;
                    }
                    // Unrelated frame on the shared channel — keep waiting.
                }
                Err(_) => {
                    let _ = yield_();
                }
            }
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > ACK_BUDGET_NS {
                let _ = debug_println("apphost: FAIL ack timeout");
                return Err("apphost: ack timeout");
            }
        }
    }
}
