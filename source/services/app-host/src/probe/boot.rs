// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host boot/wire helpers (pure move out of `main.rs`): payload resolution,
//! the window-intent + mounted-hash markers, the theme/profile boot-push wait,
//! the content-rect geometry handshake, and the bounded send/recv-ack wire IO.
//! No behavior change.

use super::*;

/// Resolves the program bytes: the granted payload VMO when present and
/// well-formed (leaked once — the app-host process IS one app instance,
/// so the payload lives for the process), otherwise the embedded
/// fallback. Marked on both paths (`APPHOST: payload source=…`).
pub(super) fn resolve_payload() -> Option<&'static [u8]> {
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

/// `APPHOST: mounted hash=<first-16-hex>` — the R2 DoD marker.
pub(super) fn emit_mounted_hash_marker(nxir: &[u8]) {
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
pub(super) fn emit_window_intent_marker(nxir: &[u8]) {
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

/// The attach-time `OP_SURFACE_REGION` push (locale/tz/hour format),
/// stashed by the pre-mount event-channel drains instead of dropped —
/// windowd only re-pushes region data on CHANGE, so a dropped attach push
/// left every fresh mount at the baked defaults (English UI, default tz)
/// until the next settings change.
pub(super) struct RegionPush {
    pub hour_fmt: u8,
    pub locale: alloc::string::String,
    pub tz: alloc::string::String,
    pub keymap: alloc::string::String,
}

/// Stashes the region half of an RFC-0083 presentation snapshot (LATEST
/// wins). False for any other frame. (The legacy `OP_SURFACE_REGION` push is
/// retired — the snapshot is the only region carrier.)
pub(super) fn stash_region(frame: &[u8], slot: &mut Option<RegionPush>) -> bool {
    let Some(snap) = nexus_display_proto::surface_settings::decode_surface_settings(frame) else {
        return false;
    };
    *slot = Some(RegionPush {
        hour_fmt: snap.hour_fmt,
        locale: alloc::string::String::from(snap.locale),
        tz: alloc::string::String::from(snap.tz),
        keymap: alloc::string::String::from(snap.keymap),
    });
    true
}

/// Drains a still-queued attach-burst REGION frame, non-blocking. The attach
/// burst is theme+profile+REGION and `wait_for_boot_pushes` returns on
/// profile — for a NORMAL window the region frame is still queued at mount
/// (desktop/fullscreen surfaces consume it inside `request_content_rect`;
/// nothing else is legal pre-create). Without this drain the app paints its
/// baked-default locale and the create/present ack-wait stashes are never
/// applied.
pub(super) fn drain_region(events: &KernelClient, slot: &mut Option<RegionPush>) {
    if slot.is_some() {
        return;
    }
    let mut frame = [0u8; 96];
    while let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
        if stash_region(&frame[..len], slot) {
            break;
        }
    }
}

/// Fire-and-forget cursor hint to windowd (`OP_SURFACE_CURSOR_HINT`):
/// I-beam while the pointer hovers an editable field, default otherwise.
pub(super) fn send_cursor_hint(client: &KernelClient, surface_id: u32, over_text: bool) {
    use nexus_display_proto::surface_text as st;
    let shape = if over_text { st::CURSOR_HINT_TEXT } else { st::CURSOR_HINT_DEFAULT };
    let hint = st::encode_surface_cursor_hint(surface_id, shape);
    let _ = client.send(&hint, Wait::NonBlocking);
}

/// Bounded wait for windowd's boot push: the RFC-0083 `OP_SURFACE_SETTINGS`
/// snapshot (theme + accent + profile + region in ONE frame, marked due at
/// event-channel attach and delivered by the next compositor pump — before we
/// mount). Returns `(theme, profile, settings_gen)`; compositor defaults
/// (dark / tablet) if nothing arrives in time — the app still renders, just
/// possibly not matched until the next push. The snapshot's `gen` seeds the
/// main loop's dedupe (the boot snapshot IS delivered — a dropped gen would
/// mark it as never-sent).
pub(super) fn wait_for_boot_pushes(
    events: &KernelClient,
    region: &mut Option<RegionPush>,
) -> (u8, u8, Option<u32>) {
    let start = nsec().unwrap_or(0);
    let mut frame = [0u8; 96];
    loop {
        if let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
            if let Some(snap) =
                nexus_display_proto::surface_settings::decode_surface_settings(&frame[..len])
            {
                raw_marker("APPHOST: settings received");
                *region = Some(RegionPush {
                    hour_fmt: snap.hour_fmt,
                    locale: alloc::string::String::from(snap.locale),
                    tz: alloc::string::String::from(snap.tz),
                    keymap: alloc::string::String::from(snap.keymap),
                });
                return (snap.theme, snap.profile, Some(snap.gen));
            }
            // Anything else queued pre-mount (acks from a prior life, input)
            // is not ours to interpret yet — drop and keep waiting.
        }
        if nsec().unwrap_or(u64::MAX).saturating_sub(start) > 500_000_000 {
            return (wire::THEME_DARK, wire::PROFILE_TABLET, None);
        }
        let _ = yield_();
    }
}

