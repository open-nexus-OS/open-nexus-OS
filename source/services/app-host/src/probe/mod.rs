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
fn raw_marker(line: &str) {
    let mut buf = [0u8; 96];
    let bytes = line.as_bytes();
    let n = bytes.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf[n] = b'\n';
    let _ = nexus_abi::debug_write(&buf[..n + 1]);
}
use nexus_display_proto::client_surface as wire;

/// Max packed WebRender band height (header+footer+content, surface rows)
/// an app surface may keep RESIDENT in the shared gpud atlas (4000 rows
/// minus the desktop base's 800 and headroom for a second window).
const MAX_BAND_ROWS: u32 = 2000;
use nexus_ipc::{Client as _, KernelClient, Wait};

mod anim;
mod boot;
mod clock;
mod env;
mod interaction;
mod locale;
mod mount;
mod paint;
mod presentation;
mod scroll;
mod state;
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
/// the header normally beats us; the budget only bounds failure. 3s→8s
/// (RFC-0075 Phase 8d): the CJK-atlas-grown image lengthened early-boot
/// loads enough that the grant lagged 3s on busy boots (same class as
/// the content-rect budget below).
const PAYLOAD_BUDGET_NS: u64 = 8_000_000_000;
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

/// RFC-0080: slot execd grants the shared atlas VMO into (=execd `CHILD_ATLAS_VMO_SLOT`; clear of sdk-routes child_slots 11..=18).
const ATLAS_VMO_SLOT: u32 = 19;

/// Maps the shared atlas VMO READ-only and installs it as the text atlas
/// base, so this app-host renders from ONE shared copy instead of its own
/// embedded 4.25 MB (the blob is not in this image). Best-effort: any
/// failure falls back to blank text (never a crash) with a loud marker.
#[allow(unsafe_code)]
fn map_atlas_base() {
    use nexus_abi::page_flags;
    let len = nexus_text_baked::atlas_len();
    // RFC-0085: one whole-range map at a KERNEL-CHOSEN va (was a ~1100-call
    // per-page loop at fixed 0x3000_0000 — the slot-15 scar class). The RO
    // alias force-maps read-only kernel-side regardless of flags.
    let mapped_len = len.div_ceil(4096) * 4096;
    let flags = page_flags::VALID | page_flags::USER | page_flags::READ;
    let atlas_va = match nexus_abi::vm_map(ATLAS_VMO_SLOT, 0, mapped_len, flags) {
        Ok(va) => va,
        Err(_) => {
            raw_marker("APPHOST: FAIL atlas map");
            return;
        }
    };
    // SAFETY: the range [atlas_va, atlas_va + len) is now mapped read-only
    // from the shared VMO and stays valid for the process lifetime; `len`
    // is the baked atlas size.
    unsafe {
        nexus_text_baked::set_atlas_base(atlas_va as *const u8, len);
    }
    raw_marker("APPHOST: atlas mapped");
}

