// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a resource's guest backing as a scatter-gather list (TASK-0286 P4a,
//! RFC-0098 C4). virtio-gpu `RESOURCE_ATTACH_BACKING` takes `nr_entries` memory
//! entries, so no backing needs to be physically contiguous: gpud's own backings,
//! windowd's framebuffer and the sub-ranges gpud aliases as textures (the atlas
//! rows, the display planes) are attached from the runs the kernel's one door for
//! physical addresses (`vmo_runs`) answers. The command must fit one ring slot, so
//! one attach carries at most `MAX_BACKING_RUNS` entries — a backing with more
//! runs is refused loudly (`gpud: resource runs fail`), never truncated.
//! OWNERS: @drivers
//! STATUS: Functional
//! TEST_COVERAGE: every QEMU boot (scanout, textures, atlas and blur aliases)

#![cfg(all(feature = "os-lite", target_os = "none"))]

use super::VirtioGpuBackend;
use crate::markers::GPUD_RESOURCE_RUNS_FAIL;
use crate::protocol::{VirtioGpuCtrlHdr, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking};
use core::mem::{offset_of, size_of};
use nexus_abi::DmaRun;
use nexus_gfx::backend::error::GfxError;

/// Memory entries one ATTACH_BACKING carries: the command fills one 4 KiB ring slot.
pub(crate) const MAX_BACKING_RUNS: usize =
    (4096 - size_of::<VirtioGpuResourceAttachBacking>()) / size_of::<VirtioGpuMemEntry>();

// The kernel's run IS the device's memory entry once its length fits 32 bits:
// the address at offset 0, the length's low word at offset 8 (little endian),
// the high word — 0 — where the entry's padding is. Proven here, not assumed.
const _: () = assert!(size_of::<DmaRun>() == size_of::<VirtioGpuMemEntry>());
const _: () = assert!(offset_of!(DmaRun, bus) == offset_of!(VirtioGpuMemEntry, addr));
const _: () = assert!(offset_of!(DmaRun, len) == offset_of!(VirtioGpuMemEntry, length));
const _: () = assert!(cfg!(target_endian = "little"));

impl VirtioGpuBackend {
    /// Attach `offset..offset + len` of the VMO in `vmo` as the backing of
    /// `resource_id`, one memory entry per run as the GPU addresses it; `hdr` is the
    /// command's header (the 2D control header or the virgl context's). Returns the
    /// entries. A run outside the GPU's DMA reach refuses the attach.
    pub(crate) fn attach_backing_runs(
        &mut self,
        hdr: VirtioGpuCtrlHdr,
        resource_id: u32,
        vmo: u32,
        offset: usize,
        len: usize,
    ) -> Result<usize, GfxError> {
        let mut runs = [DmaRun::default(); MAX_BACKING_RUNS];
        let count =
            nexus_abi::vmo_runs(vmo, self.device, offset, len, &mut runs).map_err(|err| {
                let _ = nexus_abi::debug_println(GPUD_RESOURCE_RUNS_FAIL);
                crate::diag::err_line(b"backing runs", err);
                crate::diag::kv_line(
                    b"backing runs",
                    &[
                        (b"res", u64::from(resource_id)),
                        (b"off", offset as u64),
                        (b"len", len as u64),
                    ],
                );
                GfxError::ResourceExhausted
            })?;
        let runs = &runs[..count];
        if runs.iter().any(|run| run.len > u64::from(u32::MAX)) {
            return Err(GfxError::InvalidArgument);
        }
        let attach = VirtioGpuResourceAttachBacking { hdr, resource_id, nr_entries: count as u32 };
        // SAFETY: `count` initialised runs, every length below 2^32, so their bytes
        // are exactly the device's memory entries (layout asserted above).
        let entries = unsafe {
            core::slice::from_raw_parts(runs.as_ptr().cast::<u8>(), count * size_of::<DmaRun>())
        };
        self.ctrl_submit_payload(&attach, entries)?;
        // The measurement behind RFC-0098 C4's claim (a large backing needs no
        // contiguous block): one line per backing of a megabyte or more.
        if len >= 1 << 20 {
            crate::diag::kv_line(
                b"backing runs",
                &[(b"res", u64::from(resource_id)), (b"len", len as u64), (b"runs", count as u64)],
            );
        }
        Ok(count)
    }
}
