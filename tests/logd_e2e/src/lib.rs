// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: [Test harness] for logd journal end-to-end testing
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: Refer to tests/journal_roundtrip.rs
//!
//! TEST_SCOPE:
//!   - Observability integration testing with logd journal and crash reports
//!   - Journal roundtrip, overflow behavior, crash reports, query pagination
//!
//! DEPENDENCIES:
//!   - logd (service integration)
//!
//! ADR: docs/adr/0017-service-architecture.md

#![cfg(nexus_env = "host")]
#![forbid(unsafe_code)]
