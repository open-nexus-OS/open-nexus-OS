// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the painter's kernels on this machine (ADR-0099 rule 4, TASK-0362 P4). A program
//!   chooses the vector forms of `nexus-paint-kernels` from the kernel's answer
//!   (`nexus_abi::sched::offers`) — this one does it the way app-host does, then runs the
//!   crate's own proof: 8 192 spans no caller picks (any length, any byte offset, every alpha),
//!   each painted by the kernels as chosen and by the scalar reference. `SELFTEST: paint
//!   kernels ok (` only when no span differs AND the digest of what the kernels left is the
//!   number the scalar reference yields on the host — the same line on a machine with 16-byte
//!   vector registers, with 32-byte ones and with none.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) and the board —
//!   `SELFTEST: paint kernels ok (`; the proof itself is host-tested in `nexus-paint-kernels`.
//! ADR: docs/adr/0099-every-userspace-program-is-built-for-one-hard-float-target-vectors-are-asked-for-at-run-time.md

use nexus_paint_kernels::{proof, Unit};

use crate::markers::{self, emit_bytes, emit_hex_u64, emit_line, emit_u64};

/// Chooses the kernels from the kernel's answer and proves them against their reference.
pub(crate) fn paint_kernels_proof() {
    let width = nexus_abi::sched::offers().ok().and_then(|o| o.vector_bytes());
    let runs =
        nexus_paint_kernels::choose(if width.is_some() { Unit::Vector } else { Unit::Scalar });
    let vector = runs == Unit::Vector;
    let result = proof::run();
    let held = result == Ok(proof::DIGEST);
    let verdict = if held {
        markers::M_SELFTEST_PAINT_KERNELS_OK
    } else {
        markers::M_SELFTEST_PAINT_KERNELS_FAIL
    };
    emit_bytes(verdict.as_bytes());
    emit_bytes(b"vector=");
    emit_u64(u64::from(vector));
    emit_bytes(b" width=");
    emit_u64(width.filter(|_| vector).unwrap_or(0) as u64);
    emit_bytes(b" spans=");
    emit_u64(u64::from(proof::SPANS));
    match result {
        Ok(digest) => {
            emit_bytes(b" digest=0x");
            emit_hex_u64(digest);
        }
        Err(d) => {
            emit_bytes(if d.blend { b" differs=blend span=" } else { b" differs=fill span=" });
            emit_u64(u64::from(d.span));
            emit_bytes(b" at=");
            emit_u64(d.at as u64);
            emit_bytes(b" len=");
            emit_u64(d.len as u64);
            emit_bytes(b" colour=0x");
            emit_hex_u64(u64::from(u32::from_le_bytes(d.colour)));
        }
    }
    emit_line(")");
}
