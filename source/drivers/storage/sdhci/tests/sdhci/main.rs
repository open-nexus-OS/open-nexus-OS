// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SDHCI core against a behavioural controller + eMMC model with an exact
//! non-coherent cache (TASK-0246 P2; the model is `storage-sdhci-model` since P4b, so the
//! block owner and the boot loader are proven against the same machine) — the host is the
//! oracle before QEMU (P5) and the
//! board (P6) run it. `init`: power-up to HS52 on 4 and 8 bits and to HS400 enhanced strobe
//! on the K1, the command and vendor-register sequences as goldens, the HS400ES fallback,
//! determinism. `io`: ADMA2 through scattered buffers under the cache protocol, PIO, the
//! cache model's own honesty. `pio`: the boot loader's path — PIO writes and reads on a polling
//! host, their refusals. `reject`: the `test_reject_*` matrix — timeouts, CRC and
//! ADMA errors, short transfers, R1 error bits and states, EXT_CSD, byte addressing,
//! refused switches, a corrupting bus, ranges, unreachable DMA memory, bad configs.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! TEST_COVERAGE: this file is the coverage

mod init;
mod io;
mod pio;
mod reject;

// The machine is its own crate (P4b): the block owner's adapter and the boot loader's reader
// are proven against the same one.
use storage_sdhci_model as model;

pub use model::{buffer, card, disk, ModelCard, ModelDisk, NONCOHERENT};
