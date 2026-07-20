// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Declarative SSOT for service↔service wire frames (ADR-0051)
//! OWNERS: @runtime
//! PUBLIC API: codec::{Writer, Reader, put_hdr, check_hdr, request_op}, frames! DSL,
//!             per-protocol modules (execd, updated, routing, bundlemgrd, sessiond,
//!             settingsd, bundleimg, policy, policyd)
//! DEPENDS_ON: nothing (no_std, alloc-free, zero deps)
//! INVARIANTS: all scalar fields little-endian; decoders are fail-closed (`None` on
//!             malformed input, exact-length by default); wire bytes are locked by
//!             golden-byte tests — a declaration change that alters bytes is a
//!             protocol change and needs the consumers updated in the same commit
//! ADR: docs/adr/0051-declarative-wire-codec-nexus-wire.md (mechanism),
//!      docs/adr/0038-display-wire-ssot-and-capnp-boundary.md (why not capnp here)

pub mod bundleimg;
pub mod bundlemgrd;
pub mod codec;
pub mod execd;
mod frames;
pub mod policy;
pub mod policyd;
pub mod routing;
pub mod sessiond;
pub mod settingsd;
pub mod updated;
