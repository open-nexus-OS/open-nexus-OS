//! TASK-0023B P2-14: aggregator for the `updated` selftest helpers.
//!
//! Pre-split this file held all OTA helper functions (~450 LOC). It now
//! re-exports the same `pub(crate)` surface from focused submodules so all
//! call-sites (`os_lite::updated::*`) keep working unchanged:
//!
//!   * [`types`]      -- shared constants (`SYSTEM_TEST_NXS`) and `SlotId`.
//!   * [`reply_pump`] -- shared `updated_send_with_reply` / `updated_expect_status`.
//!   * [`stage`]      -- `updated_stage` + `updated_log_probe`.
//!   * [`switch`]     -- `updated_switch`.
//!   * [`status`]     -- `updated_get_status` + `updated_boot_attempt`.
//!   * [`health`]     -- `init_health_ok`.
//!
//! Behavior, marker emissions, and timing budgets are byte-for-byte identical
//! to the pre-split module; only file layout changed.

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
