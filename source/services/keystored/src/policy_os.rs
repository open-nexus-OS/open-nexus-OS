// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: keystored's delegated capability check (os-lite). Split out of `os_stub.rs` under
//! the module-size ratchet (TASK-0324 P4f-2), the same shape statefsd's `policy_os.rs` has.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: QEMU ladder (`SELFTEST: keystored v1 ok`, device-key persist chain)

use nexus_ipc::reqrep::ReplyBuffer;

/// Deny-by-default check of `cap` for `subject_id` over keystored's declared policyd leg.
///
/// Before the declaration policyd sat at slot 9 only because the optional logd leg was
/// transferred first — without logd, these checks would have gone to rngd's slot.
pub(crate) fn policyd_allows(
    _pending: &mut ReplyBuffer<16, 512>,
    subject_id: u64,
    cap: &[u8],
) -> bool {
    use nexus_service_topology::slots::keystored as topo;
    matches!(
        nexus_ipc::policyd::check_cap_on(
            topo::POLICYD.send,
            topo::REPLY.send,
            topo::REPLY.recv,
            subject_id,
            cap,
        ),
        nexus_ipc::policyd::CapDecision::Allow
    )
}
