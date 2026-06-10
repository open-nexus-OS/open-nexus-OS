// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Session module root — organizes submodules for dsoftbusd session lifecycle: FSM, handshake, QUIC frames, records, selftest server, single-VM runner, and orchestration steps.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! Session lifecycle helpers (FSM, handshake seeds, record constants).

pub(crate) mod cross_vm;
pub(crate) mod fsm;
pub(crate) mod handshake;
pub(crate) mod quic_frame;
pub(crate) mod records;
pub(crate) mod selftest_server;
pub(crate) mod single_vm;
pub(crate) mod steps;
