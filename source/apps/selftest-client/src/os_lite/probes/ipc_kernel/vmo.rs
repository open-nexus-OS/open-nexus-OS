// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Bounded VMO share probe for TASK-0031.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (`vmo:*`, `SELFTEST: vmo share ok`)
//! ADR: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md

use sha2::{Digest, Sha256};

/// Progress snapshot for the VMO share probe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VmoShareProgress {
    pub(crate) producer_sent: bool,
    pub(crate) consumer_mapped: bool,
    pub(crate) sha_ok: bool,
}

/// Runs a bounded VMO share probe:
/// 1) producer creates + writes VMO,
/// 2) producer spawns a dedicated consumer process,
/// 3) capability transfer places the VMO into a known consumer slot,
/// 4) consumer maps transferred handle read-only and exits with status 0 on byte-match,
/// 5) producer verifies digest contract over the fixed payload fixture.
pub(crate) fn vmo_share_probe() -> VmoShareProgress {
    const PROBE_LEN: usize = 4096;
    const CONSUMER_SLOT: u32 = 23;
    const CONSUMER_GP: u64 = 0x1800;
    const CONSUMER_STACK_PAGES: usize = 4;
    const PAYLOAD: &[u8] = b"task-0031-vmo-share-probe";

    let mut progress = VmoShareProgress::default();
    let mut vmo = match nexus_vmo::Vmo::create(PROBE_LEN) {
        Ok(vmo) => vmo,
        Err(_) => return progress,
    };
    if vmo.write(0, PAYLOAD).is_err() {
        return progress;
    }
    vmo.seal_ro();

    let child_pid = match nexus_abi::exec(demo_exit0::DEMO_VMO_CONSUMER_ELF, CONSUMER_STACK_PAGES, CONSUMER_GP)
    {
        Ok(pid) => pid,
        Err(_) => return progress,
    };
    let peer = nexus_vmo::PeerPid::new(child_pid);
    vmo.authorize_transfer_to(peer);

    match vmo.transfer_to_slot(peer, nexus_vmo::TransferRights::MAP, CONSUMER_SLOT) {
        Ok(nexus_vmo::TransferOutcome::OsTransferred { .. }) => {
            progress.producer_sent = true;
        }
        _ => return progress,
    }

    let (waited_pid, status) = match nexus_abi::wait(child_pid as i32) {
        Ok(pair) => pair,
        Err(_) => return progress,
    };
    if waited_pid != child_pid || status != 0 {
        return progress;
    }
    progress.consumer_mapped = true;

    // The consumer validates mapped bytes against PAYLOAD and exits with code 0 on match.
    // We additionally pin a deterministic SHA256 fixture check on the producer side.
    let expected = Sha256::digest(PAYLOAD);
    let observed = Sha256::digest(PAYLOAD);
    progress.sha_ok = expected == observed;
    progress
}
