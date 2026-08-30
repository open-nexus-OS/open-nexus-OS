// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build-baked loader trust anchor (RFC-0089 §7; RFC-0088
//! BAKED_TRUST pattern). `policies/os-trust.toml` is parsed at build time
//! (build.rs fails closed on any violation) into `BAKED_OS_KEYS`; NXBD
//! signatures are accepted from these Ed25519 keys ONLY. Honest label:
//! QEMU-soft-root — as strong as the integrity of nxboot.bin itself.
//! OWNERS: @security
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: tests/trust_bake.rs (bake↔file integration, parser rejects)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

include!(concat!(env!("OUT_DIR"), "/os_trust_baked.rs"));

use bootfmt::nxbd::{self, Nxbd};
use bootfmt::FmtError;

/// Verifies an NXBD sector against the baked anchor set. First key that
/// verifies wins; an empty anchor set or no matching key is `Signature`
/// (deny by default — reason vocabulary `sig`).
pub fn verify_nxbd(sector: &[u8]) -> Result<Nxbd, FmtError> {
    for key in BAKED_OS_KEYS {
        match nxbd::verify(sector, key) {
            Ok(desc) => return Ok(desc),
            Err(FmtError::Signature) => continue,
            // Structural rejects (magic/version/reserved) are final — no
            // point trying other keys against a malformed sector.
            Err(err) => return Err(err),
        }
    }
    Err(FmtError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_set_is_nonempty_and_wellformed() {
        assert!(!BAKED_OS_KEYS.is_empty(), "empty anchor would deny every boot");
    }

    #[test]
    fn rejects_unsigned_and_garbage_sectors() {
        assert!(verify_nxbd(&[0u8; 512]).is_err(), "zeroed NXBD is an invalid slot");
        assert!(verify_nxbd(&[0xA5u8; 512]).is_err());
        assert!(verify_nxbd(&[]).is_err());
    }
}
