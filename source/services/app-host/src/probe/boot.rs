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
    use nexus_abi::{cap_clone, cap_close, vmo_read};
    // Slot presence: execd grants the payload VMO BEFORE it resumes this task (TASK-0324
    // P4e), so the slot is there or the launch is wrong — one probe (cap_clone+close: the
    // established presence pattern), no wait.
    match cap_clone(PAYLOAD_VMO_SLOT) {
        Ok(probe) => {
            let _ = cap_close(probe);
        }
        Err(_) => {
            raw_marker("APPHOST: FAIL payload (no vmo)");
            return None;
        }
    }
    // The header: bundlemgrd wrote it AFTER the payload bytes and ANSWERED execd, and execd
    // resumed this task only after that answer (TASK-0324 P7-d) — one read, fail-closed:
    // an unwritten or bad header is a launch defect, never something to wait for.
    let mut hdr = [0u8; nexus_abi::payload_vmo::HEADER_LEN];
    if vmo_read(PAYLOAD_VMO_SLOT, 0, &mut hdr).is_err() {
        raw_marker("APPHOST: FAIL payload (header read)");
        return None;
    }
    let Some((status, len)) = nexus_abi::payload_vmo::decode_header(&hdr) else {
        raw_marker("APPHOST: FAIL payload (header unwritten)");
        return None;
    };
    if status != nexus_abi::status::CODE_OK
        || len == 0
        || len as usize > PAYLOAD_MAX_LEN
        || len % 8 != 0
    {
        raw_marker("APPHOST: FAIL payload (header status)");
        return None;
    }
    let mut buf = nexus_dsl_ir::read::AlignedBytes::zeroed(len as usize);
    if vmo_read(PAYLOAD_VMO_SLOT, nexus_abi::payload_vmo::DATA_OFFSET, buf.as_bytes_mut()).is_err()
    {
        raw_marker("APPHOST: FAIL payload (vmo read)");
        return None;
    }
    raw_marker("APPHOST: payload source=bundle");
    Some(alloc::boxed::Box::leak(alloc::boxed::Box::new(buf)).as_bytes())
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
    while let Ok((len, cap)) = events.recv_into_with_cap(Wait::NonBlocking, &mut frame, false) {
        close_moved(cap);
        if stash_region(&frame[..len.min(frame.len())], slot) {
            break;
        }
    }
}

