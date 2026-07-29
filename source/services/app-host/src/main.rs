// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host — the DSL app runtime process (TASK-0080D). Spawned by
//! execd (not a boot service), it validates + mounts a compiled `.nxir`
//! program with the SAME interpreter windowd's demo mount uses, lays the
//! scene out, renders it into its OWN surface VMO and presents through
//! windowd's client-surface wire (ADR-0042: `SURFACE_CREATE` moves the VMO
//! capability, presents are strictly sequenced). R2a: payload embedded at
//! build time (bundle GET_PAYLOAD replaces the byte source, not this code);
//! scene fills only — text lands with the shared text/painter promotion
//! (RFC-0067 P5). Falls back to the R1 solid-fill probe if the mount fails
//! (fail-closed, visibly).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: wire codecs host-tested in nexus-display-proto; the probe
//! itself is proven via QEMU markers (`APPHOST: …`).
//! ADR: docs/adr/0042-cross-process-surface-transport.md

// RFC-0080: installing the shared-atlas base is one raw mapped-VMO pointer →
// `unsafe`. `deny` (not `forbid`) so ONLY `map_atlas_base` may opt in with a
// documented safety contract; everything else stays unsafe-free.
#![deny(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> Result<(), &'static str> {
    probe::run()
}

#[cfg(nexus_env = "host")]
fn main() {
    println!("app-host: host mode - the probe runs on the OS (QEMU markers)");
}

// The DSL `EffectHost` over execd-provisioned fixed slots (TASK-0080C #16).
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod effect_files;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod effect_host;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod effect_ime;
// The `svc.files` listing filter (RFC-0084 Phase 6). Compiled for the OS build
// — where `effect_host` calls it — and for host TEST builds, where its unit
// tests exercise it. A plain host build compiles it nowhere, so it is never
// dead code: the CLAUDE.md rule for a one-cfg item, instead of a blanket
// `allow(dead_code)`.
#[cfg(any(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), test))]
mod file_filter;
// Where the paint-time hover wash goes. Same one-cfg shape as `file_filter`:
// `probe/` is RISC-V-only, so a pure decision that belongs to it lives here
// instead, where host tests can actually reach it.
#[cfg(any(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), test))]
mod hover_wash;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod svc_call;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod time_client;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod probe;
