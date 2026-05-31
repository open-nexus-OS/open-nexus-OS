// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Observer subsystem — marker-reader, telemetry poller, liveness checker.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
//!
//! After RFC-0061 M4, selftest-client is a pure observer. It never initiates
//! service IPC — it only reads markers from logd, polls display telemetry,
//! and checks samgrd for service liveness.

pub(crate) mod liveness;
pub(crate) mod markers;
pub(crate) mod telemetry;
