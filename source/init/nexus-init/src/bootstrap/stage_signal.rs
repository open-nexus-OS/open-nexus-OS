// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE place a boot stage is advanced on the kernel fence (ADR-0062). The decision
//! lives in the pure, host-tested `crate::stage` ladder; this module only performs it — signal
//! the fence, then print the stage marker AT THE SIGNAL SITE, so the marker is evidence of the
//! fence advancing and not of init reaching a line of code.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P5-b)
//! API_STABILITY: Internal
//! TEST_COVERAGE: ladder logic in `crate::stage` (host); QEMU ladder (`stage: …` markers)
//! ADR: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md

use crate::bootstrap::CtrlChannel;
use crate::ready_table::ReadyTable;
use crate::service_topology::ServiceId;
use crate::stage::StageLadder;

/// Advances every stage whose barrier is now satisfied, one rung at a time. Called after each
/// piece of evidence (`@ready`, `@stage`) — never on a timer, never on resume order.
///
/// A failed signal is loud and does NOT advance the ladder's view: a fleet waiting on a fence
/// that silently never moved is the wedge class this task exists to remove.
pub(crate) fn advance(
    ladder: &mut StageLadder,
    ctrls: &[CtrlChannel],
    ready: &ReadyTable,
    stage_fence: u32,
) {
    let is_ready =
        |svc: ServiceId| ctrls.iter().any(|c| c.svc_name == svc.name() && ready.is_ready(c.pid));
    while let Some(stage) = ladder.try_advance(&is_ready) {
        match nexus_abi::fence_signal(stage_fence, stage.value()) {
            Ok(()) => crate::bootstrap::diag::emit_marker_atomic(
                &[b"stage: ", stage.label().as_bytes()],
                None,
            ),
            Err(err) => {
                crate::bootstrap::diag::emit_marker_atomic(
                    &[b"init: FAIL stage signal ", stage.label().as_bytes()],
                    Some(err as u64),
                );
                return;
            }
        }
    }
}
