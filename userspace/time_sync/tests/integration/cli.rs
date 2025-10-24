// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for time sync CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Time offset application
//!   - CLI argument parsing
//!   - Clock synchronization operations
//!
//! TEST_SCENARIOS:
//!   - test_offset_parsing(): Test time offset parsing and application
//!
//! DEPENDENCIES:
//!   - time_sync::execute: CLI execution function
//!   - Numeric parsing for offset values
//!
//! ADR: docs/adr/0012-time-sync-architecture.md
