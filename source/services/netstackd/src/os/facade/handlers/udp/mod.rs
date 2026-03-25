// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OP_UDP_* handler module split (bind/send_to/recv_from)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

mod bind;
mod recv_from;
mod send_to;

pub(crate) use bind::handle_bind;
pub(crate) use recv_from::handle_recv_from;
pub(crate) use send_to::handle_send_to;
