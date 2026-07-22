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

/// Stashes `frame` when it is a region push (LATEST wins). False otherwise.
pub(super) fn stash_region(frame: &[u8], slot: &mut Option<RegionPush>) -> bool {
    let Some((hf, loc, tzv, km)) = nexus_display_proto::surface_text::decode_surface_region(frame)
    else {
        return false;
    };
    *slot = Some(RegionPush {
        hour_fmt: hf,
        locale: alloc::string::String::from(loc),
        tz: alloc::string::String::from(tzv),
        keymap: alloc::string::String::from(km),
    });
    true
}

/// Bounded wait for windowd's boot pushes (`OP_SURFACE_THEME` +
/// `OP_SURFACE_PROFILE`, sent when the event channel attaches — before we
/// mount). Returns `(theme, profile)`; either defaults (dark / tablet, the
/// compositor defaults) if it does not arrive in time — the app still
/// renders, just possibly not matched until the next push. A region push
/// interleaving here is STASHED for the post-mount apply, never dropped.
pub(super) fn wait_for_boot_pushes(
    events: &KernelClient,
    region: &mut Option<RegionPush>,
) -> (u8, u8) {
    let start = nsec().unwrap_or(0);
    let mut frame = [0u8; 64];
    let mut theme: Option<u8> = None;
    let mut profile: Option<u8> = None;
    loop {
        if let Ok(len) = events.recv_into(Wait::NonBlocking, &mut frame) {
            if let Some(mode) = wire::decode_surface_theme(&frame[..len]) {
                raw_marker("APPHOST: theme received");
                theme = Some(mode);
            } else if let Some(p) = wire::decode_surface_profile(&frame[..len]) {
                raw_marker("APPHOST: profile received");
                profile = Some(p);
            } else {
                let _ = stash_region(&frame[..len], region);
            }
            if let (Some(t), Some(p)) = (theme, profile) {
                return (t, p);
            }
        }
        if nsec().unwrap_or(u64::MAX).saturating_sub(start) > 500_000_000 {
            return (theme.unwrap_or(wire::THEME_DARK), profile.unwrap_or(wire::PROFILE_TABLET));
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

/// Geometry handshake: send the window intent (`OP_SURFACE_INTENT`) and wait
/// (bounded) for windowd's composed content rect (`OP_SURFACE_RECT`) on the
/// event channel. `None` if windowd never answers (older WM) — the caller
/// falls back to the probe default. The WM owns geometry; the app sizes its
/// VMO to whatever rect it gets.
pub(super) fn request_content_rect(
    client: &KernelClient,
    events: &KernelClient,
    style: u8,
    level: u8,
    mode: u8,
    nonce: u64,
    region: &mut Option<RegionPush>,
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
            // The attach-time region push races the rect on this channel —
            // stash it (dropping it un-localized every fresh mount).
            let _ = stash_region(&frame[..len], region);
        }
        // 8s: early-boot windowd can lag several seconds before it drains
        // the request queue (grown image); with the parked-reply flush a
        // LATE answer is correct — falling back early re-created the
        // 320x240/splash-hang class this budget exists to avoid.
        if nsec().unwrap_or(u64::MAX).saturating_sub(start) > 8_000_000_000 {
            raw_marker("apphost: no content rect (fallback)");
            return None;
        }
        let _ = yield_();
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
    pending_rect: &mut Option<(u16, u16)>,
    region: &mut Option<RegionPush>,
) -> Result<u32, &'static str> {
    let mut frame = [0u8; 64];
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
                if let Some((_, _, w, h)) = wire::decode_surface_rect(&frame[..len]) {
                    *pending_rect = Some((w, h));
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
