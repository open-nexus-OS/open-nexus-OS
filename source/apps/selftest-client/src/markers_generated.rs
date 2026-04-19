// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Marker-literal SSOT for selftest-client emit sites.
//!
//! `OUT_DIR/markers_generated.rs` is produced by `build.rs` (P4-03) from
//! `source/apps/selftest-client/proof-manifest.toml`. It holds one
//! `pub(crate) const M_<KEY>: &str = "<literal>"` per declared marker.
//! P4-04 migrated every emit site (host + OS slice) to reference these
//! constants; literals MUST NOT survive outside this module + `markers.rs`.
//! `arch-gate` Rule 3 mechanically enforces that invariant.
//!
//! This module is built unconditionally so the host-pfad (`host_lite.rs`)
//! and the OS-pfad (`os_lite/**`) consume the same SSOT.
//!
//! OWNERS: @runtime
//! STATUS: Functional (host + OS, P4-04+)
//! API_STABILITY: Generated; treat literals as the contract surface.
//! TEST_COVERAGE: QEMU ladder (`just test-os`) + host slice (cargo).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/markers_generated.rs"));