/// Reads the app's window intent from the payload as the `WIN_*` wire tags
/// (style, level, mode). Absent `Window {}` ⇒ the ordinary defaults.
pub(super) fn read_window_intent_tags(nxir: &[u8]) -> (u8, u8, u8) {
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

/// The declared window intent for one geometry handshake, kept together so the
/// nonce can never drift away from the style/level/mode it belongs to.
pub(super) struct WindowIntent {
    pub(super) style: u8,
    pub(super) level: u8,
    pub(super) mode: u8,
    pub(super) nonce: u64,
}

/// Geometry handshake: send the window intent (`OP_SURFACE_INTENT`) and wait
/// (bounded) for windowd's composed content rect (`OP_SURFACE_RECT`) on the
/// event channel. `None` if windowd never answers. The WM owns geometry; the
/// app sizes its VMO to whatever rect it gets.
///
/// The intent is RE-DRIVEN once before giving up: the first ask can be lost
/// outright (2026-07-25, `build/logs/manual--2026-07-25T15-57-20`: windowd's
/// gpud-reply drain was consuming client frames off an aliased endpoint, so
/// both the attach and this intent vanished), and a lost ask is
/// indistinguishable from a slow WM until the second one also goes unanswered.
/// Callers for whom geometry is MANDATORY use [`compositor_owned_geometry`]
/// instead of substituting a default for `None`.
pub(super) fn request_content_rect(
    client: &KernelClient,
    events: &KernelClient,
    win: &WindowIntent,
    region: &mut Option<RegionPush>,
) -> Option<(u32, u32, u32)> {
    // Nonce-correlated: windowd answers on OUR event channel — without it,
    // concurrent mounts stole each other's rect and every app fell back.
    let intent = wire::encode_surface_intent(win.style, win.level, win.mode, false, win.nonce);
    // Attempt 1 pays the full early-boot budget; attempt 2 re-drives the ask
    // with a short budget, so a LOST intent costs ~1s instead of a session.
    const BUDGETS_NS: [u64; 2] = [8_000_000_000, 1_000_000_000];
    let mut frame = [0u8; 96];
    for (attempt, budget_ns) in BUDGETS_NS.iter().enumerate() {
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
        if attempt > 0 {
            raw_marker("apphost: content rect re-driven");
        }
        let start = nsec().unwrap_or(0);
        loop {
            if let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
                if let Some((_, inset, w, h)) = wire::decode_surface_rect(&frame[..len]) {
                    raw_marker("APPHOST: content rect received");
                    return Some((u32::from(inset), u32::from(w), u32::from(h)));
                }
                // The attach-time region push races the rect on this channel —
                // stash it (dropping it un-localized every fresh mount).
                let _ = stash_region(&frame[..len], region);
            }
            // 8s on the first ask: early-boot windowd can lag several seconds
            // before it drains the request queue (grown image); with the
            // parked-reply flush a LATE answer is correct — falling back early
            // re-created the 320x240/splash-hang class this budget exists to
            // avoid.
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > *budget_ns {
                break;
            }
            let _ = yield_();
        }
    }
    raw_marker("apphost: no content rect (fallback)");
    None
}

/// Geometry for a surface whose size the COMPOSITOR owns (desktop, overlay,
/// fullscreen): windowd's content rect is the only legitimate answer, so a
/// missing one is a hard failure — never the probe default.
///
/// Substituting the default was a fake green: on 2026-07-25 a lost intent
/// produced a `320x240` "desktop" that mounted, presented frames and never
/// routed input (`build/logs/manual--2026-07-25T15-57-20`) — a dead session
/// that looked alive for 20 s of backpressure before anyone could tell. Failing
/// here exits the process with a logged error (`nexus-service-entry` maps `Err`
/// to `exit(-1)`), which is diagnosable in one line and reclaimable by execd.
pub(super) fn compositor_owned_geometry(
    client: &KernelClient,
    events: &KernelClient,
    win: &WindowIntent,
    region: &mut Option<RegionPush>,
) -> Result<(u32, u32, u32), &'static str> {
    match request_content_rect(client, events, win, region) {
        Some(rect) => Ok(rect),
        None => {
            raw_marker(
                "APPHOST: FAIL no content rect (compositor owns this surface — refusing probe-size desktop)",
            );
            Err("apphost: no content rect for compositor-owned surface")
        }
    }
}

/// Sends with bounded retries: the fixed slots may not be populated yet
/// (execd transfers after spawn) and windowd may still be booting.
pub(super) fn send_retry(client: &KernelClient, frame: &[u8]) -> Result<(), &'static str> {
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

pub(super) fn send_retry_cap(
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
pub(super) fn recv_ack(
    client: &KernelClient,
    op: u8,
    pending_rect: &mut Option<(u16, u16, u16)>,
    region: &mut Option<RegionPush>,
) -> Result<u32, &'static str> {
    // 96: an RFC-0083 snapshot (max 70 bytes) may race the ack on this
    // channel — a 64-byte buffer would truncate it into an undecodable frame.
    let mut frame = [0u8; 96];
    let start = nsec().unwrap_or(0);
    loop {
        match client.recv_into(Wait::NonBlocking, &mut frame) {
            Ok(len) => {
                if let Some((status, value)) = wire::decode_surface_ack(&frame[..len], op) {
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
                if let Some((_, inset, w, h)) = wire::decode_surface_rect(&frame[..len]) {
                    *pending_rect = Some((inset, w, h));
                    continue;
                }
                // The attach-time region push races the create/present acks
                // for LAUNCHED window apps (no content-rect wait ran) —
                // stash it or fresh mounts stay un-localized (the chat-app
                // English-despite-de finding, RFC-0075 Phase 8b).
                if stash_region(&frame[..len], region) {
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