/// A capability that moved with a frame a launch's handshake has no use for is closed — kept, it
/// would be a sender of the app's own channel and its EOF would never come. A compositor that
/// died during the launch (its BACK among them) thereby ends the launch's next wait.
fn close_moved(cap: Option<u32>) {
    if let Some(cap) = cap {
        let _ = nexus_abi::cap_close(cap);
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
    let mut frame = [0u8; 96];
    // WAIT for the push: windowd marks the snapshot due at event-channel attach and delivers
    // it on its next pump — it always comes (or windowd is gone), so nothing here is timed.
    while let Ok(len) = recv_event(events, &mut frame) {
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
    (wire::THEME_DARK, wire::PROFILE_TABLET, None)
}

/// One event-channel receive that WAITS in the kernel until a frame arrives — or windowd,
/// the channel's only sender, is gone (last-sender EOF, RFC-0079). No clock (TASK-0324
/// P7-b): the compositor's answer or its death, nothing else, ends a boot handshake.
pub(super) fn recv_event(events: &KernelClient, frame: &mut [u8]) -> Result<usize, ()> {
    let (len, cap) = events.recv_into_with_cap(Wait::Blocking, frame, true).map_err(|_| ())?;
    close_moved(cap);
    Ok(len.min(frame.len()))
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

/// The page base's alpha for a window of `level`/`mode` (declarative base alpha). A DESKTOP
/// surface paints a fully TRANSPARENT base — windowd blends the band over the retained wallpaper
/// plane, so empty desktop area IS the wallpaper. The OSK band (overlay) and a fullscreen
/// floating surface (a kiosk app) stay opaque: keys never blend with what lies below, a kiosk
/// owns every pixel. A frosted floating window leaves ~1/3 of the blurred backdrop visible (190
/// read as a solid slab; opaque ELEMENTS still paint fully on top).
pub(super) fn base_alpha(level: u8, mode: u8) -> u8 {
    if level == wire::WIN_LEVEL_DESKTOP {
        0
    } else if level == wire::WIN_LEVEL_OVERLAY || mode == wire::WIN_MODE_FULLSCREEN {
        255
    } else {
        168
    }
}

/// The declared window intent for one geometry handshake, kept together so the
/// nonce can never drift away from the style/level/mode it belongs to.
pub(super) struct WindowIntent {
    pub(super) style: u8,
    pub(super) level: u8,
    pub(super) mode: u8,
    pub(super) nonce: u64,
}

/// Geometry handshake: send the window intent (`OP_SURFACE_INTENT`) and WAIT
/// for windowd's composed content rect (`OP_SURFACE_RECT`) on the event
/// channel. `None` if windowd never answers inside the liveness bound. The WM
/// owns geometry; the app sizes its VMO to whatever rect it gets.
///
/// ONE ask (TASK-0324 P7). The second "re-driven" ask existed for a lost first
/// one (2026-07-25: windowd's gpud-reply drain consumed client frames off an
/// aliased endpoint) — that endpoint aliasing is gone (wired slots + envelope
/// guard), and a wait that the kernel wakes cannot lose the answer to its own
/// iteration speed. Callers for whom geometry is MANDATORY use
/// [`compositor_owned_geometry`] instead of substituting a default for `None`.
pub(super) fn request_content_rect(
    client: &KernelClient,
    events: &KernelClient,
    win: &WindowIntent,
    region: &mut Option<RegionPush>,
) -> Option<(u32, u32, u32)> {
    // Nonce-correlated: windowd answers on OUR event channel — without it,
    // concurrent mounts stole each other's rect and every app fell back.
    let intent = wire::encode_surface_intent(win.style, win.level, win.mode, false, win.nonce);
    if send_wait(client, &intent).is_err() {
        return None;
    }
    // No clock: early-boot windowd may take seconds to drain its request queue, and its
    // answer is correct however late it comes — the only other end is windowd's death.
    let mut frame = [0u8; 96];
    while let Ok(len) = recv_event(events, &mut frame) {
        if let Some((_, inset, w, h)) = wire::decode_surface_rect(&frame[..len]) {
            raw_marker("APPHOST: content rect received");
            return Some((u32::from(inset), u32::from(w), u32::from(h)));
        }
        // The attach-time region push races the rect on this channel —
        // stash it (dropping it un-localized every fresh mount).
        let _ = stash_region(&frame[..len], region);
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

/// Sends one request, WAITING in the kernel for queue space (TASK-0324 P7): a full windowd
/// queue is backpressure to wait out; a dead windowd ends the wait. No clock.
pub(super) fn send_wait(client: &KernelClient, frame: &[u8]) -> Result<(), &'static str> {
    if client.send(frame, Wait::Blocking).is_ok() {
        return Ok(());
    }
    let _ = debug_println("apphost: FAIL send");
    Err("apphost: send failed")
}

/// The create frame of this app's surface (RFC-0109): its size, its declared intent, its nonce
/// and the store `plan` takes.
pub(super) fn create_frame(
    (w, h): (u32, u32),
    win: &WindowIntent,
    resizable: bool,
    plan: band::StorePlan,
) -> [u8; wire::SURFACE_CREATE_FRAME_LEN] {
    wire::encode_surface_create(&wire::SurfaceCreate {
        width: w as u16,
        height: h as u16,
        format: wire::FORMAT_BGRA8888,
        style: win.style,
        level: win.level,
        mode: win.mode,
        resizable,
        nonce: win.nonce,
        rows: plan.rows.min(u32::from(u16::MAX)) as u16,
        store_w: plan.width,
    })
}

/// Why a create came to nothing.
pub(super) enum CreateFailed {
    /// The compositor is gone (a send to it failed, its channel closed, or a new one is back):
    /// the window waits for the next one (RFC-0112).
    Gone,
    /// The compositor refused the surface, or its buffer could not be mapped.
    Refused(&'static str),
    /// The compositor closed the window on purpose while the create waited (RFC-0112).
    Closed,
}

impl From<CreateFailed> for &'static str {
    fn from(failed: CreateFailed) -> Self {
        match failed {
            CreateFailed::Gone => "apphost: compositor gone during create",
            CreateFailed::Refused(why) => why,
            CreateFailed::Closed => "apphost: window closed during create",
        }
    }
}

/// The surface for `plan` (RFC-0114 §3): the create — no capability rides with it — and
/// windowd's answer, the buffer gpud made for this app with its capability moved along, mapped.
/// A refusal for want of memory while the plan has a store asks once more without one (every
/// scroll layer flat); any other refusal ends the launch. Returns the surface's id and buffer.
pub(super) fn create_surface(
    (client, events): (&KernelClient, &KernelClient),
    (size, win, resizable): ((u32, u32), &WindowIntent, bool),
    dsl: &mut DslApp,
    plan: band::StorePlan,
    (pending_rect, region): (&mut Option<(u16, u16, u16)>, &mut Option<RegionPush>),
) -> Result<(u32, super::surface_mem::SurfaceMem), CreateFailed> {
    let mut plan = plan;
    loop {
        send_wait(client, &create_frame(size, win, resizable, plan))
            .map_err(|_| CreateFailed::Gone)?;
        let (b, cap) = recv_buffers(events, pending_rect, region)?;
        match cap {
            Some(cap) if b.status == wire::SURFACE_STATUS_OK => {
                let mem =
                    super::surface_mem::SurfaceMem::map(&b, cap).map_err(CreateFailed::Refused)?;
                dsl.created(plan);
                if crate::proof_line::SURFACE_MAPPED.claim() {
                    raw_marker(&alloc::format!(
                        "apphost: surface mapped (id={} slots={} stride={} rows={} bytes={} clean={})",
                        b.surface_id,
                        b.slots,
                        b.stride,
                        b.rows,
                        mem.bytes(),
                        b.cache_block
                    ));
                }
                return Ok((b.surface_id, mem));
            }
            refused => {
                if let Some(cap) = refused {
                    let _ = nexus_abi::cap_close(cap);
                }
                if b.status == wire::SURFACE_STATUS_QUOTA
                    && plan.rows > size.1
                    && !dsl.store_refused
                {
                    plan = dsl.refuse_store();
                    continue;
                }
                let _ = debug_println("apphost: FAIL surface refused");
                return Err(CreateFailed::Refused("apphost: surface refused"));
            }
        }
    }
}

/// WAITS for windowd's answer to a create (`OP_SURFACE_BUFFERS`) and the capability that moved
/// with it. A content rect or a region push interleaving with it is stashed for the event loop;
/// anything else waits for the loop that asked for it (a capability with it is closed). A BACK
/// ends the wait — its SEND is kept for the rejoin: the compositor asked died — and so does a
/// CLOSED: the window went on purpose while it waited (RFC-0112).
fn recv_buffers(
    events: &KernelClient,
    pending_rect: &mut Option<(u16, u16, u16)>,
    region: &mut Option<RegionPush>,
) -> Result<(wire::Buffers, Option<u32>), CreateFailed> {
    let mut frame = [0u8; 96];
    while let Ok((len, cap)) = events.recv_into_with_cap(Wait::Blocking, &mut frame, true) {
        let len = len.min(frame.len());
        if let Some(b) = wire::decode_surface_buffers(&frame[..len]) {
            return Ok((b, cap));
        }
        if super::reconnect::take_cap(&frame[..len], cap) {
            return Err(CreateFailed::Gone);
        }
        if nexus_display_proto::surface_restart::is_surface_closed(&frame[..len]) {
            return Err(CreateFailed::Closed);
        }
        stash_interleaved(&frame[..len], pending_rect, region);
    }
    let _ = debug_println("apphost: create answer channel closed");
    Err(CreateFailed::Gone)
}

/// A content rect interleaving with an answer (windowd pushes it INSIDE create handling, so it
/// precedes the answer on this channel): the LATEST is stashed for the event loop — dropped, the
/// surface stayed at the probe size forever. The attach-time region push races the answers for
/// LAUNCHED apps (no content-rect wait ran): stashed too, or fresh mounts stay un-localized.
fn stash_interleaved(
    frame: &[u8],
    pending_rect: &mut Option<(u16, u16, u16)>,
    region: &mut Option<RegionPush>,
) {
    if let Some((_, inset, w, h)) = wire::decode_surface_rect(frame) {
        *pending_rect = Some((inset, w, h));
        return;
    }
    let _ = stash_region(frame, region);
}

/// WAITS for the ack of `op` (a present's: the release of the other slot); its value on OK.
pub(super) fn recv_ack(
    client: &KernelClient,
    op: u8,
    pending_rect: &mut Option<(u16, u16, u16)>,
    region: &mut Option<RegionPush>,
) -> Result<u32, &'static str> {
    // 96: an RFC-0083 snapshot (max 70 bytes) may race the ack on this
    // channel — a 64-byte buffer would truncate it into an undecodable frame.
    let mut frame = [0u8; 96];
    // EOF asked: a compositor that dies before it acks ends the launch (no clock otherwise).
    while let Ok((len, cap)) = client.recv_into_with_cap(Wait::Blocking, &mut frame, true) {
        close_moved(cap);
        let len = len.min(frame.len());
        if let Some((status, value)) = wire::decode_surface_ack(&frame[..len], op) {
            if status == wire::SURFACE_STATUS_OK {
                return Ok(value);
            }
            let _ = debug_println("apphost: FAIL surface ack status");
            return Err("apphost: ack status");
        }
        stash_interleaved(&frame[..len], pending_rect, region);
    }
    let _ = debug_println("apphost: FAIL ack channel closed");
    Err("apphost: ack channel closed")
}
