// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! External-framebuffer attach: windowd's shared VMO becomes the GPU scanout
//! backing (2D resource on the non-virgl path, virgl GL scanout init + warmup
//! on the GL path). Split out of `backend/present.rs` (structure-gate).

use super::{ResourceRecord, VirtioGpuBackend};
use nexus_gfx::backend::error::GfxError;
#[allow(unused_imports)]
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::core::types::PixelFormat;

#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::transport::{align_page, ctrl_hdr, DISPLAY_PLANE_HEIGHT, DISPLAY_PLANE_ROW};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::markers::GPUD_RESOURCE_VMO_MAP_FAIL;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

impl VirtioGpuBackend {
    /// Attach an externally-owned VMO as the display scanout backing.
    /// Zero-copy: the same VMO that windowd composes into becomes the GPU scanout.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn attach_external_framebuffer(
        &mut self,
        vmo_slot: u32,
        width: u32,
        height: u32,
    ) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        // windowd's framebuffer replaces the bootstrap scanout — stop the 2D
        // splash pulse (the GL hold phase takes the breathing over from here)
        // and remember the dead splash resource: its 4MB one-shot backing goes
        // back to the kernel arena once the new scanout is live (task #124).
        let dead_splash = if self.bootstrap_splash_live { self.scanout_resource } else { None };
        self.bootstrap_splash_live = false;
        let mut info = nexus_abi::CapQuery::default();
        nexus_abi::cap_query(vmo_slot, &mut info).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: ERROR cap_query failed");
            let _ = e;
            GfxError::MmioFault
        })?;
        if info.kind_tag != 1 {
            let _ = nexus_abi::debug_println("gpud: ERROR attach: kind_tag != VMO");
            return Err(GfxError::InvalidArgument);
        }
        if info.len < (width as u64 * height as u64 * 4) {
            let _ = nexus_abi::debug_println("gpud: ERROR attach: VMO too small");
            return Err(GfxError::InvalidArgument);
        }
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;

        // Map the external VMO into gpud's VA space for direct framebuffer
        // write access. RFC-0085: ONE `vm_map` for the whole range at a
        // KERNEL-CHOSEN va — this replaces the ~12 000-syscall per-page loop
        // whose moving mid-loop EINVAL was TASK-0309's Heisenbug habitat.
        // Interior 2 MiB spans promote to superpage leaves kernel-side.
        let backing_len_aligned = align_page((width * height * 4) as usize);
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        let backing_va =
            nexus_abi::vm_map(vmo_slot, 0, backing_len_aligned, flags).map_err(|err| {
                let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_MAP_FAIL);
                crate::diag::err_line(b"attach map", err);
                crate::diag::kv_line(
                    b"attach map",
                    &[
                        (b"slot", u64::from(vmo_slot)),
                        (b"len", backing_len_aligned as u64),
                        (b"vmolen", info.len),
                    ],
                );
                GfxError::MmioFault
            })?;

        // The VMO needs a virtio 2D resource ONLY for the non-virgl 2D scanout
        // path. On the virgl path the VMO is read solely as the 3D texture 0xF8
        // (an alias of the same physical pages); creating a 2D resource on that
        // same memory is the "mixing 3D rendering and 2D scanout on one resource"
        // that blacks out the gl device (see the comment below). So skip it when
        // virgl drives the scanout — leaving 0xF8 (3D) as the sole resource on
        // that memory. The non-virgl/mmio build keeps the clean 2D path.
        // TASK-0324 P0: the path is a POLICY decision (`scanout_policy`), the
        // same one `probe` already enforced — a GL device is never scanned out
        // through the 2D plane-row window.
        #[cfg(feature = "virgl")]
        let draw_ok = self.virgl_draw_ok;
        #[cfg(not(feature = "virgl"))]
        let draw_ok = false;
        // TASK-0326: GL is NEEDED from here on, and the display is live — so this
        // is where an UNPROVEN draw is settled. `probe()` only recorded whether
        // it had been proven yet, and on a backend whose window is realized
        // asynchronously it often had not been; that says nothing about the
        // DEVICE. The proof here is the real work, not a second self-test: the
        // cascade's test objects sit on fixed ids and re-running it collides
        // with itself (measured: `virgl rt clear fail` on the second pass).
        let unproven = super::scanout_policy::draw_verdict(self.gl_device, draw_ok, true)
            == super::scanout_policy::DrawVerdict::Fatal;
        let use_virgl_scanout = if unproven {
            true // attempt it; `gl_scanout_init`'s error path is the loud failure
        } else {
            match super::scanout_policy::scanout_path(
                self.gl_device,
                draw_ok,
                cfg!(feature = "virgl"),
            ) {
                Ok(super::scanout_policy::ScanoutPath::GlRenderTarget) => true,
                Ok(super::scanout_policy::ScanoutPath::PlaneRow2d) => false,
                Err(_) => return Err(GfxError::Unsupported),
            }
        };
        if !use_virgl_scanout {
            let create = protocol::VirtioGpuResourceCreate2d {
                hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
                resource_id: id.0,
                format: Self::to_gpu_format(PixelFormat::Bgra8888),
                width,
                height,
            };
            self.ctrl_submit_struct(&create).map_err(|_| GfxError::CommandRejected)?;

            // windowd's framebuffer is an anonymous object: attach its runs.
            let hdr = ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
            self.attach_backing_runs(hdr, id.0, vmo_slot, 0, (width * height * 4) as usize)?;
        }

        // GL-presented scanout (G0/G1): on a virgl-capable device the display
        // is a GPU render target and presents are GPU blits of this VMO — the
        // 2D SET_SCANOUT/transfer/flush below stays the non-virgl path. Mixing
        // 3D rendering and 2D scanout on one resource is what blacked out the
        // gl device.
        #[cfg(feature = "virgl")]
        {
            let record = ResourceRecord {
                id,
                width,
                height,
                format: PixelFormat::Bgra8888,
                backing_va,
                dma_vmo: vmo_slot,
                backing_len: (width * height * 4) as usize,
                backing_vmo: 0,
                backing_map_len: backing_len_aligned,
            };
            if use_virgl_scanout {
                self.resources.push(record);
                self.scanout_resource = Some(id);
                match self.gl_scanout_init() {
                    Ok(()) => {
                        if unproven {
                            // The device just did it. That is the proof.
                            #[cfg(feature = "virgl")]
                            {
                                self.virgl_draw_ok = true;
                            }
                            let _ =
                                nexus_abi::debug_println(crate::markers::GPUD_GL_DRAW_PROVEN_LATE);
                        }
                        let _ = nexus_abi::trace_line("gpud: set_scanout ok");
                        let _ = nexus_abi::trace_line("gpud: scanout ok");
                        let _ = nexus_abi::trace_line("gpud: scanout bgra8888");
                        // Absorb the one-time virgl texture-sampling stall (~500ms)
                        // HERE, at boot, so the user's first present/scroll is fast
                        // instead of frozen for half a second.
                        let _ = self.gl_pipeline_warmup();
                        self.gl_present_damage(Rect {
                            x: 0,
                            y: 0,
                            width,
                            height: DISPLAY_PLANE_HEIGHT,
                        })?;
                        let _ = nexus_abi::trace_line("gpud: transfer_to_host ok");
                        let _ = nexus_abi::trace_line("gpud: resource flush ok");
                        // GL scanout is live — the bootstrap splash resource
                        // and its 4MB backing are dead weight now (task #124).
                        if let Some(dead) = dead_splash {
                            self.release_resource(dead);
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        // Name the failing class at the source (RFC-0066 clean
                        // errors): a silent fallback cost a debug cycle once.
                        let _ = nexus_abi::debug_println(match e {
                            GfxError::DeviceNotFound => "gpud: gl init err device-not-found",
                            GfxError::MmioFault => "gpud: gl init err mmio-fault",
                            GfxError::CommandRejected => "gpud: gl init err command-rejected",
                            GfxError::ResourceExhausted => "gpud: gl init err resource-exhausted",
                            GfxError::Unsupported => "gpud: gl init err unsupported",
                            GfxError::InvalidArgument => "gpud: gl init err invalid-argument",
                        });
                        // A GL device has no other path (scanout_policy):
                        // fail loudly, the service exits, supervision restarts
                        // it. The 2D retry that used to live here showed BLACK
                        // on every GL display backend (2026-09-09).
                        let _ = nexus_abi::debug_println(crate::markers::GPUD_FAIL_GL_SCANOUT_INIT);
                        self.resources.pop();
                        self.scanout_resource = None;
                        return Err(e);
                    }
                }
            }
        }

        // Activate the scanout first, then commit the initial framebuffer contents.
        // This matches the visible-bootstrap contract more closely: QEMU first learns
        // the target mode/scanout, then receives the content transfer + flush.
        // Phase 3: scanout displays frame ring slot A (rows 1600..2399) in 4-plane VMO.
        let scanout = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: DISPLAY_PLANE_ROW,
                width,
                height: DISPLAY_PLANE_HEIGHT,
            },
            scanout_id: 0,
            resource_id: id.0,
        };
        self.ctrl_submit_struct(&scanout).map_err(|_| GfxError::CommandRejected)?;
        let _ = nexus_abi::trace_line("gpud: set_scanout ok");
        let _ = nexus_abi::trace_line("gpud: scanout ok");
        let _ = nexus_abi::trace_line("gpud: scanout bgra8888");

        let record = ResourceRecord {
            id,
            width,
            height,
            format: PixelFormat::Bgra8888,
            backing_va,
            dma_vmo: vmo_slot,
            backing_len: (width * height * 4) as usize,
            backing_vmo: 0, // external VMO from windowd (cap lifetime independent)
            backing_map_len: backing_len_aligned,
        };

        // Transfer the display plane (rows 1600..2399) to host for the first frame.
        self.transfer_to_host_os(
            record,
            Rect { x: 0, y: DISPLAY_PLANE_ROW, width, height: DISPLAY_PLANE_HEIGHT },
        )
        .map_err(|e| {
            let _ =
                nexus_abi::debug_println("gpud: ERROR transfer_to_host for initial frame failed");
            e
        })?;
        let _ = nexus_abi::trace_line("gpud: transfer_to_host ok");

        let flush = protocol::VirtioGpuResourceFlush {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: DISPLAY_PLANE_ROW,
                width,
                height: DISPLAY_PLANE_HEIGHT,
            },
            resource_id: id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&flush).map_err(|_| {
            let _ = nexus_abi::debug_println("gpud: ERROR resource flush failed");
            GfxError::CommandRejected
        })?;
        let _ = nexus_abi::trace_line("gpud: resource flush ok");

        self.resources.push(record);
        self.scanout_resource = Some(id);
        // The 2D scanout switched to windowd's framebuffer — free the dead
        // bootstrap splash resource + its one-shot backing (task #124).
        if let Some(dead) = dead_splash {
            self.release_resource(dead);
        }
        Ok(())
    }
}
