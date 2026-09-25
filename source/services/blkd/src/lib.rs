// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `blkd` — the ONE block owner (ADR-0044/0067, TASK-0246): it opens the disk init
//! granted, parses its GPT once and serves partition-scoped `blockproto` over IPC. The pure
//! parts live here so the host proves them: `backend`, which backend runs the granted device
//! (the loader's boot-disk record, checked against the window), and `gate`, the
//! deny-by-default partition gate on the kernel-attributed sender id. The service loop is
//! `os_lite.rs` in the binary.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/backend.rs` (every kind from its record, the window check, the
//!   direct-kernel stand-in, the refusals), `tests/gate.rs` (the whole sender × partition × op
//!   matrix); QEMU: `blkd: backend=… ready`, `blkd: gpt ok (parts=7)`, the block-plane
//!   selftests, the cross-partition deny probes
//! ADR: docs/adr/0067-one-block-owner-backend-selected-by-fdt.md

#![no_std]
#![forbid(unsafe_code)]

pub mod backend;
pub mod gate;
