// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0252 host-first behavior proofs for HID, touch, keymaps, repeat, and accel.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 20 integration tests under `tests/input_v1_0_host/tests`.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

//! CONTEXT: TASK-0252 host-first behavior proofs for HID, touch, keymaps, repeat, and accel.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0252 proof floor
//! TEST_COVERAGE: Integration tests cover Soll vectors and reject paths under tests/input_v1_0_host/tests.
//! ADR: docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md

#![forbid(unsafe_code)]
