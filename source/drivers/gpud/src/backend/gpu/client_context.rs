// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A client's context on the board (TASK-0215 5c-3 P3, RFC-0101 §1, §6, §7b): the GPU beside the
//! display as the request loop serves it ([`BoardGpu`], a `GpuService`) — the capabilities from
//! the device, an open for windowd (the loop checked the sender and mapped the library). An open
//! replaces the client's context whole: a restarted windowd gets a fresh one, the old one torn
//! down by the firmware's cleanup and its bundle's buffers destroyed. The new one is made as the
//! render rungs' is — the context at the priority asked, the render bundle bound with its target
//! left unbound (windowd names every pass's target), the render objects set up, the client's
//! library placed (`library.rs`): `gpud: gpu library ok (client=windowd …)`. Every line is
//! formatted on the stack: an open costs gpud's heap, which never frees, the library's table
//! alone (`heap_bytes=`).

use nexus_display_proto::gpu_context::{
    ContextOpened, GpuCapabilities, PRIORITY_HIGH, PRIORITY_NORMAL, PRIORITY_REALTIME,
};
use nexus_display_proto::{STATUS_DEVICE_ERROR, STATUS_OK, STATUS_REFUSED};
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::backend::v2::{ClientId, ContextId, DeviceError, GfxDevice, LibraryTaken, Priority};

use super::device_v2::Device;
use super::emit_fmt;
use super::render_template::RENDER_BUNDLE;
use super::scene_load::{self, Loaded, TargetMem, Texel};
use crate::gpu_context::{refused, GpuService};

/// A context a client opened, with the render bundle in it.
struct Opened {
    client: ClientId,
    ctx: ContextId,
    scene: Loaded,
}

/// The board's GPU as the request loop serves it.
pub(crate) struct BoardGpu {
    pub(crate) dev: Device,
    /// windowd's context, once it opened one (windowd alone opens).
    held: Option<Opened>,
}

/// Why an open made nothing: the step and the device's word, when it said one.
type Refusal = (&'static str, Option<DeviceError>);

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

const fn priority(p: u8) -> Priority {
    match p {
        PRIORITY_REALTIME => Priority::Realtime,
        PRIORITY_HIGH => Priority::High,
        PRIORITY_NORMAL => Priority::Normal,
        _ => Priority::Low,
    }
}

impl BoardGpu {
    pub(crate) fn new(dev: Device) -> Self {
        Self { dev, held: None }
    }

    /// Tears an opened context down: the context by the firmware's cleanup (quarantined, its
    /// memory kept, when the firmware will not let go), then the bundle's buffers — a buffer
    /// still bound is refused, never freed.
    fn retire(&mut self, old: Opened) -> Result<(), DeviceError> {
        let gone = self.dev.destroy_context(old.client, old.ctx);
        old.scene.release(&mut self.dev, old.client, old.ctx);
        gone
    }

    /// The new context, made as the render rungs' is; on a refusal everything it took is given
    /// back.
    fn make(
        &mut self,
        client: ClientId,
        prio: Priority,
        library: &[u8],
    ) -> Result<(Opened, LibraryTaken), Refusal> {
        let core = CoreId(self.dev.gpu.win.core_id());
        let scene = Scene::parse(RENDER_BUNDLE, core).map_err(|_| ("bundle", None))?;
        let setup = scene.render().ok_or(("render trailer", None))?;
        let ctx = self.dev.create_context(client, prio).map_err(|e| ("context", Some(e)))?;
        let loaded = match scene_load::load(
            &mut self.dev,
            client,
            ctx,
            &scene,
            TargetMem::Unbound,
            Texel::Checker,
        ) {
            Ok(l) => l,
            Err(_) => {
                let _ = self.dev.destroy_context(client, ctx);
                return Err(("bundle bind", None));
            }
        };
        let opened = Opened { client, ctx, scene: loaded };
        let taken = self
            .dev
            .render_setup(client, ctx, &setup)
            .map_err(|e| ("render setup", Some(e)))
            .and_then(|()| {
                self.dev.take_library(client, ctx, library).map_err(|e| ("library", Some(e)))
            });
        match taken {
            Ok(t) => Ok((opened, t)),
            Err(why) => {
                let _ = self.retire(opened);
                Err(why)
            }
        }
    }
}

impl GpuService for BoardGpu {
    fn capabilities(&self) -> GpuCapabilities {
        let caps = self.dev.capabilities();
        GpuCapabilities {
            status: STATUS_OK,
            max_contexts: u8::try_from(caps.max_contexts).unwrap_or(u8::MAX),
            features: caps.gpu_code_features,
            code_target: caps.gpu_code_target.0,
        }
    }

    fn open(&mut self, client: u64, prio: u8, library: &[u8]) -> ContextOpened {
        let client = ClientId(client);
        let (t0, heap0) = (now_ns(), nexus_service_entry::os::heap_cursor().1);
        // The client's old context goes whole first: a restart gets a fresh one.
        let replaced = match self.held.take() {
            Some(old) if old.client == client => {
                if let Err(e) = self.retire(old) {
                    emit_fmt(format_args!(
                        "gpud: FAIL gpu library (client=windowd the old context's teardown {e:?})"
                    ));
                }
                true
            }
            other => {
                self.held = other;
                false
            }
        };
        match self.make(client, priority(prio), library) {
            Ok((opened, t)) => {
                let ctx = opened.ctx;
                self.held = Some(opened);
                let open_us = now_ns().saturating_sub(t0) / 1000;
                let heap_bytes = nexus_service_entry::os::heap_cursor().1.saturating_sub(heap0);
                if t.refused == 0 {
                    emit_fmt(format_args!(
                        "gpud: gpu library ok (client=windowd ctx={:#x} prio={prio} bytes={} entries={} refused=0 code_bytes={} setup_bytes={} open_us={open_us} replaced={replaced} heap_bytes={heap_bytes})",
                        ctx.0,
                        library.len(),
                        t.accepted,
                        t.code_bytes,
                        t.setup_bytes
                    ));
                } else {
                    emit_fmt(format_args!(
                        "gpud: FAIL gpu library (client=windowd ctx={:#x} accepted={} refused={} first_refused={})",
                        ctx.0, t.accepted, t.refused, t.first_refused
                    ));
                }
                ContextOpened {
                    status: STATUS_OK,
                    ctx: ctx.0,
                    accepted: t.accepted,
                    refused: t.refused,
                    first_refused: t.first_refused,
                }
            }
            Err((step, e)) => {
                emit_fmt(format_args!(
                    "gpud: FAIL gpu library (client=windowd {step} {e:?} bytes={})",
                    library.len()
                ));
                match e {
                    Some(DeviceError::Library(_) | DeviceError::LibraryHeld) => {
                        refused(STATUS_REFUSED)
                    }
                    _ => refused(STATUS_DEVICE_ERROR),
                }
            }
        }
    }

    fn on_wake(&mut self) {
        self.dev.on_wake();
    }
}
