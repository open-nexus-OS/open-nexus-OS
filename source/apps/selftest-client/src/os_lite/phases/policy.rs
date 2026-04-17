//! Phase: policy (extracted in Cut P2-07 — allow/deny, MMIO-policy deny, ABI-filter profile, audit verify, policy malformed).

use crate::os_lite::context::PhaseCtx;

#[allow(dead_code)]
pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    Ok(())
}
