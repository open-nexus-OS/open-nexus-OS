// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Netstackd IPC facade OP_* handler modules (split from runtime loop)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod accept;
pub(crate) mod close;
pub(crate) mod connect;
pub(crate) mod listen;
pub(crate) mod local_addr;
pub(crate) mod ping;
pub(crate) mod read;
pub(crate) mod udp;
pub(crate) mod wait_writable;
pub(crate) mod write;
