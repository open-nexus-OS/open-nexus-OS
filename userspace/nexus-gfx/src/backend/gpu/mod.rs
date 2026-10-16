// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU device's host half (TASK-0329 G1, RFC-0101 Phase 2 / §8b): everything the
//! thin side in gpud needs as PURE logic — the firmware image's documented layout, the MIPS
//! firmware processor's boot words and page table, the start/stop register sequences over a
//! recorder trait, the firmware interface structs with their sizes and offsets pinned, the
//! kernel/firmware/client command buffers as rings, and the GPU MMU's three-level page tables
//! as an encoder over a page allocator trait. Nothing here touches hardware; the device driver
//! (`gpud::backend::gpu`) implements the traits over its MMIO window and its uncached memory.
//!
//! The reading list is the open kernel driver's device logic (dual GPL/MIT; ported under the
//! MIT option, notices kept in `THIRD_PARTY_NOTICES`) and the published firmware's documented
//! host interface (version 1.1, build 6976702); the expected values are the stock system's
//! live registers (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`).
//!
//! - [`fw_image`] — the trailer (info header, layout table, device-info masks) and the ELF32
//!   program headers → placements into the code and data regions; refuses the wrong blob.
//! - [`mips_boot`] — the firmware heap's map, the boot-data words, the remap register pairs,
//!   the firmware page-table entries (goldens: the stock remap words).
//! - [`start`] — the start and stop sequences as recorded operations (goldens: the stock
//!   `SLC_CTRL_MISC`, `MIPS_WRAPPER_CONFIG`, the remap pairs).
//! - [`fwif`] — the interface structs (`SysInit`, `OsInit`, the command buffers' control
//!   words, the commands, the contexts), each with `const` size/offset asserts mirrored from
//!   the interface's own check headers; the command type values with their magic.
//! - [`ccb`] — the kernel command buffer (slots + return slots), the firmware-to-host buffer,
//!   the client command buffer (a byte ring whose commands never wrap).
//! - [`mmu`] — page catalogue / directory / table entries, the map/unmap over a page
//!   allocator trait, the flush flags a change accumulates.
//! - [`scene`] — the captured scene bundles (buffers, jobs, render trailer) the rungs replay.
//! - [`csb`] — the control-stream words the device writes (VDM, PPP, PDS state, the clip and
//!   empty-tile bits) from the open driver's word layouts.
//! - [`quads`] — the emitter: a pass of textured quads, each draw its library pipeline and its
//!   texture, every pass in fresh memory.
//! - [`eot`] — the end-of-tile program: a pass's target, as the two back-end state words the
//!   program loads (a copy per target).
//! - [`pipelines`] — a context's pipelines: the client's library parsed, checked and placed, a
//!   draw's pipeline found by id (RFC-0101 §7b).
//! - [`texstate`] — a texture's descriptor block: the image and sampler words of a stride
//!   texture.
//! OWNERS: @gpu @runtime
//! STATUS: Functional (host half; the driver over it is TASK-0329 P3–P5)
//! API_STABILITY: Internal (the device's; the contract above it is `nexus_driverkit::gpu`)
//! TEST_COVERAGE: unit tests per module + `tests/gpu_goldens.rs` over the published blob in
//!   `resources/firmware/gpu/` and the archived stock registers; `tests/gpu_quads_model.rs`
//!   (the emitter against the captures), `tests/gpu_pipelines.rs` (the table and its rejects)
//! INVARIANTS: a firmware image whose id, open-build flag or version differs is refused before
//!   a byte is placed; every placement lies inside its layout section; a GPU page-table entry
//!   is written only for a page-aligned, in-range address; a client command never wraps.

pub mod ccb;
pub mod csb;
pub mod eot;
pub mod fw_features;
pub mod fw_image;
pub mod fwif;
pub mod mips_boot;
pub mod mmu;
pub mod pipelines;
pub mod quads;
pub mod scene;
pub mod start;
pub mod texstate;

/// The GPU's id as the core register packs it: `b << 48 | v << 32 | n << 16 | c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CoreId(pub u64);

impl CoreId {
    /// From the four fields.
    pub const fn new(b: u16, v: u16, n: u16, c: u16) -> Self {
        Self(((b as u64) << 48) | ((v as u64) << 32) | ((n as u64) << 16) | c as u64)
    }

    /// The board's core (G0: `CORE_ID__PBVNC = 0x0024001D003400B6`).
    pub const BOARD: Self = Self::new(36, 29, 52, 182);

    pub const fn b(self) -> u16 {
        (self.0 >> 48) as u16
    }
    pub const fn v(self) -> u16 {
        (self.0 >> 32) as u16
    }
    pub const fn n(self) -> u16 {
        (self.0 >> 16) as u16
    }
    pub const fn c(self) -> u16 {
        self.0 as u16
    }

    /// Whether the packed id came from the PBVNC register (its branch field is non-zero;
    /// an older core reports zero there and its id lives in two 32-bit registers).
    pub const fn from_pbvnc_register(self) -> bool {
        self.b() != 0
    }
}

/// The fixed GPU virtual heaps (the firmware's interface fixes them; shader addresses in
/// command streams are offsets from the PDS and USC bases).
pub mod heaps {
    pub const GENERAL_BASE: u64 = 0x80_0000_0000;
    pub const GENERAL_SIZE: u64 = 128 << 30;
    pub const PDS_CODE_DATA_BASE: u64 = 0xDA_0000_0000;
    pub const PDS_CODE_DATA_SIZE: u64 = 4 << 30;
    pub const USC_CODE_BASE: u64 = 0xE0_0000_0000;
    pub const USC_CODE_SIZE: u64 = 4 << 30;
    /// Region headers of the render-target dataset (RFC-0101 §8d).
    pub const RGNHDR_BASE: u64 = 0xDB_F000_0000;
    pub const RGNHDR_SIZE: u64 = 256 << 20;
    /// Visibility-test results.
    pub const VIS_TEST_BASE: u64 = 0xF2_0000_0000;
    pub const VIS_TEST_SIZE: u64 = 2 << 20;
    pub const FIRMWARE_BASE: u64 = 0xE1_C000_0000;
    pub const TRANSFER_FRAG_BASE: u64 = 0xE4_0000_0000;
    pub const TRANSFER_FRAG_SIZE: u64 = 16 << 30;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_board_core_id_packs_as_the_register_shows() {
        assert_eq!(CoreId::BOARD.0, 0x0024001D003400B6);
        assert_eq!(
            (CoreId::BOARD.b(), CoreId::BOARD.v(), CoreId::BOARD.n(), CoreId::BOARD.c()),
            (36, 29, 52, 182)
        );
        assert!(CoreId::BOARD.from_pbvnc_register());
        assert!(!CoreId(0x0000_0001_0002_0003).from_pbvnc_register());
    }
}
