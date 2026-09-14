// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The harness's ONE route ask (`route_with_retry`): a client for a route the
//! topology declares for the selftest, obtained from init's responder through the fleet
//! helper `nexus_ipc::budget::route_with_nonce_budgeted` and CHECKED against the declaration
//! (TASK-0324 P4f-5). Since P7 it is one WAITED ask, not a 64-round poll: routing v2 parks
//! the ask in init until the target is ready (RFC-0093 §1), and the private
//! `routing_v1_get` copy of the exchange (NONBLOCK + `yield_()` against a 500 ms clock) is
//! deleted.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (`SELFTEST: ipc routing <svc> ok`).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::time::Duration;

use nexus_ipc::budget::{route_with_nonce_budgeted, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::KernelClient;
use nexus_service_topology::{route_matches, route_slots, ServiceId};

use crate::markers::{emit_byte, emit_bytes};

/// Liveness bound for the responder's answer (the ask is parked in init while the target
/// is not ready; a responder silent for 2 s is a real outage, ADR-0057).
const ROUTE_LIVENESS: Duration = Duration::from_secs(2);
/// Foreign frames tolerated on the control channel before the ask is given up.
const ROUTE_MISMATCH_BUDGET: NonceMismatchBudget = NonceMismatchBudget::new(64);

/// Asks init's responder for `name` once and returns the declared `(send, recv)` slots.
pub(crate) fn route_slots_from_responder(name: &str) -> core::result::Result<(u32, u32), ()> {
    route_slots_from_responder_within(name, ROUTE_LIVENESS)
}

/// [`route_slots_from_responder`] with a caller-chosen liveness bound: the ask is PARKED in
/// init until the target reports ready (RFC-0093 §1), so waiting on the answer IS waiting
/// for the target's readiness — the harness's readiness gates use it with the bound the
/// target's bring-up warrants (dsoftbusd configures the network first).
pub(crate) fn route_slots_from_responder_within(
    name: &str,
    liveness: Duration,
) -> core::result::Result<(u32, u32), ()> {
    match route_with_nonce_budgeted(name.as_bytes(), liveness, ROUTE_MISMATCH_BUDGET) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Ok((send_slot, recv_slot)),
        _ => Err(()),
    }
}

/// A client for a route the topology declares for this harness, obtained from init's route
/// responder and CHECKED against the declaration (TASK-0324 P4f-5). The
/// `SELFTEST: ipc routing <svc> ok` markers that follow a successful call therefore prove that the
/// responder serves exactly the declared slots. An undeclared name is refused without asking.
pub(crate) fn route_with_retry(name: &str) -> core::result::Result<KernelClient, ()> {
    let target = ServiceId::from_name(name.as_bytes()).ok_or(())?;
    route_slots(ServiceId::SelftestClient, target).ok_or(())?;
    let (send, recv) = route_slots_from_responder(name)?;
    if !route_matches(ServiceId::SelftestClient, target, send, recv) {
        emit_bytes(crate::markers::M_SELFTEST_ROUTE_DIVERGES_FROM_DECLARATION_FAIL_SVC.as_bytes());
        emit_bytes(name.as_bytes());
        emit_byte(b'\n');
        return Err(());
    }
    KernelClient::new_with_slots(send, recv).map_err(|_| ())
}
