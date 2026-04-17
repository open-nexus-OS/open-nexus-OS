// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Aggregator-only module for per-service IPC clients used by the
//! selftest. Pre-P2-17 this file also hosted two generic "is this core
//! service answering?" probes (`core_service_probe`,
//! `core_service_probe_policyd`); P2-17 moves them to
//! `probes::core_service` so this file is a pure aggregator (no fn bodies).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU `just test-os` (full ladder).
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) mod bootctl;
pub(crate) mod bundlemgrd;
pub(crate) mod execd;
pub(crate) mod keystored;
pub(crate) mod logd;
pub(crate) mod metricsd;
pub(crate) mod policyd;
pub(crate) mod samgrd;
pub(crate) mod statefs;
