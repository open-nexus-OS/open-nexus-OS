// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: DslMountContract — simulated windowd DSL-mount stage (TASK-0076B)
//! for integration chain tests.
//! OWNERS: @tools-team @ui
//!
//! Encodes the mount contract AND its budget failure modes as observable hop
//! markers — the machine-readable record of the 2026-07-06 boot-hang debug:
//!
//! 1. The mount runs at the **present-visible milestone** (never in the frame
//!    loop: the compositor is reactive, per-frame retries starve without
//!    damage) — so `windowd: present visible ok` strictly precedes every
//!    `DSL:` marker.
//! 2. The window needs `2 × DSL_WIN_H` atlas rows (content + blur bands). A
//!    pool short of that MUST fail with the value-carrying marker
//!    `windowd: dsl open FAIL atlas (need=WxH rows_remaining=N)` — silent
//!    denial is a contract violation.
//! 3. A validation failure keeps the window closed with an honest
//!    `DSL: program mount FAILED (validation)` — never a partial mount.

use crate::chain::contract::{Contract, ContractError};
use crate::chain::{ServiceId, SimIpcBus};

/// The DSL demo window's atlas demand (mirrors
/// `windowd/src/compositor/runtime/dsl_mount.rs`).
pub const DSL_WIN_W: u32 = 300;
pub const DSL_WIN_H: u32 = 220;
/// Content + blur bands.
pub const DSL_ATLAS_ROWS_NEEDED: u32 = 2 * DSL_WIN_H;

/// Simulated windowd DSL-mount stage.
pub struct DslMountContract {
    id: Option<ServiceId>,
    /// Rows the on-demand window pool has left at the milestone.
    pool_rows: u32,
    /// The embedded program validates (hash/structure).
    program_valid: bool,
    /// Simulate a live pointer tap after the first frame.
    with_interaction: bool,
}

impl DslMountContract {
    /// Healthy boot: pool reserved per `WINDOW_POOL_ROWS`, valid program.
    pub fn healthy() -> Self {
        Self {
            id: None,
            pool_rows: DSL_ATLAS_ROWS_NEEDED + 16,
            program_valid: true,
            with_interaction: true,
        }
    }

    /// The 2026-07-06 failure: pool under-reserved at the milestone.
    pub fn pool_starved(pool_rows: u32) -> Self {
        Self { id: None, pool_rows, program_valid: true, with_interaction: false }
    }

    /// Tampered/incompatible payload: must fail closed, no window.
    pub fn invalid_program() -> Self {
        Self {
            id: None,
            pool_rows: DSL_ATLAS_ROWS_NEEDED + 16,
            program_valid: false,
            with_interaction: false,
        }
    }
}

impl Contract for DslMountContract {
    fn service_name(&self) -> &'static str {
        "windowd-dsl"
    }

    fn set_service_id(&mut self, id: ServiceId) {
        self.id = Some(id);
    }

    fn run(&mut self, bus: &mut SimIpcBus) -> Result<(), ContractError> {
        let id = self
            .id
            .ok_or_else(|| ContractError::new(ServiceId(0), "dsl-mount: service id not set"))?;

        // Hop 0 — the milestone that triggers the mount (contract rule 1).
        bus.emit_marker(id, "windowd: present visible ok");

        // Hop 1 — fail-closed program validation.
        if !self.program_valid {
            bus.emit_marker(id, "DSL: program mount FAILED (validation)");
            return Ok(()); // window stays closed; chain asserts no frame marker
        }
        bus.emit_marker(id, "DSL: program loaded hash=0cc78eff5a933b77");

        // Hop 2 — atlas budget (contract rule 2: value-carrying denial).
        if self.pool_rows < DSL_ATLAS_ROWS_NEEDED {
            bus.emit_marker(
                id,
                &format!(
                    "windowd: dsl open FAIL atlas (need={DSL_WIN_W}x{DSL_WIN_H} rows_remaining={})",
                    self.pool_rows
                ),
            );
            return Ok(());
        }

        // Hop 3 — first interpreter frame reaches the surface.
        bus.emit_marker(id, "DSL: first frame presented");

        // Hop 4 — a live tap routed through the interpreter's hit-testing.
        if self.with_interaction {
            bus.emit_marker(id, "DSL: interaction visible ok");
        }
        Ok(())
    }
}
