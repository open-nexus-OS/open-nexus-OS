// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot payload the `nx image` CLI tests write as their kernel (ADR-0081): every
//! disk assembler takes only a container — a kernel item of `kernel_len` deterministic bytes
//! salted by `salt`, a footprint with room for a build trailer, and a one-file bootfs.
//! OWNERS: @tools-team

/// A container of `kernel_len` kernel bytes (deterministic, `salt` makes a second build).
pub fn payload(kernel_len: usize, salt: u8) -> Vec<u8> {
    let kernel: Vec<u8> = (0..kernel_len as u32).map(|i| (i % 251) as u8 ^ salt).collect();
    let mut files = nexus_bootfs::toc::Builder::new();
    files.add("init-lite", &[0x5A ^ salt; 3000], 16, 0x1800).expect("fixture file");
    let bootfs = files.finish().expect("fixture bootfs");
    let footprint = kernel_len as u64 + 64 * 1024;
    nexus_bootfs::payload::compose(&kernel, footprint, &bootfs).expect("fixture payload")
}
