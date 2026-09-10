// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's view of the declarative service topology. The declarations themselves —
//! service ids, the required route graph, per-service expectations AND their capability
//! slots — live in the `nexus-service-topology` crate (TASK-0324 P4, RFC-0093 §4) so init,
//! every service and the app-child view read ONE source instead of three copies of the same
//! numbers. This module only re-exports them and adds init's own placement helper, so every
//! existing `crate::service_topology::…` path keeps resolving.
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: `service_topology_tests.rs` (route/policy cross-checks) + the crate's own
//! slot tests (`cargo test -p nexus-service-topology`)

pub use nexus_service_topology::*;

/// CPU placement for a service (init applies it at resume; kernel-side comments in
/// `core/trap/runtime.rs` refer to this by name).
pub use crate::affinity::affinity_for;

#[cfg(test)]
#[path = "service_topology_tests.rs"]
mod tests;
