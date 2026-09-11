// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefsd's policy seam decision (RFC-0066 delegated capability check). Split out
//! of `os_lite.rs` (module-size ratchet) when TASK-0324 P3 made an unreachable authority a
//! NAMED outcome instead of a silent deny.
//! OWNERS: @security @runtime
//! STATUS: Production
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladders (`SELFTEST: statefs *`, `!cap-deny`), security_v2_host

pub(crate) fn policyd_allows(subject_id: u64, cap: &[u8]) -> bool {
    // RFC-0066: the shared CAP_MOVE policy check (nexus_ipc::policyd::check_cap_on).
    //
    // TASK-0324 P3: an UNREACHABLE authority still denies (fail-closed), but it is no longer
    // silent. Collapsing "policyd said no" and "policyd did not answer" into one bool hid a
    // real defect for weeks — a transient miss printed only `access denied`, which reads like
    // a policy decision. The witness names the outage so a lane failure points at the
    // control plane instead of the policy file.
    use nexus_service_topology::slots::statefsd as topo;
    match nexus_ipc::policyd::check_cap_on(
        topo::POLICYD.send,
        topo::REPLY.send,
        topo::REPLY.recv,
        subject_id,
        cap,
    ) {
        nexus_ipc::policyd::CapDecision::Allow => true,
        nexus_ipc::policyd::CapDecision::Deny => false,
        nexus_ipc::policyd::CapDecision::Unreachable => {
            let mut line = [0u8; 96];
            let mut len = 0usize;
            for part in [b"statefsd: FAIL policy unreachable cap=".as_slice(), cap] {
                let take = part.len().min(line.len() - len);
                line[len..len + take].copy_from_slice(&part[..take]);
                len += take;
            }
            if len < line.len() {
                line[len] = b'\n';
                len += 1;
            }
            let _ = nexus_abi::debug_write(&line[..len]);
            false
        }
    }
}
