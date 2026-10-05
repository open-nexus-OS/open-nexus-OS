// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The USB host controller service's (`xhcid`) declared slots (TASK-0328 U1,
//! RFC-0099). Its controller window is the fleet-wide `DEVICE_MMIO_SLOT`; its three wait
//! endpoints are what the reactive loop waits on — the controller's line, the one-shot of its
//! bounded waits and, since TASK-0253B, its server endpoint, where a class client subscribes
//! (RFC-0099 §5). High slots on purpose (as blkd's): the DMA objects xhcid makes take the
//! lowest free slots, and init pins these before xhcid runs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal — the numbers are the contract between init and xhcid

use super::SlotPair;

/// xhcid's own server endpoint: a class client's SUBSCRIBE arrives here, with the SEND half
/// of the client's push channel moved along.
pub const SERVER: SlotPair = crate::SERVER_SLOTS;
/// RECV half of the controller's interrupt notify endpoint (`irq_bind` target).
pub const IRQ_NOTIFY: u32 = 0xF3;
/// The one-shot of the bounded waits (a controller reset step, a port's reset or power-good,
/// a command's deadline, a class frame owed to a full client) — a waitset member beside the
/// interrupt endpoint.
pub const TIMER: SlotPair = SlotPair::new(0xF5, 0xF4);
/// The CAP_MOVE reply inbox of its one outbound call: policyd's verdict on a subscriber.
pub const REPLY: SlotPair = SlotPair::new(0xF8, 0xF7);
/// Delegated policy checks: does the subscriber (kernel-attributed) hold the class's
/// capability (`usb.hid`)?
pub const POLICYD: SlotPair = SlotPair::new(0xF9, REPLY.recv);
