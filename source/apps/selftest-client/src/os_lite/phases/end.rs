//! Phase: end (extracted in Cut P2-13 — `SELFTEST: end` + cooperative idle).

use crate::os_lite::context::PhaseCtx;

#[allow(dead_code)]
pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    Ok(())
}
