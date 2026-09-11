// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The declarations themselves — the required route graph and the per-service
//! spec (what init must provision, which slots carry it, what the service may call).
//! OWNERS: @runtime
//! STATUS: Production
//! API_STABILITY: Stable within the workspace
//! TEST_COVERAGE: crate tests in lib.rs + `nexus-init` route/policy cross-checks

use crate::routes::Route;
use crate::{NamedSlotBinding, ServiceId, SlotPair};

/// Per-service expectations the orchestrator must satisfy (RFC-0066/0069). A
/// service that `exposes_server` must be given a server endpoint by init;
/// `routes_to` must each appear in [`REQUIRED_ROUTES`]. This is the declaration
/// the data-driven orchestrator consumes to wire init generically.
#[derive(Clone, Copy, Debug)]
pub struct ServiceSpec {
    /// The service.
    pub id: ServiceId,
    /// Init must provision a server endpoint (recv/send slots) for it.
    pub exposes_server: bool,
    /// Init must provision a CAP_MOVE reply inbox for its outbound calls.
    pub reply_inbox: bool,
    /// Services it must be able to call (each must be in `REQUIRED_ROUTES`).
    pub routes_to: &'static [Route],
    /// Emit the `init: <svc> slots …` / `route->… ok` wire markers. True only
    /// where the pre-migration bespoke arm printed them — migrated arms keep
    /// byte-identical boot logs (RFC-0069 migration discipline).
    pub announce: bool,
    /// The service's OWN server endpoint slots (the pair init transfers into it).
    pub server_slots: SlotPair,
    /// The shared CAP_MOVE reply inbox slots, when `reply_inbox` is set.
    pub reply_slots: SlotPair,
    /// Everything that is neither a server pair nor a route: device MMIO, IRQ notify,
    /// settings/watch channels, the stage fence.
    pub extra_slots: &'static [NamedSlotBinding],
}

/// The declared specs for services that participate in the v6b chain. Grown
/// incrementally; the host tests keep it consistent with `REQUIRED_ROUTES`.
pub const SERVICE_SPECS: &[ServiceSpec] = &[
    crate::specs_app::ABILITYMGR,
    crate::specs_app::BUNDLEMGRD,
    crate::specs_app::METRICSD,
    crate::specs_security::NETSTACKD,
    crate::specs_security::DSOFTBUSD,
    crate::specs_storage::UPDATED,
    crate::specs_app::EXECD,
    crate::specs_storage::KEYSTORED,
    crate::specs_security::POLICYD,
    crate::specs_ui::HIDRAWD,
    crate::specs_ui::GPUD,
    crate::specs_ui::INPUTD,
    crate::specs_ui::WINDOWD,
    crate::specs_security::RNGD,
    crate::specs_app::TIMED,
    crate::specs_ui::IMED,
    crate::specs_storage::VFSD,
    crate::specs_storage::PACKAGEFSD,
    crate::specs_app::SAMGRD,
    crate::specs_storage::STATEFSD,
    crate::specs_security::INGRESSD,
    crate::specs_storage::VIRTIOBLKD,
    crate::specs_storage::LOGD,
    crate::specs_app::SESSIOND,
    crate::specs_app::SETTINGSD,
    crate::specs_app::PINCHED,
    crate::specs_storage::BOOTCTLD,
];

/// Looks up the declared [`ServiceSpec`] for a service by name, if any. This is
/// what the (data-driven) orchestrator consults to decide what to provision —
/// instead of a bespoke `match` arm per service.
pub fn spec_for(name: &[u8]) -> Option<&'static ServiceSpec> {
    let id = ServiceId::from_name(name)?;
    SERVICE_SPECS.iter().find(|s| s.id == id)
}

/// `true` if init must provision a server endpoint for `name` (declarative).
pub fn exposes_server(name: &[u8]) -> bool {
    spec_for(name).is_some_and(|s| s.exposes_server)
}