pub(super) fn run() -> Result<(), &'static str> {
    raw_marker("apphost: start");
    // RFC-0080: install the shared atlas BEFORE any text renders.
    map_atlas_base();

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
                        // RFC-0079: relinquish our OWN send cap to our event
                        // inbox — windowd is the sole sender now. Otherwise
                        // this leftover SEND cap keeps the last-sender scan
                        // non-zero and the window-close EOF never fires.
                        let _ = nexus_abi::cap_close(EVENTS_SEND_CLONE_SLOT);
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
    let (mut safe_top, mut surf_w, mut surf_h) = if level == wire::WIN_LEVEL_DESKTOP
        || level == wire::WIN_LEVEL_OVERLAY
        || matches!(mode, wire::WIN_MODE_FULLSCREEN | wire::WIN_MODE_FREEFORM)
    {
        compositor_owned_geometry(&client, &events, &win, &mut boot_region)?
    } else {
        (0, SURFACE_W as u32, SURFACE_H as u32)
    };
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
    // WebRender scroll band geometry — ONLY for a floating windowed app that
    // actually scrolls (desktop/fullscreen surfaces keep the plain path; the
    // desktop uses a separate windowd path that ignores the scroll band).
    let band: Option<(u32, u32, u32)> = if level != wire::WIN_LEVEL_DESKTOP
        && mode != wire::WIN_MODE_FULLSCREEN
    {
        app.as_ref().and_then(|d| d.band_geometry()).filter(|&(h, f, c)| {
            // Resident-band budget: the gpud GL atlas holds 4000 rows
            // SHARED with every other resident surface (desktop base
            // = 800). A taller band still ALLOCATES but gpud clamps
            // its upload — the composite then samples transparent
            // rows and the window "vanishes" (the chat-thread
            // re-create bug). Too-tall content falls back to the
            // plain path honestly: visible-sized VMO, wheel-driven
            // re-emit scroll — slower, but complete and correct.
            let fits = h + f + c <= MAX_BAND_ROWS;
            if !fits {
                let _ = nexus_abi::debug_write(b"apphost: band too tall, plain-path fallback\n");
            }
            fits
        })
    } else {
        None
    };
    let (create_content_h, create_header_h, create_footer_h) =
        band.map_or((0u16, 0u16, 0u16), |(h, f, c)| {
            (
                c.min(u16::MAX as u32) as u16,
                h.min(u16::MAX as u32) as u16,
                f.min(u16::MAX as u32) as u16,
            )
        });
    // VMO height: the packed band (header + footer + content) when banded,
    // else the VISIBLE surface height (create `height` field stays VISIBLE).
    let vmo_h = band.map_or(surf_h, |(h, f, c)| h + f + c);
    if let Some(dsl) = app.as_mut() {
        dsl.banded = band.is_some();
        dsl.last_band = band;
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
    let create = wire::encode_surface_create(
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
    let mut surface_id =
        recv_ack(&events, wire::OP_SURFACE_CREATE, &mut pending_rect, &mut boot_region)?;
    if let Some(dsl) = app.as_mut() {
        dsl.set_surface_id(surface_id);
    }
    raw_marker("APPHOST: surface created");

    // 6. SURFACE_PRESENT seq=1, full damage — strictly one in flight.
    let mut damage = [wire::DamageRect { x: 0, y: 0, width: surf_w as u16, height: surf_h as u16 }];
    let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
    let len = wire::encode_surface_present(surface_id, 1, &damage, &mut buf);
    send_retry(&client, &buf[..len])?;
    let _ = recv_ack(&events, wire::OP_SURFACE_PRESENT, &mut pending_rect, &mut boot_region)?;
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
        let len = if let Some((inset, rw, rh)) = pending_rect.take() {
            // Replay the stashed rect VERBATIM — dropping `inset` here would
            // lose the safe area on exactly the create-time corrective push
            // that this stash exists to rescue.
            let f = wire::encode_surface_rect(0, inset, rw, rh);
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
            let wait = app.as_ref().map(|d| d.event_wait(animating)).unwrap_or(Wait::Blocking);
            // RFC-0079: opt into last-sender EOF — when windowd closes our
            // event channel (window closed), recv returns `Disconnected`
            // and the app self-exits (handled below) so its image returns
            // to the arena instead of parking forever (#29 app-side).
            match events.recv_into_eof(wait, &mut event_frame) {
                Ok(len) => {
                    recv_err_marked = false;
                    len
                }
                Err(nexus_ipc::IpcError::Timeout) | Err(nexus_ipc::IpcError::WouldBlock) => {
                    if let Some(dsl) = app.as_mut() {
                        if dsl.clock_supported() && dsl.clock_tick() {
                            dirty = true;
                            dirty_rows = None;
                        }
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
                    // Editable-field hover → windowd cursor hint (I-beam),
                    // sent only on CHANGE (enter/leave), never per move.
                    if let Some(over) = dsl.text_hover(i32::from(x), i32::from(y)) {
                        boot::send_cursor_hint(&client, surface_id, over);
                    }
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
                    if dsl.text_hover_clear() {
                        boot::send_cursor_hint(&client, surface_id, false);
                    }
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
                // Wheel impulse into the scroll physics (see `wheel_event`).
                if let Some(dsl) = app.as_mut() {
                    let (d, rows) = dsl.wheel_event(&client, surface_id, y, &mut wheel_rx_markers);
                    if d {
                        dirty_rows = match (dirty, dirty_rows, rows) {
                            (_, _, None) | (true, None, _) => None,
                            (_, Some((a0, a1)), Some(sp)) => Some((a0.min(sp.0), a1.max(sp.1))),
                            (false, None, Some(sp)) => Some(sp),
                        };
                        dirty = true;
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
                    let outcome = dsl.tap(i32::from(x), i32::from(y));
                    dsl.announce_text_focus(&client, surface_id, i32::from(x), i32::from(y));
                    // ONLY a repaint is dirty. A tap a handler absorbed
                    // without one (`PanelNoop`, or a control whose effect
                    // lands in another service) is NOT a miss; calling it
                    // one sent readers hunting a hit-test bug for days.
                    if outcome == TapOutcome::Repainted {
                        dirty = true;
                        // Model change: full repaint. The tap may also have
                        // started an animation — arm the frame pulse so the
                        // physics ticks on the real cadence.
                        dirty_rows = None;
                        if dsl.anim_active() {
                            let req = wire::encode_surface_frame_req(surface_id);
                            let _ = client.send(&req, Wait::NonBlocking);
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
        // Band re-negotiation: a dispatch/theme-driven re-emit can change
        // the page STRUCTURE (chat overview ⇄ thread) and with it the
        // packed-band geometry the surface was created with. The band
        // slices are mount-fixed on the windowd side, so the surface must
        // RE-CREATE — same flow as a WM resize, same size.
        if !recreate_surface && dirty {
            if let Some(dsl) = app.as_ref() {
                let now_band =
                    if level != wire::WIN_LEVEL_DESKTOP && mode != wire::WIN_MODE_FULLSCREEN {
                        // Same MAX_BAND_ROWS budget as surface create — the
                        // detector and the re-create MUST agree, or a too-tall
                        // page would re-create into the clamped-band vanish.
                        dsl.band_geometry().filter(|&(h, f, c)| h + f + c <= MAX_BAND_ROWS)
                    } else {
                        None
                    };
                if now_band != dsl.last_band {
                    recreate_surface = true;
                }
            }
        }
        // SHARED surface re-create (WM resize / band re-negotiation):
        // recompute the band, new VMO, destroy + re-create + present.
        if recreate_surface && app.is_some() {
            let band2: Option<(u32, u32, u32)> = if let Some(dsl) = app.as_ref() {
                if level != wire::WIN_LEVEL_DESKTOP && mode != wire::WIN_MODE_FULLSCREEN {
                    dsl.band_geometry().filter(|&(h, f, c)| h + f + c <= MAX_BAND_ROWS)
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
                    dsl.last_band = band2;
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
                        if let Ok(id) = ack {
                            surface_id = id;
                            if let Some(dsl) = app.as_mut() {
                                dsl.set_surface_id(id);
                            }
                            damage = [wire::DamageRect {
                                x: 0,
                                y: 0,
                                width: surf_w as u16,
                                height: surf_h as u16,
                            }];
                            // The fresh surface's seq restarts at 0 on the
                            // windowd side (strict last_seq+1). Reset ours
                            // so the next present is seq=1 — otherwise it's
                            // rejected BAD_SEQ and the frame never shows.
                            seq = 0;
                            present_in_flight = false;
                            (dirty, dirty_rows) = (true, None); // see `submit_layers`
                            raw_marker("apphost: resized");
                            // Fresh surface id: re-arm parked pulses.
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
