// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The selftest client's ONE socd exchange (`BRING_UP`, RFC-0106): the
//! harness asks the SoC glue owner to bring up the node it holds a window for and
//! expects the honest verdict of the machine it runs on — `NOT_NEEDED` on a tree
//! without providers (QEMU virt), `OK` on the board. The wire is `nexus_wire::soc`;
//! the reply is matched by nonce on the declared shared pair.
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

/// One `BRING_UP` exchange for `path`, correlated by `nonce`.
pub(crate) fn bring_up(
    client: &KernelClient,
    path: &str,
    nonce: u32,
) -> Result<soc::BringUpReply, SocdError> {
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, nonce, path).ok_or(SocdError::Send)?;
    let (send_slot, _) = client.slots();
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let mut buf = [0u8; 32];
    let rsp = nexus_ipc::exchange::call_matching(send_slot, reply, &req[..n], &mut buf, |rsp| {
        let ours = rsp.len() >= 9
            && rsp[0] == soc::MAGIC0
            && rsp[1] == soc::MAGIC1
            && rsp[2] == soc::VERSION
            && u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]) == nonce;
        ours.then(|| rsp.to_vec())
    })
    .map_err(|e| match e {
        nexus_ipc::IpcError::Disconnected => SocdError::NoReply,
        _ => SocdError::Send,
    })?;
    soc::decode_bring_up_rsp(&rsp).ok_or(SocdError::WrongOp)
}
