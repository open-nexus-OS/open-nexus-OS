// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The selftest client's ONE socd exchange (`BRING_UP`, RFC-0106): the
//! harness asks the SoC glue owner to bring up the node it holds a window for and
//! expects the honest verdict of the machine it runs on — `NOT_NEEDED` on a tree
//! without providers (QEMU virt), `OK` on the board. The exchange is the shared client
//! (`nexus_ipc::socd`); the reply is matched by nonce on the declared shared pair.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU `SELFTEST: soc glue not needed ok` (virt), the board lane later
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_ipc::KernelClient;
use nexus_wire::soc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SocdError {
    NoSlots,
    Send,
    NoReply,
    WrongOp,
}

pub(crate) fn client() -> Result<KernelClient, SocdError> {
    let route = nexus_service_topology::slots::selftest_client::SOCD;
    KernelClient::new_with_slots(route.send, route.recv).map_err(|_| SocdError::NoSlots)
}

/// One `BRING_UP` exchange for `path`, correlated by `nonce` — the shared client
/// (`nexus_ipc::socd`, TASK-0246 P4c) over the harness's declared route.
pub(crate) fn bring_up(
    client: &KernelClient,
    path: &str,
    nonce: u32,
) -> Result<soc::BringUpReply, SocdError> {
    let (send_slot, _) = client.slots();
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    nexus_ipc::socd::bring_up(send_slot, reply, path, nonce).map_err(|e| match e {
        nexus_ipc::socd::SocError::Encode | nexus_ipc::socd::SocError::Send => SocdError::Send,
        nexus_ipc::socd::SocError::NoReply => SocdError::NoReply,
        nexus_ipc::socd::SocError::Reply => SocdError::WrongOp,
    })
}
