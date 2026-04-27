// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Host snapshot helpers for TASK-0054 renderer behavior proofs.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 24 ui_host_snap integration tests
//!
//! PUBLIC API:
//!   - Golden comparison/update helpers for canonical BGRA hex fixtures.
//!   - Deterministic PNG encode/decode helpers that ignore non-pixel metadata.
//!   - Codec helpers and common renderer fixture constructors.
//!
//! DEPENDENCIES:
//!   - `std`: filesystem, temp artifacts, and test error integration.
//!   - `ui_renderer`: renderer contract under test.
//!
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

pub mod codec;
pub mod fixtures;
pub mod golden;
pub mod png;

pub use codec::{bgra_to_rgba, hex_bytes, SnapResult, SnapshotError};
pub use fixtures::{artifact_root, make_damage, temp_artifact_path};
pub use golden::{
    compare_hex_golden, golden_root, resolve_under_root, update_hex_golden, GoldenMode,
};
pub use png::{decode_png_rgba, encode_png_rgba, insert_chunk_after_ihdr, DecodedPng};
