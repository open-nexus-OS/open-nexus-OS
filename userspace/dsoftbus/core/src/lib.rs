// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: no_std + alloc DSoftBus core crate for transport-neutral contracts and mux state machine
//! OWNERS: @runtime
//! STATUS: Functional (TASK-0022 extraction seam implemented; task-level closure is in review sync)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: userspace/dsoftbus/tests/core_contract_rejects.rs (8 host tests) + mux contract suites
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![no_std]
#![forbid(unsafe_code)]

#[path = "../../src/core_contract.rs"]
pub mod core_contract;

#[path = "../../src/mux_v2.rs"]
pub mod mux_v2;

pub use core_contract::{
    validate_payload_identity_spoof_vs_sender_service_id, validate_record_bounds,
    BorrowedFrameTransport, CoreReject, CorrelationNonce, CorrelationWindow, OwnedRecord,
    PayloadIdentityClaim, SenderServiceId, REJECT_INVALID_STATE_TRANSITION,
    REJECT_NONCE_MISMATCH_OR_STALE_REPLY, REJECT_OVERSIZE_FRAME_OR_RECORD,
    REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID, REJECT_UNAUTHENTICATED_MESSAGE_PATH,
};
pub use mux_v2::*;
