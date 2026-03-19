// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus remote gateway entry points and protocol surfaces
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host reject tests + QEMU 2-VM remote proxy markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod local_ipc;
pub(crate) mod packagefs_ro;
pub(crate) mod remote_proxy;
