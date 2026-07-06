// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Canonical program hashing.
//!
//! `programHash` = SHA-256 over the **canonical** single-segment message bytes
//! with the `programHash` field set to 32 zero bytes. Writer and verifier both
//! derive that zeroed canonical form through the typed API (copy → zero the
//! field → canonicalize), so the hashed byte string is well-defined on both
//! sides — no pointer arithmetic, no byte searching.

use crate::{ui_ir_capnp::ui_program, IrError, DIGEST_LEN};
use alloc::vec::Vec;
use sha2::{Digest, Sha256};

/// SHA-256 convenience.
#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; DIGEST_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

const ZERO_DIGEST: [u8; DIGEST_LEN] = [0u8; DIGEST_LEN];

/// Canonical single-segment bytes of `root` with `programHash` zeroed.
///
/// # Errors
/// [`IrError::Malformed`] if the message cannot be copied/canonicalized.
pub fn zeroed_canonical_bytes(root: ui_program::Reader<'_>) -> Result<Vec<u8>, IrError> {
    // Copy the program through the typed API so we can zero the hash field.
    let mut copy = capnp::message::Builder::new_default();
    copy.set_root(root).map_err(|_| IrError::Malformed)?;
    {
        let mut program: ui_program::Builder<'_> =
            copy.get_root().map_err(|_| IrError::Malformed)?;
        program.set_program_hash(&ZERO_DIGEST);
    }
    // Canonicalize: single segment, canonical layout.
    let mut canonical = capnp::message::Builder::new_default();
    canonical
        .set_root_canonical(
            copy.get_root_as_reader::<ui_program::Reader<'_>>()
                .map_err(|_| IrError::Malformed)?,
        )
        .map_err(|_| IrError::Malformed)?;
    let segments = canonical.get_segments_for_output();
    if segments.len() != 1 {
        return Err(IrError::Malformed);
    }
    Ok(segments[0].to_vec())
}

/// Computes the canonical program hash for a (typically freshly built) program.
///
/// # Errors
/// [`IrError::Malformed`] if the message cannot be canonicalized.
pub fn compute_program_hash(root: ui_program::Reader<'_>) -> Result<[u8; DIGEST_LEN], IrError> {
    Ok(sha256(&zeroed_canonical_bytes(root)?))
}

/// Verifies the stored `programHash` of a loaded program.
///
/// # Errors
/// [`IrError::BadDigest`] if the stored digest is missing/mis-sized/zero;
/// [`IrError::HashMismatch`] if recomputation differs;
/// [`IrError::Malformed`] if the message cannot be canonicalized.
pub fn verify_program_hash(root: ui_program::Reader<'_>) -> Result<(), IrError> {
    let stored = root.get_program_hash().map_err(|_| IrError::Malformed)?;
    if stored.len() != DIGEST_LEN || stored == ZERO_DIGEST {
        return Err(IrError::BadDigest);
    }
    if compute_program_hash(root)? == stored {
        Ok(())
    } else {
        Err(IrError::HashMismatch)
    }
}
