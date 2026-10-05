// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The USB host controller service's (`xhcid`) declared slots (TASK-0328 U1,
//! RFC-0099). Its controller window is the fleet-wide `DEVICE_MMIO_SLOT`; its two endpoints
//! are what the reactive loop waits on — the controller's line and the one-shot of its bounded
//! waits. High slots on purpose (as blkd's): the DMA objects xhcid makes take the lowest free
//! slots, and init pins these before xhcid runs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal — the numbers are the contract between init and xhcid

use super::SlotPair;

/// RECV half of the controller's interrupt notify endpoint (`irq_bind` target).
pub const IRQ_NOTIFY: u32 = 0xF3;
/// The one-shot of the bounded waits (a controller reset step, a port's reset or power-good,
/// a command's deadline) — a waitset member beside the interrupt endpoint.
pub const TIMER: SlotPair = SlotPair::new(0xF5, 0xF4);
