// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the driver's state lives on the service's stack (no heap in xhcid): its size is a
//! budget against `stack_pages` in `Cargo.toml` — two copies of the state during the move from
//! the bring-up's frame into the loop's, plus 16 KiB for every frame — not a surprise on the
//! board (TASK-0328 U3: the `usb` lane's xhcid died of a stack overflow at 8 pages after a
//! frame grew). Measured 21 696 bytes on the model's types with four TRBs per pipe
//! (2026-10-05), 25 816 with eight (cycle 12): 24 pages.
//! OWNERS: @runtime @drivers

use xhcid::Xhci;
use xhcid_model::{ModelAlloc, ModelBus};

/// `stack_pages` in `Cargo.toml`.
const STACK_PAGES: usize = 24;
const FRAMES: usize = 16 * 1024;

#[test]
fn the_driver_state_fits_the_stack_budget() {
    let xhci = core::mem::size_of::<Xhci<ModelBus, ModelAlloc>>();
    eprintln!("Xhci = {xhci} bytes");
    assert!(
        2 * xhci + FRAMES <= STACK_PAGES * 4096,
        "the driver state is {xhci} bytes: the stack budget ({STACK_PAGES} pages) must follow"
    );
}
