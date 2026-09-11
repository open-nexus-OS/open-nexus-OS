// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel RNG entropy probe (TASK-0006). Exercises the rngd entropy
//!   request path with bounded payloads and an oversized-request reject, over the
//!   selftest client's one rngd exchange (`services::rngd` — it WAITS for the reply;
//!   the two hand-copied polling loops that lived here are gone).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase consumes
//!   `rng_entropy_selftest`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::services::rngd::{self, RngdError};

/// `GET_ENTROPY` for 32 bytes must succeed with exactly 32 bytes.
pub(crate) fn rng_entropy_selftest() {
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0xA5A5_5A5A;
    let client = match rngd::client() {
        Ok(c) => c,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_NO_SLOTS);
            return;
        }
    };
    emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_SEND);
    match rngd::get_entropy(&client, 32, nonce) {
        Err(RngdError::NoSlots) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_NO_SLOTS),
        Err(RngdError::Send) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_SEND),
        Err(RngdError::NoReply) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_RECV),
        Err(RngdError::WrongOp) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_WRONG_OP),
        Ok(reply) if reply.status != 0 => {
            emit_bytes(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_STATUS.as_bytes());
            emit_hex_u64(reply.status as u64);
            emit_line(")");
        }
        Ok(reply) if reply.entropy.len() != 32 => {
            emit_bytes(crate::markers::M_SELFTEST_RNG_ENTROPY_FAIL_LEN.as_bytes());
            emit_hex_u64(reply.entropy.len() as u64);
            emit_line(")");
        }
        // SECURITY: Do NOT log entropy bytes!
        Ok(_) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OK),
    }
}

/// Test rngd rejects oversized entropy requests.
/// Proves: bounds enforcement on entropy length (rngd answers status 1).
pub(crate) fn rng_entropy_oversized_selftest() {
    let nonce = (nexus_abi::nsec().unwrap_or(0) as u32) ^ 0x5A5A_A5A5;
    let client = match rngd::client() {
        Ok(c) => c,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_NO_SLOTS);
            return;
        }
    };
    match rngd::get_entropy(&client, 257, nonce) {
        Err(RngdError::NoSlots) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_NO_SLOTS)
        }
        Err(RngdError::Send) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_SEND)
        }
        Err(RngdError::NoReply) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_RECV)
        }
        Err(RngdError::WrongOp) => {
            emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_WRONG_OP)
        }
        Ok(reply) if reply.status != 1 => {
            emit_bytes(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_FAIL_STATUS.as_bytes());
            emit_hex_u64(reply.status as u64);
            emit_line(")");
        }
        Ok(_) => emit_line(crate::markers::M_SELFTEST_RNG_ENTROPY_OVERSIZED_OK),
    }
}
