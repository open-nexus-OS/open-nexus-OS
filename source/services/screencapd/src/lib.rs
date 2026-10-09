// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `screencapd` — the ONE screen-capture facade (RFC-0095, ADR-0071, TASK-0068).
//! The shell's screenshot UI (`svc.screencap`) begins a capture, picks a selection, the
//! screen or a window, and shoots; Shift+Print and Alt+Print shoot without the UI. Nothing
//! here draws or reads pixels on its own: windowd freezes the screen on a frame gpud reads
//! into this service's frame VMO (lent to windowd once at start), and this service crops that
//! frame, draws the pointer when asked, encodes a PNG into a VMO sized for the worst case and
//! hands it to vfsd, which writes the file in one transaction.
//!
//! - [`plan`]: which rectangle a shoot saves, how a stem becomes a file name (pure).
//! - [`pointer`]: the pointer sprite over a pulled row (pure).
//! - `os_lite`: the service loop and its legs to windowd and vfsd (OS target).
//!
//! WHO MAY DO WHAT: the route is the capability — app children hold it only with
//! `nexus.permission.SCREENCAP`, which the packer grants to the `shell` and `settings`
//! bundle types alone; the harness holds a declared edge. windowd accepts the capture verb
//! from this service's kernel sid only and refuses a freeze while the greeter owns the
//! display. Markers carry kinds and sizes, never pixels or file names.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the wire is `nexus_wire::screencapd`)
//! TEST_COVERAGE: tests/contract.rs — rectangles, names, the pointer blend, `test_reject_*`
//! RFC: docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md

#![cfg_attr(not(any(test, nexus_env = "host")), no_std)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

pub mod plan;
pub mod pointer;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub mod os_lite;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod save_os;
