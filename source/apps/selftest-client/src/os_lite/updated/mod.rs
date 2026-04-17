// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Aggregator for the `updated` (OTA) selftest helpers — re-exports
//!   the same `pub(crate)` surface (`init_health_ok`, `updated_stage`,
//!   `updated_log_probe`, `updated_switch`, `updated_get_status`,
//!   `updated_boot_attempt`, `SlotId`) from focused submodules.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! Sub-split landed in TASK-0023B Cut P2-14:
//!   * [`types`]      -- shared constants (`SYSTEM_TEST_NXS`) and `SlotId`.
//!   * [`reply_pump`] -- shared `updated_send_with_reply` / `updated_expect_status`.
//!   * [`stage`]      -- `updated_stage` + `updated_log_probe`.
//!   * [`switch`]     -- `updated_switch`.
//!   * [`status`]     -- `updated_get_status` + `updated_boot_attempt`.
//!   * [`health`]     -- `init_health_ok`.
//!
//! Behavior, marker emissions, and timing budgets are byte-for-byte identical
//! to the pre-split module; only file layout changed.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

mod health;
mod reply_pump;
mod stage;
mod status;
mod switch;
mod types;

pub(crate) use health::init_health_ok;
pub(crate) use stage::{updated_log_probe, updated_stage};
pub(crate) use status::{updated_boot_attempt, updated_get_status};
pub(crate) use switch::updated_switch;
pub(crate) use types::SlotId;
