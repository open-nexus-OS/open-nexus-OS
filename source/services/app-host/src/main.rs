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

    /// The embedded fallback payload (compiled by `build.rs`). Since the
    /// GET_PAYLOAD step (TASK-0080D R2 remainder) the PRIMARY payload source
    /// is the VMO execd grants into [`PAYLOAD_VMO_SLOT`] — this embed is the
    /// fail-closed fallback (missing/late VMO, bad header), always marked.
    /// 8-byte aligned — capnp segments are word-aligned by contract and
    /// `include_bytes!` alone guarantees nothing (riscv misaligned-u64
    /// hazard).
    #[repr(C, align(8))]
    struct AlignedNxir<const N: usize>([u8; N]);
    static APP_NXIR_ALIGNED: AlignedNxir<
        { include_bytes!(concat!(env!("OUT_DIR"), "/app_payload.nxir")).len() },
    > = AlignedNxir(*include_bytes!(concat!(env!("OUT_DIR"), "/app_payload.nxir")));
    static APP_NXIR: &[u8] = &APP_NXIR_ALIGNED.0;

    /// Fixed child slot holding the payload VMO (execd's
    /// `CHILD_PAYLOAD_SLOT`); bundlemgrd fills it and writes the 16-byte
    /// header LAST (`nexus_abi::bundlemgrd::encode_payload_header`).
    const PAYLOAD_VMO_SLOT: u32 = 7;
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
    fn resolve_payload() -> &'static [u8] {
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
                        raw_marker("APPHOST: payload source=embedded (no vmo)");
                        return APP_NXIR;
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
                        raw_marker("APPHOST: payload source=embedded (header status)");
                        return APP_NXIR;
                    }
                    let mut buf = nexus_dsl_ir::read::AlignedBytes::zeroed(len as usize);
                    if vmo_read(PAYLOAD_VMO_SLOT, wire::PAYLOAD_DATA_OFFSET, buf.as_bytes_mut())
                        .is_err()
                    {
                        raw_marker("APPHOST: payload source=embedded (vmo read)");
                        return APP_NXIR;
                    }
                    raw_marker("APPHOST: payload source=bundle");
                    return alloc::boxed::Box::leak(alloc::boxed::Box::new(buf)).as_bytes();
                }
            }
            if nsec().unwrap_or(u64::MAX).saturating_sub(start) > PAYLOAD_BUDGET_NS {
                raw_marker("APPHOST: payload source=embedded (header timeout)");
                return APP_NXIR;
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

    pub(super) fn run() -> Result<(), &'static str> {
        raw_marker("apphost: start");

        // 1. The app's own surface VMO (per-app isolation, ADR-0037).
        let bytes = SURFACE_W as usize * SURFACE_H as usize * 4;
        let vmo = vmo_create(bytes).map_err(|_| "apphost: vmo create failed")?;

        // 2. Mount + render the DSL program into the VMO (R2); the R1 solid
        //    fill stays as the fail-closed VISIBLE fallback. The program
        //    bytes come from the GET_PAYLOAD VMO (slot 7) when execd granted
        //    one, else the embedded fallback — both marked.
        let payload = resolve_payload();
        let mut app = DslApp::mount(payload);
        match &app {
            Some(dsl) if dsl.render(vmo) => raw_marker("APPHOST: dsl frame rendered"),
            _ => {
                app = None;
                raw_marker("APPHOST: FAIL dsl mount (probe fill fallback)");
                let mut row = [0u8; SURFACE_W as usize * 4];
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&FILL_BGRA);
                }
                let row_bytes = SURFACE_W as usize * 4;
                for y in 0..SURFACE_H as usize {
                    vmo_write(vmo, y * row_bytes, &row)
                        .map_err(|_| "apphost: vmo fill failed")?;
                }
            }
        }
        raw_marker("apphost: vmo filled");

        let client = KernelClient::new_with_slots(WINDOWD_SEND_SLOT, WINDOWD_RECV_SLOT)
            .map_err(|_| "apphost: client slots")?;
        // The DEDICATED event channel: acks + input arrive here. execd
        // grants the slot before resume — a presence probe decides the
        // source honestly (fallback keeps older wiring alive, marked).
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

        // 3. SURFACE_CREATE — a CLONE of the VMO cap moves with the message
        //    (the gpud-attach pattern); the original stays ours for redraws.
        let clone = cap_clone(vmo).map_err(|_| "apphost: cap clone failed")?;
        let create = wire::encode_surface_create(SURFACE_W, SURFACE_H, wire::FORMAT_BGRA8888);
        send_retry_cap(&client, &create, clone)?;
        let surface_id = recv_ack(&events, wire::OP_SURFACE_CREATE)?;
        raw_marker("APPHOST: surface created");

        // 4. SURFACE_PRESENT seq=1, full damage — strictly one in flight.
        let damage = [wire::DamageRect { x: 0, y: 0, width: SURFACE_W, height: SURFACE_H }];
        let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
        let len = wire::encode_surface_present(surface_id, 1, &damage, &mut buf);
        send_retry(&client, &buf[..len])?;
        let _ = recv_ack(&events, wire::OP_SURFACE_PRESENT)?;
        raw_marker("APPHOST: probe surface presented");

        // 5. The event loop (R3): BLOCKING recv on the app channel — windowd
        //    routes body taps here (`OP_SURFACE_INPUT`, surface-local
        //    coordinates). A tap runs the interpreter's hit-testing
        //    (`View::pointer`); visible damage re-lays-out + re-renders the
        //    VMO and presents the next strictly-sequenced frame. Never a
        //    yield-spin: a Normal-QoS yield loop starves every Idle-QoS
        //    service (netstackd's exact failure mode). v1 limitation
        //    (recorded): taps arriving while we wait for a present ack are
        //    skipped by `recv_ack` as unrelated frames.
        let mut seq: u32 = 1;
        let mut event_frame = [0u8; 64];
        let mut recv_err_marked = false;
        let mut odd_frame_markers: u32 = 0;
        let mut tap_miss_markers: u32 = 0;
        raw_marker("APPHOST: event loop armed");
        loop {
            // Plain BLOCKING recv (P0.2): the sender-wake of an exec'd child
            // parked in a blocking recv is PROVEN every boot by the
            // recv-wake regression gate (`SELFTEST: exec child blocking recv
            // wake ok` — execd spawns recv-wake-probe post-ready). The
            // earlier Timeout(30ms) loop was a transitional workaround for
            // the #102-family finding (boot 2026-07-07T12-12); with the gate
            // green the reactive park is the production path — zero polls,
            // the kernel wakes us on message arrival.
            let len = match events.recv_into(Wait::Blocking, &mut event_frame) {
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
            };
            let Some((_, kind, x, y)) = wire::decode_surface_input(&event_frame[..len])
            else {
                // Stale ack / unrelated frame — bounded marker, never silent.
                if odd_frame_markers < 8 {
                    odd_frame_markers += 1;
                    raw_marker("apphost: event frame skipped (not input)");
                }
                continue;
            };
            if kind != wire::INPUT_KIND_TAP {
                continue;
            }
            let Some(dsl) = app.as_mut() else { continue };
            if !dsl.tap(i32::from(x), i32::from(y)) {
                // No handler hit / no visible change: report the first few
                // (coordinate-mapping bugs look exactly like this).
                if tap_miss_markers < 8 {
                    tap_miss_markers += 1;
                    raw_marker("apphost: input tap miss");
                }
                continue;
            }
            if !dsl.render(vmo) {
                raw_marker("apphost: FAIL interactive render");
                continue;
            }
            seq = seq.wrapping_add(1);
            let len = wire::encode_surface_present(surface_id, seq, &damage, &mut buf);
            if send_retry(&client, &buf[..len]).is_err()
                || recv_ack(&events, wire::OP_SURFACE_PRESENT).is_err()
            {
                raw_marker("apphost: FAIL interactive present");
                continue;
            }
            raw_marker("APPHOST: interactive frame presented");
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
    }

    impl DslApp {
        /// Validates + mounts the program bytes and lays them out at
        /// surface size. `None` on any failure (fail-closed; caller shows
        /// the probe fill).
        fn mount(nxir: &'static [u8]) -> Option<Self> {
            use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

            let runtime = nexus_dsl_runtime::Runtime::mount(nxir).ok()?;
            let symbols = runtime.symbols().to_vec();
            emit_mounted_hash_marker(nxir);
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
            let tokens = nexus_dsl_runtime::theme_tokens::BaseTokens;
            let device = FixtureEnv::default();
            let view = {
                let locale = IdentityLocale { symbols: &symbols, keys: &keys };
                View::mount(nxir, &tokens, &device, &locale).ok()?
            };
            let engine = nexus_layout::LayoutEngine::new();
            let layout = engine
                .layout(
                    view.scene(),
                    nexus_layout_types::FxPx::new(SURFACE_W as i32),
                    &nexus_text_baked::measure_text::BakedTextMeasure,
                )
                .ok()?;
            let mut texts = alloc::vec::Vec::new();
            collect_texts(view.scene(), &mut 0, &mut texts);
            Some(Self { view, symbols, keys, layout, texts })
        }

        /// Runs the interpreter's hit-testing for a body tap; on visible
        /// damage re-lays-out + refreshes the text runs. Returns whether a
        /// re-render is needed.
        fn tap(&mut self, x: i32, y: i32) -> bool {
            use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, NoIo};
            let tokens = nexus_dsl_runtime::theme_tokens::BaseTokens;
            let device = FixtureEnv::default();
            let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
            let damage = self
                .view
                .pointer(
                    &tokens,
                    &device,
                    &locale,
                    &mut NoIo,
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
            let engine = nexus_layout::LayoutEngine::new();
            let Ok(layout) = engine.layout(
                self.view.scene(),
                nexus_layout_types::FxPx::new(SURFACE_W as i32),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            ) else {
                return false;
            };
            self.layout = layout;
            self.texts.clear();
            collect_texts(self.view.scene(), &mut 0, &mut self.texts);
            true
        }

        /// Writes the current scene (fills + glyph runs) into the VMO. The
        /// page base is the theme's Surface token — the scene's own boxes
        /// (surfaceVariant buttons, onSurface text) are specified against it.
        fn render(&self, vmo: u32) -> bool {
            use nexus_dsl_runtime::theme_tokens::{ColorToken, Tokens};
            let s = nexus_dsl_runtime::theme_tokens::BaseTokens.color(ColorToken::Surface);
            let base = [s.b, s.g, s.r, s.a];
            let row_bytes = SURFACE_W as usize * 4;
            let mut row = alloc::vec![0u8; row_bytes];
            for y in 0..SURFACE_H as i32 {
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&base);
                }
                for b in &self.layout.boxes {
                    let (bx, by, bw, bh) =
                        (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                    if bw <= 0 || bh <= 0 || y < by || y >= by + bh {
                        continue;
                    }
                    if let Some(bg) = b.visual.background {
                        let x0 = bx.max(0) as usize;
                        let x1 = ((bx + bw).max(0) as usize).min(SURFACE_W as usize);
                        for px in row[x0 * 4..x1 * 4].chunks_exact_mut(4) {
                            px.copy_from_slice(&[bg.b, bg.g, bg.r, bg.a]);
                        }
                    }
                    // Glyph pass: the shared text SSOT (same blender windowd
                    // uses) blends the run slice intersecting this row.
                    if let Some((_, content, font, color)) =
                        self.texts.iter().find(|(id, _, _, _)| *id == b.node_id)
                    {
                        nexus_text_baked::draw_text_row(
                            &mut row,
                            y as u32,
                            by,
                            bx.max(0) as u32,
                            SURFACE_W as u32,
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
    fn recv_ack(client: &KernelClient, op: u8) -> Result<u32, &'static str> {
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
