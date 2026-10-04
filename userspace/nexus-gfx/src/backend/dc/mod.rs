// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The display-controller (`dc`) backend's host half (TASK-0250 P1–P3, D2 of the
//! hardware fast track): what the board's display controller and HDMI encoder need, as pure
//! logic over a register-writer trait, with goldens from the measurement of 2026-09-29
//! (`docs/board/measurements/2026-09-29-display-regs/`). Three parts:
//!
//! - [`edid`] — a bounded EDID 1.4 + CEA-861 parser (the monitor's bytes are untrusted input:
//!   every access is bounds-checked, every block checksummed, no `unwrap`) and `pick_mode`,
//!   the ONE policy: the highest progressive mode inside the SoC's maximum with the monitor's
//!   own aspect, 60 Hz preferred, the monitor's detailed timing preferred over the standard
//!   table for the same size (the stock system's mode line came from that timing).
//! - [`regs`] — the controller's register map as measured on the reference board: block bases
//!   and the word offsets of the first-light sequence (the boot loader's splash writes about
//!   thirty registers; the live dump at 1920x1080@60 agrees with them word for word).
//! - [`model`] — the bring-up, plane and flush sequence over [`model::RegWriter`]; the OS
//!   driver (D4) implements the trait over its MMIO window, the host tests over a recorder.
//! - [`encoder`] — the HDMI encoder's first-light sequence for each MEASURED pixel clock, as
//!   data (TASK-0251 P2a); an unmeasured clock is refused.
//!
//! No display MMU, no command list (the splash path needs neither; the vendor kernel's
//! contiguous-memory path is the same), one contiguous scanout plane addressed by its BUS
//! address — the address the kernel names for the controller's device (`vmo_dma_base`, the
//! tree's `dma-ranges`: bank 0 identical, bank 1 at bus 0x8000_0000). Everything here is
//! host-tested; nothing here touches hardware.
//! OWNERS: @gpu @runtime
//! STATUS: Functional (host half; the driver over it is TASK-0251 P2)
//! API_STABILITY: Internal (the register map is the board's; the `Mode`/`Plane` types are
//!   the contract to the driver)
//! TEST_COVERAGE: unit tests per module + `tests/dc_goldens.rs` against the archived EDID
//!   and the archived register dump

pub mod edid;
pub mod encoder;
pub mod model;
pub mod regs;

pub use edid::{cea_mode, parse_edid, pick_mode, Edid, EdidError, Mode, ModeList};
pub use model::{
    bring_up, damage_spans, flip, flush, DamageSpans, Plane, RegWriter, Sequence, Write,
};
