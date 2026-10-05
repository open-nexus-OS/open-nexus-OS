// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `xhcid` — the one owner of a USB host controller (TASK-0328 U1, RFC-0099). The
//! core is a pure, event-driven state machine over `nexus_hal::Bus` (the registers) and
//! [`DmaAlloc`] (the controller's memory, `nexus_driverkit::DmaShared`): [`Xhci::start`] brings
//! the controller up, [`Xhci::on_interrupt`] drains its event ring, [`Xhci::on_timer`] runs the
//! bounded waits, [`Xhci::deadline`] names the next one — nothing polls, nothing blocks,
//! nothing allocates on the heap. What happens goes to a [`Sink`] as [`Note`]s: the OS loop
//! (`os_lite`) prints the markers, the host tests record them against the behavioural model
//! (`xhcid-model`: the controller, a hub, HID devices, non-coherent DMA memory).
//! OWNERS: @runtime @drivers
//! STATUS: Functional (U1: QEMU's controller, hubs, HID boot interfaces; the board in U3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/xhci` against the model — bring-up, enumeration through a hub (the
//!   desk's measured descriptors, TT fields), the HID set-up incl. a STALLed SET_IDLE, reports
//!   end to end, detach, refused descriptors; QEMU: the `usb` lane's markers
//! INVARIANTS: descriptors are untrusted (bounded by `nexus-usb` before use); one port is reset
//!   and addressed at a time; a detached device's memory lives until Disable Slot completes;
//!   every TRB is published fields-first, every event observed before it is read.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod command;
pub mod context;
pub mod controller;
pub mod device;
mod enumerate;
pub mod hid_class;
mod hub;
pub mod memory;
mod pipe;
pub mod regs;
pub mod ring;
pub mod sink;
pub mod trb;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub mod os_lite;

pub use controller::{PortRef, Xhci};
pub use hid_class::{Channel, HidClass, Pushed};
pub use memory::{DmaAlloc, Region};
pub use sink::{Note, Sink, Step};
