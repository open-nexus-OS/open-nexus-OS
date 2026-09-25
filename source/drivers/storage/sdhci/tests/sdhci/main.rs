// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SDHCI core against a behavioural controller + eMMC model with an exact
//! non-coherent cache (TASK-0246 P2) — the host is the oracle before QEMU (P5) and the
//! board (P6) run it. `init`: power-up to HS52 on 4 and 8 bits and to HS400 enhanced strobe
//! on the K1, the command and vendor-register sequences as goldens, the HS400ES fallback,
//! determinism. `io`: ADMA2 through scattered buffers under the cache protocol, PIO, the
//! cache model's own honesty. `reject`: the `test_reject_*` matrix — timeouts, CRC and
//! ADMA errors, short transfers, R1 error bits and states, EXT_CSD, byte addressing,
//! refused switches, a corrupting bus, ranges, unreachable DMA memory, bad configs.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! TEST_COVERAGE: this file is the coverage

mod init;
mod io;
mod model;
mod reject;

use nexus_abi::DmaCoherence;
use nexus_driverkit::DmaBuffer;
use storage_sdhci::{Card, Ceiling, Disk};

use model::mem::{ModelCache, ModelMem};
use model::{ModelBus, Shared, SimPlatform};

pub type ModelCard = Card<ModelBus, SimPlatform>;
pub type ModelDisk = Disk<ModelBus, SimPlatform, ModelMem, ModelCache>;

/// The harts' cache block: every buffer is maintained (the K1's `soc` bus is non-coherent).
pub const NONCOHERENT: DmaCoherence = DmaCoherence::Maintained { block: 64 };

pub fn card(m: &Shared, ceiling: Ceiling) -> ModelCard {
    Card::init(model::host(m, true), ceiling).map_err(|f| f.error).expect("init")
}

pub fn buffer(m: &Shared, mem: ModelMem) -> DmaBuffer<ModelMem, ModelCache> {
    DmaBuffer::new(mem, NONCOHERENT, ModelCache(m.clone())).expect("buffer")
}

/// A disk with a one-page table at 1 GiB and a 64 KiB bounce buffer in four scattered runs.
pub fn disk(m: &Shared, card: ModelCard) -> ModelDisk {
    let table = buffer(m, ModelMem::new(m, 4096, 0x4000_0000, 1, 0));
    let bounce = buffer(m, ModelMem::new(m, 64 * 1024, 0x4800_0000, 4, 1));
    Disk::new(card, table, bounce).expect("disk")
}
